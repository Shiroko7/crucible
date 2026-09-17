//! Triggered modifiers and reactions (riders).

use super::damage::{DamageKind, DamageRoll};
use super::types::{Ability, Condition, Cost, Duration};
use crate::rules::combat::{Attack, DamageRider, RollMode};

/// What kind of incoming attack a [`Rider::ReactionOnTargeted`] answers.
///
/// One variant today, because nothing in the engine yet distinguishes a
/// melee attack roll from a ranged or spell one the way
/// [`Attack::finesse_or_ranged`] distinguishes weapon properties. The field
/// exists anyway so a future distinction - a reaction that only answers a
/// melee attack, say - is a new variant matched at the same call site, not a
/// new field threaded through every caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackTrigger {
    /// Any attack roll made against this creature.
    AnyAttack,
}

/// A triggered modifier.
///
/// Each variant is a mechanism, not a feature. The comments name the features
/// that map onto it, which is the test of whether the abstraction is pulling
/// its weight: a variant only one ability can use is a branch in disguise.
#[derive(Debug, Clone, PartialEq)]
pub enum Rider {
    /// On a hit, the target saves or takes a condition.
    ///
    /// Stunning Strike. Also every knockdown, every on-hit poison, and the
    /// secondary effect on most breath weapons.
    SaveOrCondition {
        ability: Ability,
        dc: i32,
        condition: Condition,
        duration: Duration,
        cost: Option<Cost>,
        /// Stunning Strike is once per turn however many times you hit.
        once_per_turn: bool,
    },
    /// A successful save against an effect that would deal half takes none
    /// instead, and a failed one takes half.
    ///
    /// Evasion, for the ability the effect names. Danger Sense and a rogue's
    /// Evasion are the same shape.
    NothingOnSuccess { ability: Ability },
    /// Turn a failed save into a success, a fixed number of times per fight.
    ///
    /// Legendary Resistance. A fighter's Indomitable is the same shape with
    /// one use and a reroll instead of a pass.
    AlwaysSucceed { uses: u32 },
    /// A reaction that reduces the damage of an incoming *attack* whose types
    /// include one of `kinds`.
    ///
    /// Deflect Attacks. Uncanny Dodge and Heavy Armor Master are variations.
    ReduceDamage {
        kinds: Vec<DamageKind>,
        roll: DamageRoll,
        /// Reactions refresh at the start of the creature's turn.
        per_round: u32,
    },
    /// A reaction spent on being *targeted* by an attack, before its hit or
    /// miss is finalized, that adds `ac_bonus` to this creature's AC against
    /// that one attack - capable of turning what would have been a hit into
    /// a miss.
    ///
    /// The mirror of [`Rider::ReduceDamage`]: that one reacts to an attack
    /// that already hit, on its damage; this one reacts to being targeted,
    /// before the roll against AC is decided. A reaction that boosts AC
    /// against a targeting attack - the Shield spell, a Ring of Protection's
    /// reactive bonus, and any homebrew item shaped the same way - is this
    /// mechanism; nothing here is specific to any one of them.
    ReactionOnTargeted {
        trigger: AttackTrigger,
        ac_bonus: i32,
        /// Reactions refresh at the start of the creature's turn, the same
        /// as [`Rider::ReduceDamage::per_round`].
        per_round: u32,
    },
    /// Extra damage dice on a hit, gated on the attack roll having advantage
    /// or an ally next to the target - and never at all if the attacker also
    /// has disadvantage, which overrides an ally in place. Spendable once per
    /// turn if `once_per_turn`.
    ///
    /// Sneak Attack. The gate itself is evaluated by whoever resolves the
    /// attack, from flags on the attack rather than derived geometry - see
    /// [`crate::rules::combat::Attack::ally_adjacent`] and
    /// [`crate::rules::combat::Attack::finesse_or_ranged`] for why. This
    /// variant only carries the dice pool and the once-per-turn budget, so
    /// anything else that shares the exact same gate is this variant too,
    /// not a new branch.
    ConditionalExtraDamage {
        dice_count: u32,
        dice_sides: u32,
        once_per_turn: bool,
    },
    /// On a hit, unconditionally applies a condition to the target - no
    /// saving throw offered, unlike [`Rider::SaveOrCondition`].
    ///
    /// Guiding Bolt's mark:
    /// [`crate::rules::creature::Condition::Marked`], granting Advantage to
    /// the next attack roll made against the target by anyone, cleared the
    /// moment that roll happens (see `sim::duel::Fight`'s attack resolution)
    /// or at the start of the target's own next turn, whichever comes first.
    /// Distinct from `SaveOrCondition` because nothing about the mark is
    /// resistible - it lands whenever the attack does - and it carries no
    /// cost or once-per-turn budget of its own; the spell's own casting cost
    /// (a spell slot) already gates it.
    ConditionOnHit {
        condition: Condition,
        duration: Duration,
    },
    /// Marks a creature whose [`Rider::ConditionalExtraDamage`] also accepts
    /// a qualifying spell attack roll
    /// ([`crate::rules::combat::Attack::is_spell_attack`]), not only a
    /// finesse-or-ranged weapon attack
    /// ([`crate::rules::combat::Attack::finesse_or_ranged`]).
    ///
    /// Most creatures with `ConditionalExtraDamage` do not carry this - it
    /// is the "some builds grant a feature that lets a non-weapon spell
    /// attack also qualify" extension, gated the same way
    /// [`Rider::NothingOnSuccess`]'s evasion check is: a second rider in the
    /// same list, queried by [`crate::rules::creature::Creature::extra_damage_applies_to_spell_attacks`]
    /// rather than a field added to `ConditionalExtraDamage` itself, so a
    /// creature can carry the extension without every existing
    /// `ConditionalExtraDamage` construction site needing to know about it.
    /// A no-op on its own; it only changes what
    /// [`Rider::extra_damage_for_with_spell_attack_extension`] does with a
    /// sibling `ConditionalExtraDamage` rider.
    ExtraDamageAppliesToSpellAttacks,
}

