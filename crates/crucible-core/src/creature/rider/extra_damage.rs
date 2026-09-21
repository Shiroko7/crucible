//! Riders that add damage dice to a hit: Sneak Attack's gate (and its
//! spell-attack extension), bonus dice against a creature type, and a weapon
//! buff armed by landing a condition.

use crate::creature::{AttackKind, Rider};
use crate::rules::{Attack, Condition, CreatureType, DamageKind, DamageRider, RollMode};

impl Rider {
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
    /// [`crate::creature::Creature::extra_damage_applies_to_spell_attacks`].
    ///
    /// The gate: a finesse-or-ranged weapon attack always qualifies; a spell
    /// attack qualifies only when the extension is present; either way, the
    /// roll still needs advantage or an ally adjacent, and disadvantage
    /// still overrides both. Saving-throw spells never reach this check at
    /// all - they are resolved via `SaveEffect`, not an [`Attack`], so there
    /// is no hit roll and no [`Rider::extra_damage_for`] call in the first
    /// place; this only ever fires from attack-roll resolution.
    ///
    /// The damage type: when `attack` carries an
    /// [`Attack::spell_damage_kind`] - set only for a spell attack - the
    /// returned [`DamageRider::kind`] is pinned to match it, the 5e rule
    /// that this kind of extra damage takes on the triggering *spell's* own
    /// damage type rather than a fixed default. A weapon attack never sets
    /// `spell_damage_kind`, so it falls through unchanged: the rider's
    /// `kind` stays `None` and it is reduced exactly like the rest of the
    /// hit, same as before this distinction existed.
    pub fn extra_damage_for_with_spell_attack_extension(
        &self,
        attack: &Attack,
        used_this_turn: bool,
        spell_attacks_extended: bool,
    ) -> Option<DamageRider> {
        let rider = self.conditional_extra_damage(
            attack.finesse_or_ranged,
            attack.is_spell_attack && spell_attacks_extended,
            attack.mode,
            attack.ally_adjacent,
            used_this_turn,
        )?;
        Some(match attack.spell_damage_kind {
            Some(kind) => rider.with_kind(kind),
            None => rider,
        })
    }

    /// The same gate as [`Rider::extra_damage_for_with_spell_attack_extension`],
    /// read off a [`crate::creature::Strike`]'s [`AttackKind`] rather than an
    /// [`Attack`]'s flags - the form `sim::duel` resolves every live attack
    /// roll in. `mode` is the roll's final mode, after every source of
    /// advantage and disadvantage has cancelled.
    ///
    /// The returned [`DamageRider`] carries `damage_kind` as its type: the
    /// weapon's own type for a weapon attack, the spell's for a spell attack
    /// (Sneak Attack's "same type as the weapon", and the spell-attack
    /// extension's "same type as the spell's damage") - both of which are
    /// just the triggering strike's primary damage type.
    pub fn extra_damage_for_strike(
        &self,
        kind: AttackKind,
        damage_kind: Option<DamageKind>,
        mode: RollMode,
        ally_adjacent: bool,
        used_this_turn: bool,
        spell_attacks_extended: bool,
    ) -> Option<DamageRider> {
        let rider = self.conditional_extra_damage(
            kind.finesse_or_ranged_weapon(),
            kind.spell && spell_attacks_extended,
            mode,
            ally_adjacent,
            used_this_turn,
        )?;
        Some(match damage_kind {
            Some(k) => rider.with_kind(k),
            None => rider,
        })
    }

    /// The gate itself, shared by both attack models: a qualifying roll
    /// (`weapon_qualifies` or `spell_qualifies`) that is not at disadvantage
    /// and has either advantage or an ally next to the target, with the
    /// once-per-turn budget unspent.
    fn conditional_extra_damage(
        &self,
        weapon_qualifies: bool,
        spell_qualifies: bool,
        mode: RollMode,
        ally_adjacent: bool,
        used_this_turn: bool,
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
        if !(weapon_qualifies || spell_qualifies) || mode == RollMode::Disadvantage {
            return None;
        }
        if mode != RollMode::Advantage && !ally_adjacent {
            return None;
        }
        Some(DamageRider::new(*dice_count, *dice_sides))
    }

    /// The [`DamageRider`] this rider contributes against a target of
    /// `target_creature_type`, or `None` if this variant is not
    /// [`Rider::BonusDamageVsCreatureType`] or the target's type does not
    /// match - including a target with no declared type at all, which can
    /// never match a specific one.
    ///
    /// Takes the type rather than the whole target [`crate::creature::Creature`]
    /// because the gate only ever needs that one field - the same shape as
    /// [`Rider::extra_damage_for`] taking the attack's own flags rather than
    /// the whole [`Attack`]'s owner.
    ///
    /// Composes with any other active rider rather than replacing it: this
    /// and [`Rider::extra_damage_for`] each contribute their own
    /// [`DamageRider`] to the same [`Attack::damage_riders`] list, so a
    /// qualifying hit from a rogue wielding a favoured weapon gets both.
    pub fn bonus_damage_vs_creature_type(
        &self,
        target_creature_type: Option<CreatureType>,
    ) -> Option<DamageRider> {
        let Rider::BonusDamageVsCreatureType {
            dice_count,
            dice_sides,
            bonus,
            creature_type,
            ..
        } = self
        else {
            return None;
        };
        if target_creature_type != Some(*creature_type) {
            return None;
        }
        Some(DamageRider::new(*dice_count, *dice_sides).with_bonus(*bonus))
    }

    /// Does inflicting `condition` on a target via a weapon attack arm this
    /// rider's buff? `false` for every rider variant except
    /// [`Rider::ConditionTriggeredWeaponDamage`], and for that variant unless
    /// `condition` is exactly its `trigger`.
    pub fn arms_on_condition(&self, condition: Condition) -> bool {
        matches!(
            self,
            Rider::ConditionTriggeredWeaponDamage { trigger, .. } if *trigger == condition
        )
    }

    /// The [`DamageRider`] this rider contributes to a weapon attack once its
    /// buff has fired, or `None` if this is not
    /// [`Rider::ConditionTriggeredWeaponDamage`] or `armed` is `false`.
    ///
    /// `armed` is per-fight state the caller tracks - see
    /// [`Rider::arms_on_condition`] - the same shape
    /// [`Rider::extra_damage_for`] already takes `used_this_turn` as an
    /// external gate rather than deciding it from data stored on the rider
    /// itself.
    pub fn weapon_damage_if_armed(&self, armed: bool) -> Option<DamageRider> {
        let Rider::ConditionTriggeredWeaponDamage {
            dice_count,
            dice_sides,
            bonus,
            ..
        } = self
        else {
            return None;
        };
        if !armed {
            return None;
        }
        Some(DamageRider::new(*dice_count, *dice_sides).with_bonus(*bonus))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Creature;
    use crate::prob::Rng;
    use crate::rules::{damage_pmf, sample_damage, Defense, Reduction};

    fn sneak_attack() -> Rider {
        Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    fn slaying_rider() -> Rider {
        Rider::BonusDamageVsCreatureType {
            dice_count: 3,
            dice_sides: 6,
            bonus: 0,
            damage_kind: DamageKind::Piercing,
            creature_type: CreatureType::Dragon,
        }
    }

    #[test]
    fn applies_only_against_a_matching_creature_type() {
        let got = slaying_rider().bonus_damage_vs_creature_type(Some(CreatureType::Dragon));
        assert_eq!(got, Some(DamageRider::new(3, 6)));
    }

    #[test]
    fn does_not_apply_against_a_different_creature_type() {
        assert_eq!(
            slaying_rider().bonus_damage_vs_creature_type(Some(CreatureType::Giant)),
            None
        );
    }

    #[test]
    fn does_not_apply_against_a_target_with_no_declared_type() {
        assert_eq!(slaying_rider().bonus_damage_vs_creature_type(None), None);
    }

    #[test]
    fn a_non_matching_rider_variant_never_contributes_this_gate() {
        let sneak = Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        };
        assert_eq!(
            sneak.bonus_damage_vs_creature_type(Some(CreatureType::Dragon)),
            None
        );
    }

    /// The acceptance criterion this task turns on: a qualifying hit stacks
    /// this rider's dice with Sneak Attack's rather than either replacing the
    /// other, because both riders contribute separately to the same
    /// `Attack::damage_riders` list.
    #[test]
    fn stacks_with_sneak_attack_on_the_same_qualifying_hit() {
        let sneak = Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        };
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);