impl Rider {
    /// Riders with a budget need somewhere to count it down.
    pub fn initial_uses(&self) -> u32 {
        match self {
            Rider::AlwaysSucceed { uses } => *uses,
            Rider::ReduceDamage { per_round, .. } => *per_round,
            Rider::ReactionOnTargeted { per_round, .. } => *per_round,
            _ => 0,
        }
    }

    /// The [`DamageRider`] this rider contributes to `attack`, or `None` if
    /// it does not apply - either because this variant is not
    /// [`Rider::ConditionalExtraDamage`], or because its gate does not hold.
    ///
    /// The gate (Sneak Attack's, specifically): a finesse or ranged weapon,
    /// on a roll that is not at disadvantage, with either advantage or an
    /// ally next to the target. `used_this_turn` is the once-per-turn
    /// budget, tracked by the caller - the same shared per-creature flag
    /// `sim::duel` already keeps for Stunning-Strike-style riders (see
    /// `README.md`'s note that a creature gets one once-per-turn rider
    /// trigger per turn in total, not one per rider).
    ///
    /// Returns the *full* qualifying dice pool. A feature that spends part
    /// of it on something other than damage (Cunning Strike) reduces
    /// `dice_count` on the result before handing it to
    /// [`Attack::with_damage_rider`] - [`DamageRider`] is a plain count of
    /// dice, so rolling fewer of them is not a special case.
    ///
    /// This is the plain weapon-only gate - equivalent to calling
    /// [`Rider::extra_damage_for_with_spell_attack_extension`] with
    /// `spell_attacks_extended: false`, kept as its own method so the common
    /// case (a creature with no spell-attack extension) reads without an
    /// extra argument that would always be `false` for it.
    pub fn extra_damage_for(&self, attack: &Attack, used_this_turn: bool) -> Option<DamageRider> {
        self.extra_damage_for_with_spell_attack_extension(attack, used_this_turn, false)
    }

    /// As [`Rider::extra_damage_for`], but also accepts a spell attack roll
    /// ([`Attack::is_spell_attack`]) when `spell_attacks_extended` is `true`.
    /// That flag is what a creature carrying
    /// [`Rider::ExtraDamageAppliesToSpellAttacks`] passes in, via
    /// [`crate::rules::creature::Creature::extra_damage_applies_to_spell_attacks`].
    ///
    /// The gate: a finesse-or-ranged weapon attack always qualifies; a spell
    /// attack qualifies only when the extension is present; either way, the
    /// roll still needs advantage or an ally adjacent, and disadvantage
    /// still overrides both. Saving-throw spells never reach this check at
    /// all - they are resolved via `SaveEffect`, not an [`Attack`], so there
    /// is no hit roll and no [`Rider::extra_damage_for`] call in the first
    /// place; this only ever fires from attack-roll resolution.
    pub fn extra_damage_for_with_spell_attack_extension(
        &self,
        attack: &Attack,
        used_this_turn: bool,
        spell_attacks_extended: bool,
    ) -> Option<DamageRider> {
        let Rider::ConditionalExtraDamage {
            dice_count,
            dice_sides,
            once_per_turn,
        } = self
        else {
            return None;
        };
        if *once_per_turn && used_this_turn {
            return None;
        }
        let qualifying_attack =
            attack.finesse_or_ranged || (attack.is_spell_attack && spell_attacks_extended);
        if !qualifying_attack || attack.mode == RollMode::Disadvantage {
            return None;
        }
        if attack.mode == RollMode::Advantage || attack.ally_adjacent {
            Some(DamageRider::new(*dice_count, *dice_sides))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prob::rng::Rng;
    use crate::rules::combat::{damage_pmf, sample_damage, Defense};
    use crate::rules::creature::Creature;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    fn rider() -> Rider {
        Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        }
    }

    /// A weapon attack triggers through the extended gate exactly like it
    /// does through the plain one - the extension flag never widens or
    /// narrows the weapon path, regardless of which way it is set.
    #[test]
    fn a_weapon_attack_is_unaffected_by_the_spell_attack_extension_either_way() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, false),
            Some(DamageRider::new(4, 6))
        );
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, true),
            Some(DamageRider::new(4, 6)),
            "the extension is additive, never a restriction on the weapon path"
        );
    }

    /// A spell attack qualifies only when the creature carries the
    /// extension - the "some builds have this, most don't" gate the marker
    /// rider exists for.
    #[test]
    fn a_spell_attack_triggers_only_with_the_extension() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true);
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, true),
            Some(DamageRider::new(4, 6)),
            "a spell attack should qualify once the extension is present"
        );
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, false),
            None,
            "a spell attack must not qualify without the extension"
        );
    }

    /// The plain two-argument [`Rider::extra_damage_for`] never grants the
    /// extension - a spell attack never triggers through it, which is
    /// exactly the "no argument means `false`" contract
    /// [`Rider::extra_damage_for_with_spell_attack_extension`]'s doc comment
    /// promises.
    #[test]
    fn the_plain_gate_never_extends_to_spell_attacks() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true);
        assert_eq!(rider().extra_damage_for(&attack, false), None);
    }

    /// Disadvantage still overrides everything, spell attack or not.
    #[test]
    fn disadvantage_still_overrides_a_spell_attack_with_the_extension() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Disadvantage)
            .with_is_spell_attack(true)
            .with_ally_adjacent(true);
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, true),
            None
        );
    }

    /// [`Creature::extra_damage_applies_to_spell_attacks`] reads the marker
    /// straight off the creature's own rider list, the same way
    /// [`Creature::has_evasion`] reads [`Rider::NothingOnSuccess`].
    #[test]
    fn a_creature_reports_the_extension_only_when_it_carries_the_marker_rider() {
        let plain = Creature::new("Plain Rogue", 15, 40).with_rider(rider());
        assert!(!plain.extra_damage_applies_to_spell_attacks());

        let extended = Creature::new("Extended Build", 15, 40)
            .with_rider(rider())
            .with_rider(Rider::ExtraDamageAppliesToSpellAttacks);
        assert!(extended.extra_damage_applies_to_spell_attacks());
    }

    /// The exact and sampled paths agree across the weapon-attack, extended
    /// spell-attack, and unextended spell-attack cases - the same kind of
    /// trigger matrix `dsl::plugin::rogue`'s sneak attack test covers for
    /// the plain weapon-only gate, extended here to the new spell-attack
    /// cases.
    #[test]
    fn sampled_extra_damage_agrees_with_the_exact_path_across_spell_attack_cases() {
        let defense = Defense::new(14, 60);
        let cases = [
            (
                "weapon attack, advantage: triggers regardless of the extension",
                Attack::new(6, 1, 8, 4)
                    .with_mode(RollMode::Advantage)
                    .with_finesse_or_ranged(true),
                false,
            ),
            (
                "spell attack, advantage, with the extension: triggers",
                Attack::new(6, 1, 8, 4)
                    .with_mode(RollMode::Advantage)
                    .with_is_spell_attack(true),
                true,
            ),
            (
                "spell attack, advantage, without the extension: does not trigger",
                Attack::new(6, 1, 8, 4)
                    .with_mode(RollMode::Advantage)
                    .with_is_spell_attack(true),
                false,
            ),
        ];
        for (seed, (name, attack, spell_attacks_extended)) in cases.into_iter().enumerate() {
            let attack = match rider().extra_damage_for_with_spell_attack_extension(
                &attack,
                false,
                spell_attacks_extended,
            ) {
                Some(extra) => attack.with_damage_rider(extra),
                None => attack,
            };
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 1_300);
            let n = 100_000;
            let mut counts = vec![0usize; (hi - lo + 1) as usize];
            for _ in 0..n {
                let d = sample_damage(&mut rng, &attack, &defense);
                assert!(
                    d >= lo && d <= hi,
                    "{name}: sampled {d} outside {lo}..={hi}"
                );
                counts[(d - lo) as usize] += 1;
            }
            for (i, &c) in counts.iter().enumerate() {
                let value = lo + i as i32;
                let want = exact.prob(value);
                let got = c as f64 / n as f64;
                let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
                assert!(
                    (got - want).abs() < tol,
                    "{name}: P(damage = {value}) sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
                );
            }
        }
        // The weapon path and the extended spell-attack path should agree
        // exactly once both qualify: the gate is an OR over roll type, not
        // a different amount of damage for one kind or the other.
        let weapon = damage_pmf(
            &Attack::new(6, 1, 8, 4)
                .with_mode(RollMode::Advantage)
                .with_finesse_or_ranged(true)
                .with_damage_rider(DamageRider::new(4, 6)),
            &defense,
        );
        let spell = damage_pmf(
            &Attack::new(6, 1, 8, 4)
                .with_mode(RollMode::Advantage)
                .with_is_spell_attack(true)
                .with_damage_rider(DamageRider::new(4, 6)),
            &defense,
        );
        assert!(close(weapon.mean(), spell.mean()));
    }
}