        let sneak_extra = sneak
            .extra_damage_for(&attack, false)
            .expect("sneak attack qualifies");
        let slaying_extra = slaying_rider()
            .bonus_damage_vs_creature_type(Some(CreatureType::Dragon))
            .expect("the target is a dragon");

        let neither = attack.clone();
        let both = attack
            .clone()
            .with_damage_rider(sneak_extra)
            .with_damage_rider(slaying_extra);

        let defense = Defense::new(1, 200); // AC 1: every non-fumble roll hits
        let mean_neither = damage_pmf(&neither, &defense).mean();
        let mean_both = damage_pmf(&both, &defense).mean();
        assert!(
            mean_both > mean_neither,
            "both riders' dice should raise the mean damage over neither firing"
        );

        // Two separate d6 riders (4d6 and 3d6) convolve to exactly the same
        // distribution as one 7d6 rider - dice of the same size are additive
        // under convolution, doubling on a crit included - so comparing
        // against that single merged rider is an exact check that both
        // riders' dice are present and neither replaced the other.
        let merged_reference = attack.with_damage_rider(DamageRider::new(4 + 3, 6));
        let pmf_both = damage_pmf(&both, &defense);
        let pmf_merged = damage_pmf(&merged_reference, &defense);
        assert!(close(pmf_both.mean(), pmf_merged.mean()));
        assert_eq!(pmf_both.min(), pmf_merged.min());
        assert_eq!(pmf_both.max(), pmf_merged.max());

        // And the max damage is the base plus every die maxed, doubled on a
        // crit exactly like the base pool - `rider_pmf` handles both riders
        // identically, so there is nothing special about there being two.
        let base_crit_max = 2 * 4; // 1d4 base, doubled on a crit
        let riders_crit_max = 2 * (4 * 6) + 2 * (3 * 6);
        assert_eq!(
            pmf_both.max(),
            base_crit_max + riders_crit_max + 3 /* damage_bonus */
        );
    }

    #[test]
    fn sampled_bonus_vs_creature_type_agrees_with_the_exact_path_alone_and_stacked() {
        let defense = Defense::new(14, 80);
        let sneak = Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
        };
        let attack = Attack::new(6, 1, 8, 4)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);

        let slaying_extra = slaying_rider()
            .bonus_damage_vs_creature_type(Some(CreatureType::Dragon))
            .expect("the target is a dragon");
        let sneak_extra = sneak
            .extra_damage_for(&attack, false)
            .expect("sneak attack qualifies");

        let cases = [
            (
                "vs-creature-type rider alone",
                attack.clone().with_damage_rider(slaying_extra),
            ),
            (
                "stacked with sneak attack",
                attack
                    .clone()
                    .with_damage_rider(slaying_extra)
                    .with_damage_rider(sneak_extra),
            ),
        ];

        for (seed, (name, attack)) in cases.into_iter().enumerate() {
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 1300);
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

    /// A spell attack that declares its own damage type pins the returned
    /// rider's [`DamageRider::kind`] to match it - the 5e rule this
    /// mechanism exists for: this kind of extra damage takes on the
    /// triggering *spell's* damage type, not a fixed default.
    #[test]
    fn a_spell_attacks_declared_kind_overrides_the_riders_damage_type() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true)
            .with_spell_damage_kind(DamageKind::Radiant);
        assert_eq!(
            rider().extra_damage_for_with_spell_attack_extension(&attack, false, true),
            Some(DamageRider::new(4, 6).with_kind(DamageKind::Radiant)),
            "the rider's damage type should match the triggering spell's"
        );
    }

    /// A weapon attack never declares a `spell_damage_kind` - the field is
    /// only ever set on a spell attack - so the rider it triggers keeps
    /// `kind: None`: reduced exactly like the rest of the hit, unaffected by
    /// this mechanism, same as before it existed.
    #[test]
    fn a_weapon_attack_never_gets_a_damage_type_override() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        let extra = rider()
            .extra_damage_for_with_spell_attack_extension(&attack, false, false)
            .expect("a finesse-or-ranged weapon attack at advantage qualifies");
        assert_eq!(
            extra.kind, None,
            "a weapon-triggered rider's damage type is unaffected by this mechanism"
        );
    }

    /// A spell attack that never declares a damage type is treated the same
    /// as a weapon attack: the rider's `kind` stays `None` rather than
    /// defaulting to something invented.
    #[test]
    fn a_spell_attack_with_no_declared_kind_leaves_the_riders_type_unaffected() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true);
        let extra = rider()
            .extra_damage_for_with_spell_attack_extension(&attack, false, true)
            .expect("qualifies via the extension");
        assert_eq!(extra.kind, None);
    }

    /// The acceptance case for this whole mechanism: the SAME rider,
    /// triggered off two attacks that differ only in which spell cast them,
    /// deals different net damage against a target that resists one of
    /// those spells' damage types but not the other. If this only changed a
    /// label on the rider and never reached damage reduction, `radiant` and
    /// `force` below would come out equal.
    #[test]
    fn the_same_rider_deals_different_net_damage_depending_on_the_triggering_spells_type() {
        let defense =
            Defense::new(1, 60).with_kind_reduction(DamageKind::Radiant, Reduction::Resistant);

        let radiant_spell = Attack::new(7, 1, 4, 0)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true)
            .with_spell_damage_kind(DamageKind::Radiant);
        let force_spell = Attack::new(7, 1, 4, 0)
            .with_mode(RollMode::Advantage)
            .with_is_spell_attack(true)
            .with_spell_damage_kind(DamageKind::Force);

        let radiant_extra = rider()
            .extra_damage_for_with_spell_attack_extension(&radiant_spell, false, true)
            .expect("qualifies");
        let force_extra = rider()
            .extra_damage_for_with_spell_attack_extension(&force_spell, false, true)
            .expect("qualifies");
        // It is genuinely the same rider - only the declared type differs.
        assert_eq!(radiant_extra.dice_count, force_extra.dice_count);
        assert_eq!(radiant_extra.dice_sides, force_extra.dice_sides);
        assert_eq!(radiant_extra.kind, Some(DamageKind::Radiant));
        assert_eq!(force_extra.kind, Some(DamageKind::Force));

        // Isolate the rider's own contribution: a base attack pool of zero
        // dice, so the whole PMF is exactly what the rider deals.
        let carrier = |extra: DamageRider| {
            Attack::new(7, 0, 4, 0)
                .with_mode(RollMode::Advantage)
                .with_damage_rider(extra)
        };
        let radiant_pmf = damage_pmf(&carrier(radiant_extra), &defense);
        let force_pmf = damage_pmf(&carrier(force_extra), &defense);

        assert!(
            radiant_pmf.mean() > 0.0,
            "resistance still lets some through"
        );
        assert!(
            radiant_pmf.mean() < force_pmf.mean(),
            "the Radiant-triggered rider should deal less net damage than the \
             Force-triggered one, against a target resistant only to Radiant"
        );

        // And Force is genuinely untouched by a Radiant-only resistance -
        // exactly what an identical rider would deal with no resistance
        // in play at all, not merely "less reduced than Radiant".
        let no_resistance = Defense::new(1, 60);
        let force_pmf_unresisted = damage_pmf(&carrier(force_extra), &no_resistance);
        assert!(close(force_pmf.mean(), force_pmf_unresisted.mean()));
    }

    /// The sampled path must agree with the exact one for this mechanism
    /// end to end: a spell's declared damage type flowing from
    /// [`Attack::spell_damage_kind`] through the rider into
    /// [`Defense::reduction_for`], for both the resisted and the unresisted
    /// spell type from the scenario above.
    #[test]
    fn sampled_damage_agrees_with_the_exact_path_for_a_spell_inherited_damage_type() {
        let defense =
            Defense::new(14, 60).with_kind_reduction(DamageKind::Radiant, Reduction::Resistant);
        let cases = [
            (
                "resisted: the spell's declared type is Radiant",
                DamageKind::Radiant,
            ),
            (
                "unresisted: the spell's declared type is Force",
                DamageKind::Force,
            ),
        ];
        for (seed, (name, kind)) in cases.into_iter().enumerate() {
            let attack = Attack::new(6, 1, 8, 4)
                .with_mode(RollMode::Advantage)
                .with_is_spell_attack(true)
                .with_spell_damage_kind(kind);
            let extra = rider()
                .extra_damage_for_with_spell_attack_extension(&attack, false, true)
                .expect("qualifies");
            let attack = attack.with_damage_rider(extra);

            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 1_700);
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
    /// trigger matrix `features::classes::rogue::sneak_attack`'s tests cover for
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

    fn poisoner_buff() -> Rider {
        Rider::ConditionTriggeredWeaponDamage {
            trigger: Condition::Poisoned,
            dice_count: 2,
            dice_sides: 6,
            bonus: 0,
            damage_kind: DamageKind::Poison,
        }
    }

    #[test]
    fn arms_only_on_its_own_trigger_condition() {
        assert!(poisoner_buff().arms_on_condition(Condition::Poisoned));
        assert!(!poisoner_buff().arms_on_condition(Condition::Stunned));
        // An unrelated rider never answers yes either, the same "not
        // mistaken for a different marker" check every other rider accessor
        // in this module is held to.
        assert!(!Rider::AlwaysSucceed { uses: 3 }.arms_on_condition(Condition::Poisoned));
    }

    #[test]
    fn contributes_no_damage_rider_until_armed() {
        assert_eq!(poisoner_buff().weapon_damage_if_armed(false), None);
        assert_eq!(
            poisoner_buff().weapon_damage_if_armed(true),
            Some(DamageRider::new(2, 6))
        );
    }

    #[test]
    fn a_non_matching_rider_never_contributes_this_buff() {
        assert_eq!(sneak_attack().weapon_damage_if_armed(true), None);
    }

    /// The framework end to end: an attacker who has not yet poisoned
    /// anything hits for its base damage alone; the same attacker, once
    /// `arms_on_condition` says the buff is live, appends the extra dice to
    /// every subsequent weapon attack - checked exactly, not merely by field
    /// value, against the same `damage_pmf`/`sample_damage` machinery every
    /// other rider in this module is held to.
    #[test]
    fn armed_buff_raises_mean_damage_and_agrees_with_the_exact_path() {
        let buff = poisoner_buff();
        let defense = Defense::new(12, 60);
        let attack = Attack::new(6, 1, 8, 4);

        let unarmed = attack.clone();
        let armed = attack.with_damage_rider(
            buff.weapon_damage_if_armed(true)
                .expect("armed buff contributes a damage rider"),
        );

        let exact_unarmed = damage_pmf(&unarmed, &defense);
        let exact_armed = damage_pmf(&armed, &defense);
        assert!(
            exact_armed.mean() > exact_unarmed.mean(),
            "an armed buff must raise expected damage over an unarmed attacker"
        );
        // 1d8+4 base, doubled on a crit, plus the buff's 2d6 doubled too.
        assert_eq!(exact_armed.max(), 2 * 8 + 2 * 2 * 6 + 4);

        for (seed, (name, attack)) in [("unarmed", unarmed), ("armed", armed)]
            .into_iter()
            .enumerate()
        {
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 6100);
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
    }
}
