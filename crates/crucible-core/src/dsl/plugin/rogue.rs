//! Rogue class feature plugins.

use crate::rules::combat::DamageRider;
use crate::rules::creature::{
    Ability, Condition, Duration, Effect, Move, MoveKind, Rider, SpellCastingProfile,
};

use super::traits::{CreatureBuilder, FeatureError, FeaturePlugin, FeatureResult};

/// Sneak Attack (2024 Rogue 1): once per turn, extra damage dice on a hit
/// with a finesse or ranged weapon, if the attack has advantage or an ally
/// is within 5 feet of the target - unless the attacker also has
/// disadvantage, which cancels it even with an ally in place.
///
/// The mechanism this registers - [`Rider::ConditionalExtraDamage`] - is a
/// [`crate::rules::combat::DamageRider`] gated on flags read straight off the
/// [`crate::rules::combat::Attack`] being resolved: [`Attack::mode`] for
/// advantage/disadvantage, and [`Attack::ally_adjacent`] standing in for the
/// "an ally is next to the target" clause the engine has no positioning model
/// to derive (see `DESIGN.md`). Whoever builds the attack for a given
/// scenario or turn sets that flag the same way `mode` already gets set;
/// `Rider::extra_damage_for` is where the gate is actually checked.
///
/// `dice_count` is a plugin parameter rather than the printed "4d6" baked in,
/// because a later character build layers character-specific totals on top
/// (a magic item's extra die, say) rather than replacing it, and because a
/// rogue's level determines it (4d6 at level 7, 5d6 later, and so on).
///
/// [`Attack::mode`]: crate::rules::combat::Attack
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SneakAttackPlugin {
    pub dice_count: u32,
    pub dice_sides: u32,
}

impl SneakAttackPlugin {
    /// A standard Sneak Attack: `dice_count` d6s, per the 2024 rules.
    pub fn new(dice_count: u32) -> Self {
        Self::with_sides(dice_count, 6)
    }

    /// As [`SneakAttackPlugin::new`], with an overridden die size - kept
    /// configurable rather than hardcoded to d6 for the same reason the dice
    /// count is a parameter and not a constant.
    pub fn with_sides(dice_count: u32, dice_sides: u32) -> Self {
        Self {
            dice_count,
            dice_sides,
        }
    }
}

impl FeaturePlugin for SneakAttackPlugin {
    fn id(&self) -> &'static str {
        "sneak_attack"
    }

    fn name(&self) -> &str {
        "Sneak Attack"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_rider(Rider::ConditionalExtraDamage {
            dice_count: self.dice_count,
            dice_sides: self.dice_sides,
            once_per_turn: true,
        });
        Ok(())
    }
}

/// Fast Hands (2024 Rogue Thief 3): use a Bonus Action to take the Use an
/// Object action, or to activate a magic item that would otherwise cost the
/// Magic action - drinking a potion, retrieving a hidden blade, waving a
/// wand - freeing the Action for something else that turn.
///
/// The engine has no generic "Use an Object" or "Magic" action of its own
/// (see [`MoveKind`]); a creature's actual item-activation move is just
/// another [`Move`], declared wherever the rest of its actions are. This
/// plugin's whole job is to take that move and register it as a bonus
/// action too: `crate::sim::duel` already picks one move from `actions` and,
/// independently, one from `bonus_actions` each turn, so having the same
/// move available in both lists *is* the feature - no special-casing in the
/// turn loop required. Whatever resource or `Uses` budget the move is under
/// still gates it exactly once, whichever slot spends it.
///
/// Takes ownership of the `Move` itself rather than a name to look up; the
/// registry's `fast_hands` entry builds one from the scenario DSL, where the
/// `object` and `item` clauses tag it `ObjectUse` or `MagicItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct FastHandsPlugin {
    pub item_move: Move,
}

impl FastHandsPlugin {
    pub fn new(item_move: Move) -> Self {
        Self { item_move }
    }
}

impl FeaturePlugin for FastHandsPlugin {
    fn id(&self) -> &'static str {
        "fast_hands"
    }

    fn name(&self) -> &str {
        "Fast Hands"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        match self.item_move.kind {
            MoveKind::ObjectUse | MoveKind::MagicItem => {
                builder.add_bonus_action(self.item_move.clone());
                Ok(())
            }
            MoveKind::Standard | MoveKind::Spell => {
                Err(FeatureError::InvalidConfiguration(format!(
                    "Fast Hands only promotes a Use an Object or magic item move to a bonus \
                 action; '{}' is tagged neither",
                    self.item_move.name
                )))
            }
        }
    }
}

/// Reliable Talent (2024 Rogue 7): treat any d20 roll of less than 10 as a
/// 10, for an ability check using a skill or tool the rogue is proficient
/// in - a floor under the roll, not a reroll, so it can only ever help.
///
/// This only sets [`crate::rules::creature::Creature::reliable_talent_floor`]
/// via [`CreatureBuilder::set_reliable_talent_floor`]; the "proficient" half
/// of the rule is up to whoever builds the [`crate::rules::check::CheckRoll`]
/// for a given check, via [`crate::rules::creature::Creature::check_floor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReliableTalentPlugin {
    pub floor: i32,
}

impl ReliableTalentPlugin {
    /// The standard feature: a floor of 10.
    pub fn new() -> Self {
        Self { floor: 10 }
    }

    /// As [`ReliableTalentPlugin::new`], with an overridden floor - kept
    /// configurable for the same reason Sneak Attack's dice count is, even
    /// though 5e only ever prints 10.
    pub fn with_floor(floor: i32) -> Self {
        Self { floor }
    }
}

impl Default for ReliableTalentPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl FeaturePlugin for ReliableTalentPlugin {
    fn id(&self) -> &'static str {
        "reliable_talent"
    }

    fn name(&self) -> &str {
        "Reliable Talent"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.set_reliable_talent_floor(self.floor);
        Ok(())
    }
}

/// Cunning Strike (2024 Rogue 5): forgo some of a qualifying Sneak Attack's
/// dice, 1d6 at a time, to fund a rider effect at this creature's own Cunning
/// Strike DC instead of rolling that share for damage.
///
/// This plugin is deliberately the whole framework and nothing else: it only
/// unlocks the DC and the ability to spend from Sneak Attack's pool (see
/// [`Rider::CunningStrike`] and [`crate::rules::combat::DamageRider::spend`]),
/// which with a Poisoner's Kit includes Poison. Trip and Withdraw are their
/// own plugins, each reading this same DC. Which effect a given hit buys is
/// chosen in the fight itself - see `sim::duel`'s Cunning Strike choice.
///
/// `dex_modifier` and `proficiency_bonus` are plugin parameters rather than a
/// baked-in `dc`, for the same reason [`crate::dsl::config::SpellcastingConfig`]
/// carries its own ability modifier and proficiency bonus instead of a single
/// precomputed number: a magic item or a level-up changes one of the inputs
/// without this plugin's shape changing.
///
/// `item_bonus` is that same idea applied to equipment specifically (ITM-06):
/// a flat bonus from a magic item that sharpens Cunning Strike's DC, kept as
/// its own field rather than folded into `dex_modifier` so a later item swap
/// changes one number without touching the character's actual Dexterity.
/// This needed no new engine mechanism at all - [`SpellCastingProfile`]
/// already carries an `item_bonus` of exactly this shape via
/// [`SpellCastingProfile::with_item_bonus`], and [`CunningStrikePlugin::dc`]
/// already builds one of those on the fly, so raising the DC is just another
/// constructor parameter passed through to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CunningStrikePlugin {
    pub dex_modifier: i32,
    pub proficiency_bonus: i32,
    pub item_bonus: i32,
}

impl CunningStrikePlugin {
    pub fn new(dex_modifier: i32, proficiency_bonus: i32) -> Self {
        Self {
            dex_modifier,
            proficiency_bonus,
            item_bonus: 0,
        }
    }

    /// As [`CunningStrikePlugin::new`], with a flat item bonus to the DC -
    /// see the field doc on [`CunningStrikePlugin::item_bonus`].
    pub fn with_item_bonus(mut self, item_bonus: i32) -> Self {
        self.item_bonus = item_bonus;
        self
    }

    /// The Cunning Strike DC: `8 + Dexterity modifier + proficiency bonus +
    /// item bonus`.
    ///
    /// That is exactly [`SpellCastingProfile::save_dc`]'s `8 + ability
    /// modifier + proficiency bonus + item bonus` shape, reused here rather
    /// than reimplemented - constructed on the fly and keyed to
    /// [`Ability::Dex`] specifically, never read off `creature.spellcasting`.
    /// Cunning Strike is not spellcasting: it uses this same formula even for
    /// a Rogue with no spellcasting profile at all (every base Rogue) and
    /// even for one whose actual spellcasting ability is something else
    /// entirely (an Arcane Trickster's Intelligence).
    pub fn dc(&self) -> i32 {
        SpellCastingProfile::new(Ability::Dex, self.dex_modifier, self.proficiency_bonus)
            .with_item_bonus(self.item_bonus)
            .save_dc()
    }
}

impl FeaturePlugin for CunningStrikePlugin {
    fn id(&self) -> &'static str {
        "cunning_strike"
    }

    fn name(&self) -> &str {
        "Cunning Strike"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_rider(Rider::CunningStrike { dc: self.dc() });
        Ok(())
    }
}

/// Steady Aim (2024 Rogue 3): Bonus Action. Grants advantage on your own next
/// attack roll before the end of the turn, and your speed becomes 0 until the
/// end of the turn.
///
/// Modelled as a bonus-action [`Move`] whose effect is
/// [`Effect::Stance { condition: Condition::SteadyAim }`](Effect::Stance) -
/// exactly the mechanism Dodge already uses for "a condition you apply to
/// yourself that lasts until the start of your own next turn." Whichever
/// attack is resolved while [`Condition::SteadyAim`] is active gets
/// [`RollMode::Advantage`](crate::rules::combat::RollMode::Advantage) from
/// it - see `sim::duel`'s `attack_mode`, which reads
/// [`Condition::advantage_on_attacks`] the same way it already read
/// [`Condition::disadvantage_on_attacks`] for Poisoned and Blinded. The speed
/// clause is [`Condition::zeroes_speed`]: nothing in this engine has a
/// position or a speed to zero yet (see `DESIGN.md`'s "Positioning is the gap
/// that matters"), so that flag is tracked and exposed generically rather
/// than acted on.
///
/// The move is marked [`Move::before_action`], so `sim::duel` resolves it
/// before the same turn's action - the attack it exists to set up - and the
/// attack roll uses the advantage up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SteadyAimPlugin;

impl SteadyAimPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FeaturePlugin for SteadyAimPlugin {
    fn id(&self) -> &'static str {
        "steady_aim"
    }

    fn name(&self) -> &str {
        "Steady Aim"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_bonus_action(
            Move::new(
                "Steady Aim",
                Effect::Stance {
                    condition: Condition::SteadyAim,
                },
            )
            .with_before_action(),
        );
        Ok(())
    }
}

/// Cunning Strike: Trip (2024 Rogue 5): forgo 1d6 of a qualifying Sneak
/// Attack to force a Dexterity save, against the Cunning Strike DC, on a
/// target that is Large size or smaller - knocking it Prone on a failure.
///
/// This plugin only unlocks the option existing at all, registering
/// [`Rider::CunningStrikeTrip`] - a pure marker, exactly like
/// [`CunningStrikePlugin`] itself unlocking [`Rider::CunningStrike`]. It
/// carries no dice or DC of its own: [`Rider::resolve_cunning_strike_trip`]
/// always reads [`Rider::CunningStrike`]'s DC, the same "framework unlocks
/// it, the marker rider carries the DC" split [`CunningStrikePlugin`]'s own
/// doc comment describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CunningStrikeTripPlugin;

impl FeaturePlugin for CunningStrikeTripPlugin {
    fn id(&self) -> &'static str {
        "cunning_strike_trip"
    }

    fn name(&self) -> &str {
        "Cunning Strike: Trip"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_rider(Rider::CunningStrikeTrip);
        Ok(())
    }
}

/// Cunning Action (2024 Rogue 2): Bonus Action. Take the Dash or Disengage
/// action as a bonus action instead of spending your action on it.
///
/// Registered as two separate bonus-action [`Move`]s rather than one, because
/// a [`crate::sim::duel::Plan`] only ever picks a single bonus action out of
/// the whole list regardless of how many are on it - offering both is exactly
/// "either one, never both" with no extra bookkeeping needed.
///
/// Neither move does anything mechanically here. Dash (double speed) and
/// Disengage (moving away provokes no opportunity attacks) are both about
/// movement and positioning, and this engine has neither (see `DESIGN.md`'s
/// "Positioning is the gap that matters") - so both are
/// `Effect::Sequence(Vec::new())`, a legal, zero-damage, zero-rider move a
/// plan can still select and spend the bonus-action slot on, rather than
/// invented movement mechanics standing in for rules that do not exist yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CunningActionPlugin;

impl CunningActionPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FeaturePlugin for CunningActionPlugin {
    fn id(&self) -> &'static str {
        "cunning_action"
    }

    fn name(&self) -> &str {
        "Cunning Action"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_bonus_action(Move::new(
            "Dash (Bonus Action)",
            Effect::Sequence(Vec::new()),
        ));
        builder.add_bonus_action(Move::new(
            "Disengage (Bonus Action)",
            Effect::Sequence(Vec::new()),
        ));
        Ok(())
    }
}

/// Cunning Strike: Withdraw (2024 Rogue 5): forgo 1d6 of a qualifying Sneak
/// Attack to move up to half speed without provoking opportunity attacks.
///
/// Registers [`Rider::CunningStrikeWithdraw`]. Resolving it - see
/// [`Rider::resolve_cunning_strike_withdraw`] - can do no more than flag
/// that the rogue withdrew safely: there is no movement or
/// opportunity-attack model here for it to actually change anything
/// against, the same gap ROG-05's Cunning Action (Dash and Disengage,
/// registered as zero-effect moves) already hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CunningStrikeWithdrawPlugin;

impl FeaturePlugin for CunningStrikeWithdrawPlugin {
    fn id(&self) -> &'static str {
        "cunning_strike_withdraw"
    }

    fn name(&self) -> &str {
        "Cunning Strike: Withdraw"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        builder.add_rider(Rider::CunningStrikeWithdraw);
        Ok(())
    }
}

/// Cunning Strike: Poison (2024 Rogue 5) - one of the effects a spend from a
/// qualifying Sneak Attack's pool can buy once [`CunningStrikePlugin`] has
/// unlocked it. Forgo 1d6: the target makes a Constitution saving throw
/// against the Cunning Strike DC or gains [`Condition::Poisoned`] for a
/// minute, repeating that same save at the end of each of its own turns
/// until it succeeds.
///
/// Deliberately a plain function rather than a `FeaturePlugin`, unlike
/// [`SneakAttackPlugin`] and [`CunningStrikePlugin`]: there is nothing to add
/// to a creature's static rider list here. The DC already lives on the
/// [`Rider::CunningStrike`] marker the creature carries -
/// [`Rider::cunning_strike_dc`] - and which option a spend buys is a
/// per-attack choice made by whoever resolves it, the same reason
/// [`Rider::extra_damage_for`] is itself a method rather than something baked
/// into the creature ahead of time.
///
/// `sneak_attack` is the qualifying Sneak Attack's [`DamageRider`], full or
/// already reduced by other options spent from the same pool. `dc` is
/// [`Rider::cunning_strike_dc`]'s value, never a creature's spellcasting DC
/// (Cunning Strike is not spellcasting - see that method's own doc comment).
/// Returns `None` if the pool cannot afford the 1d6 price,
/// [`DamageRider::spend`]'s own refusal rather than a silent clamp.
///
/// The returned [`Rider::SaveOrCondition`] reuses that mechanism - already
/// exactly "on a hit, the target saves or takes a condition" - instead of
/// inventing a new one: its `cost` is `None` because the price already came
/// out of the Sneak Attack pool above, not a separate resource, and
/// `once_per_turn` is `false` because Cunning Strike spends Sneak Attack's
/// own once-per-turn budget, already enforced wherever
/// [`Rider::extra_damage_for`] is checked.
pub fn cunning_strike_poison(sneak_attack: DamageRider, dc: i32) -> Option<(DamageRider, Rider)> {
    let reduced = sneak_attack.spend(1)?;
    let effect = Rider::SaveOrCondition {
        ability: Ability::Con,
        dc,
        condition: Condition::Poisoned,
        duration: Duration::SaveEndTurn {
            ability: Ability::Con,
            dc,
        },
        cost: None,
        once_per_turn: false,
    };
    Some((reduced, effect))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::plugin::traits::CreatureBuilder;
    use crate::prob::rng::Rng;
    use crate::rules::combat::{damage_pmf, sample_damage, Attack, DamageRider, Defense, RollMode};

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

    #[test]
    fn applying_the_plugin_registers_a_conditional_extra_damage_rider() {
        let builder = CreatureBuilder::new("Rogue", 15, 40);
        let built = builder
            .apply_feature(&SneakAttackPlugin::new(4))
            .expect("sneak attack applies")
            .build()
            .expect("builds");
        assert_eq!(
            built.riders,
            vec![Rider::ConditionalExtraDamage {
                dice_count: 4,
                dice_sides: 6,
                once_per_turn: true,
            }]
        );
    }

    #[test]
    fn triggers_on_advantage_with_a_qualifying_weapon() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        let got = rider().extra_damage_for(&attack, false);
        assert_eq!(got, Some(DamageRider::new(4, 6)));
    }

    #[test]
    fn triggers_on_ally_adjacent_without_advantage() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Normal)
            .with_finesse_or_ranged(true)
            .with_ally_adjacent(true);
        let got = rider().extra_damage_for(&attack, false);
        assert_eq!(got, Some(DamageRider::new(4, 6)));
    }

    #[test]
    fn does_not_trigger_with_disadvantage_even_with_ally_adjacent() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Disadvantage)
            .with_finesse_or_ranged(true)
            .with_ally_adjacent(true);
        assert_eq!(rider().extra_damage_for(&attack, false), None);
    }

    #[test]
    fn does_not_trigger_with_disadvantage_even_with_advantage_also_present() {
        // Advantage and disadvantage from unrelated sources have already
        // cancelled to Normal by the time `mode` is set on the attack (see
        // `resolve_mode`), so this is really the same case as the one above,
        // stated for the situation combat.rs actually produces: a roll that
        // was going to have both never reaches here as `Advantage`.
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Disadvantage)
            .with_finesse_or_ranged(true);
        assert_eq!(rider().extra_damage_for(&attack, false), None);
    }

    #[test]
    fn does_not_trigger_without_advantage_or_ally_adjacent() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Normal)
            .with_finesse_or_ranged(true);
        assert_eq!(rider().extra_damage_for(&attack, false), None);
    }

    #[test]
    fn does_not_trigger_without_a_qualifying_weapon() {
        let attack = Attack::new(7, 1, 4, 3).with_mode(RollMode::Advantage);
        assert_eq!(rider().extra_damage_for(&attack, false), None);
    }

    #[test]
    fn once_per_turn_budget_is_enforced() {
        let attack = Attack::new(7, 1, 4, 3)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        assert!(rider().extra_damage_for(&attack, false).is_some());
        assert_eq!(
            rider().extra_damage_for(&attack, true),
            None,
            "already used this turn"
        );
    }

    #[test]
    fn a_qualifying_hit_doubles_its_dice_on_a_crit_like_any_other_rider() {
        let defense = Defense::new(1, 40); // AC 1: every non-fumble roll hits
        let attack = Attack::new(5, 1, 6, 0)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        let extra = rider().extra_damage_for(&attack, false).expect("qualifies");
        let with_sneak = attack.with_damage_rider(extra);
        let pmf = damage_pmf(&with_sneak, &defense);
        assert!(close(pmf.total(), 1.0));
        // Base 1d6 (max 6) + sneak 4d6 (max 24); a crit doubles both pools -
        // ARCH-03's `rider_pmf`, not reimplemented here.
        assert_eq!(pmf.max(), 2 * 6 + 2 * 4 * 6);
    }

    #[test]
    fn sampled_sneak_attack_agrees_with_the_exact_path_across_trigger_conditions() {
        let defense = Defense::new(14, 60);
        let cases = [
            (
                "advantage, finesse weapon: triggers",
                Attack::new(6, 1, 8, 4)
                    .with_mode(RollMode::Advantage)
                    .with_finesse_or_ranged(true),
            ),
            (
                "ally adjacent, no advantage: triggers",
                Attack::new(6, 1, 8, 4)
                    .with_mode(RollMode::Normal)
                    .with_finesse_or_ranged(true)
                    .with_ally_adjacent(true),
            ),
            (
                "disadvantage with ally adjacent: does not trigger",
                Attack::new(6, 1, 8, 4)
                    .with_mode(RollMode::Disadvantage)
                    .with_finesse_or_ranged(true)
                    .with_ally_adjacent(true),
            ),
            (
                "advantage without a qualifying weapon: does not trigger",
                Attack::new(6, 1, 8, 4).with_mode(RollMode::Advantage),
            ),
        ];
        for (seed, (name, attack)) in cases.into_iter().enumerate() {
            let attack = match rider().extra_damage_for(&attack, false) {
                Some(extra) => attack.with_damage_rider(extra),
                None => attack,
            };
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 900);
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

    fn item_move(kind: MoveKind) -> Move {
        use crate::rules::creature::Effect;
        Move::new("Potion of Healing", Effect::Sequence(Vec::new())).with_kind(kind)
    }

    #[test]
    fn fast_hands_registers_an_object_use_move_as_a_bonus_action() {
        let builder = CreatureBuilder::new("Thief", 15, 40);
        let built = builder
            .apply_feature(&FastHandsPlugin::new(item_move(MoveKind::ObjectUse)))
            .expect("fast hands applies to an object-use move")
            .build()
            .expect("builds");
        assert_eq!(built.bonus_actions.len(), 1);
        assert_eq!(built.bonus_actions[0].name, "Potion of Healing");
        // The Action slot is untouched - it stays free for something else.
        assert!(built.actions.is_empty());
    }

    #[test]
    fn fast_hands_registers_a_magic_item_move_as_a_bonus_action() {
        let builder = CreatureBuilder::new("Thief", 15, 40);
        let built = builder
            .apply_feature(&FastHandsPlugin::new(item_move(MoveKind::MagicItem)))
            .expect("fast hands applies to a magic item activation")
            .build()
            .expect("builds");
        assert_eq!(built.bonus_actions.len(), 1);
        assert_eq!(built.bonus_actions[0].name, "Potion of Healing");
    }

    #[test]
    fn fast_hands_rejects_a_plain_standard_move() {
        let builder = CreatureBuilder::new("Thief", 15, 40);
        let err = builder
            .apply_feature(&FastHandsPlugin::new(item_move(MoveKind::Standard)))
            .expect_err("a move not tagged ObjectUse or MagicItem must not be silently promoted");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    /// Casting a spell is not "Use an Object or a magic item" either - Fast
    /// Hands still has nothing to say about it.
    #[test]
    fn fast_hands_rejects_a_spell_move() {
        let builder = CreatureBuilder::new("Thief", 15, 40);
        let err = builder
            .apply_feature(&FastHandsPlugin::new(item_move(MoveKind::Spell)))
            .expect_err("a spell-tagged move must not be silently promoted either");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    #[test]
    fn fast_hands_leaves_an_already_declared_action_copy_alone() {
        // A creature can have the same move declared as its Action (the
        // baseline "Use an Object" everyone can already take) and, once Fast
        // Hands applies, also as a Bonus Action - both slots usable the same
        // turn, gated by whatever `Uses`/`Cost` budget the move itself
        // carries.
        let mut builder = CreatureBuilder::new("Thief", 15, 40);
        builder.add_action(item_move(MoveKind::ObjectUse));
        let built = builder
            .apply_feature(&FastHandsPlugin::new(item_move(MoveKind::ObjectUse)))
            .expect("fast hands applies")
            .build()
            .expect("builds");
        assert_eq!(built.actions.len(), 1);
        assert_eq!(built.bonus_actions.len(), 1);
    }

    #[test]
    fn reliable_talent_defaults_to_a_floor_of_ten() {
        let built = CreatureBuilder::new("Rogue", 15, 40)
            .apply_feature(&ReliableTalentPlugin::new())
            .expect("reliable talent applies")
            .build()
            .expect("builds");
        assert_eq!(built.check_floor(true), Some(10));
        // Never applies to a check the creature isn't proficient in.
        assert_eq!(built.check_floor(false), None);
    }

    #[test]
    fn a_creature_without_reliable_talent_has_no_floor_even_when_proficient() {
        let built = CreatureBuilder::new("Fighter", 15, 40)
            .build()
            .expect("builds");
        assert_eq!(built.check_floor(true), None);
    }

    #[test]
    fn reliable_talent_floor_can_be_overridden() {
        let built = CreatureBuilder::new("Homebrew Rogue", 15, 40)
            .apply_feature(&ReliableTalentPlugin::with_floor(12))
            .expect("reliable talent applies")
            .build()
            .expect("builds");
        assert_eq!(built.check_floor(true), Some(12));
    }

    /// End-to-end with `CheckRoll`: a proficient check against a dc a floor
    /// of 10 always meets, once Reliable Talent is applied.
    #[test]
    fn reliable_talent_floor_feeds_check_roll_and_guarantees_success() {
        use crate::rules::check::CheckRoll;

        let built = CreatureBuilder::new("Rogue", 15, 40)
            .apply_feature(&ReliableTalentPlugin::new())
            .expect("reliable talent applies")
            .build()
            .expect("builds");

        let proficient_check = CheckRoll::new(0, 10);
        let proficient_check = match built.check_floor(true) {
            Some(floor) => proficient_check.with_floor(floor),
            None => proficient_check,
        };
        assert!((proficient_check.success_chance() - 1.0).abs() < 1e-12);

        // The same DC, without the floor because this check is not one the
        // creature is proficient in, is not a guaranteed success.
        let unproficient_check = CheckRoll::new(0, 10);
        let unproficient_check = match built.check_floor(false) {
            Some(floor) => unproficient_check.with_floor(floor),
            None => unproficient_check,
        };
        assert!(unproficient_check.success_chance() < 1.0);
    }

    /// `8 + Dex modifier + proficiency bonus`, the printed 2024 Cunning
    /// Strike DC - and not the spellcasting formula's `item_bonus`, which
    /// Cunning Strike has no equivalent of and this plugin never exposes.
    #[test]
    fn cunning_strike_dc_follows_the_5e_formula() {
        assert_eq!(CunningStrikePlugin::new(3, 3).dc(), 14);
        assert_eq!(CunningStrikePlugin::new(4, 3).dc(), 15);
        // Generic over the numbers, the same way SpellCastingProfile is
        // generic over the ability: a higher proficiency bonus at a later
        // tier raises the DC by exactly that much.
        assert_eq!(CunningStrikePlugin::new(4, 6).dc(), 18);
    }

    /// ITM-06: a magic item's flat bonus raises the Cunning Strike DC by
    /// exactly its own value, composed on top of the printed formula rather
    /// than replacing any part of it - and a plugin built with no item bonus
    /// at all is unaffected, so the new field cannot silently change existing
    /// behaviour.
    #[test]
    fn an_item_bonus_raises_the_cunning_strike_dc_by_exactly_its_own_value() {
        let base = CunningStrikePlugin::new(4, 3);
        assert_eq!(base.dc(), 15, "no item bonus yet");
        assert_eq!(base.item_bonus, 0);

        let plus_one = base.with_item_bonus(1);
        assert_eq!(plus_one.dc(), 16);
        let plus_two = base.with_item_bonus(2);
        assert_eq!(plus_two.dc(), 17);

        // Composes with the rest of the formula rather than overriding it:
        // a higher proficiency bonus and an item bonus both raise the DC,
        // additively.
        assert_eq!(CunningStrikePlugin::new(4, 6).with_item_bonus(2).dc(), 20);
    }

    #[test]
    fn applying_the_plugin_registers_a_cunning_strike_marker_rider() {
        let builder = CreatureBuilder::new("Rogue", 15, 40);
        let built = builder
            .apply_feature(&CunningStrikePlugin::new(4, 3))
            .expect("cunning strike applies")
            .build()
            .expect("builds");
        assert_eq!(built.riders, vec![Rider::CunningStrike { dc: 15 }]);
    }

    /// The framework acceptance test: a level-5 Rogue built from both
    /// plugins can inspect its full Sneak Attack pool, reduce it by some
    /// amount before damage is rolled, and combine two 1d6 spends against a
    /// stand-in "costs 1d6, does nothing" Cunning Strike option - proving
    /// dice deduction, DC computation and combination all work together
    /// exactly as a later Poison or Trip/Withdraw plugin would use them.
    #[test]
    fn a_level_five_rogue_can_fund_two_stand_in_cunning_strike_options_from_one_sneak_attack() {
        let builder = CreatureBuilder::new("Rogue", 15, 40);
        let creature = builder
            .apply_feature(&SneakAttackPlugin::new(4))
            .expect("sneak attack applies")
            .apply_feature(&CunningStrikePlugin::new(4, 3))
            .expect("cunning strike applies")
            .build()
            .expect("builds");

        let sneak_attack_rider = creature
            .riders
            .iter()
            .find(|r| matches!(r, Rider::ConditionalExtraDamage { .. }))
            .expect("the sneak attack rider is present");
        let dc = creature
            .riders
            .iter()
            .find_map(Rider::cunning_strike_dc)
            .expect("cunning strike is unlocked");
        assert_eq!(dc, 15);

        let attack = Attack::new(7, 1, 6, 4)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        let full = sneak_attack_rider
            .extra_damage_for(&attack, false)
            .expect("qualifies");
        assert_eq!(full, DamageRider::new(4, 6));

        // Two 1d6 test-only "does nothing" Cunning Strike options, funded
        // from the same pool Sneak Attack would otherwise roll whole.
        const TEST_OPTION_COST: u32 = 1;
        let after_both_options = full
            .spend(TEST_OPTION_COST)
            .and_then(|r| r.spend(TEST_OPTION_COST))
            .expect("4 dice affords two 1d6 options");
        assert_eq!(after_both_options.dice_count, 2);

        // Only 2 dice actually get rolled for damage now - checked against
        // the exact distribution, not merely against the field value, so a
        // regression that rolls the full pool anyway cannot slip through.
        let defense = Defense::new(1, 60); // AC 1: every non-fumble roll hits
        let reduced = damage_pmf(
            &attack.clone().with_damage_rider(after_both_options),
            &defense,
        );
        let unspent = damage_pmf(&attack.with_damage_rider(full), &defense);
        assert_eq!(reduced.max(), 2 * 6 + 2 * 2 * 6 + 4);
        assert_eq!(unspent.max(), 2 * 6 + 2 * 4 * 6 + 4);
        assert!(reduced.mean() < unspent.mean());

        // Spending more than remains is refused rather than silently capped,
        // so a later effect plugin cannot overdraw the pool by accident.
        assert_eq!(after_both_options.spend(3), None);
    }

    #[test]
    fn applying_steady_aim_registers_a_bonus_action_that_applies_its_condition() {
        let builder = CreatureBuilder::new("Rogue", 15, 40);
        let built = builder
            .apply_feature(&SteadyAimPlugin::new())
            .expect("steady aim applies")
            .build()
            .expect("builds");
        assert_eq!(built.bonus_actions.len(), 1);
        let steady_aim = &built.bonus_actions[0];
        assert_eq!(steady_aim.name, "Steady Aim");
        assert_eq!(
            steady_aim.effect,
            Effect::Stance {
                condition: Condition::SteadyAim,
            }
        );
        // Free to take: it costs the bonus action slot, not a resource.
        assert!(steady_aim.is_free());
        assert!(
            steady_aim.before_action,
            "Steady Aim is taken before the attack it sets up"
        );
        // Zero damage in its own right - the advantage it grants only shows
        // up on whatever attack rolls against it, which `sim::duel`'s
        // `attack_mode` and `Condition::advantage_on_attacks` cover.
        let dummy = crate::rules::creature::Creature::new("dummy", 10, 10);
        assert_eq!(steady_aim.effect.mean_damage(&dummy), 0.0);
        assert_eq!(steady_aim.effect.stance(), Some(Condition::SteadyAim));
    }

    #[test]
    fn applying_cunning_action_registers_dash_and_disengage_as_free_bonus_actions() {
        let builder = CreatureBuilder::new("Rogue", 15, 40);
        let built = builder
            .apply_feature(&CunningActionPlugin::new())
            .expect("cunning action applies")
            .build()
            .expect("builds");
        assert_eq!(built.bonus_actions.len(), 2);
        let names: Vec<&str> = built
            .bonus_actions
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["Dash (Bonus Action)", "Disengage (Bonus Action)"]
        );

        let dummy = crate::rules::creature::Creature::new("dummy", 10, 10);
        for m in &built.bonus_actions {
            // Both are free to take (no resource cost, unlimited uses) and
            // have no mechanical effect - there is no movement model for
            // either to act on yet, but a plan can still legally select them.
            assert!(m.is_free());
            assert_eq!(m.effect, Effect::Sequence(Vec::new()));
            assert_eq!(m.effect.mean_damage(&dummy), 0.0);
            assert_eq!(m.effect.stance(), None);
        }
    }

    #[test]
    fn applying_the_trip_plugin_registers_its_marker_rider() {
        let builder = CreatureBuilder::new("Rogue", 15, 40);
        let built = builder
            .apply_feature(&CunningStrikeTripPlugin)
            .expect("cunning strike trip applies")
            .build()
            .expect("builds");
        assert_eq!(built.riders, vec![Rider::CunningStrikeTrip]);
    }

    #[test]
    fn applying_the_withdraw_plugin_registers_its_marker_rider() {
        let builder = CreatureBuilder::new("Rogue", 15, 40);
        let built = builder
            .apply_feature(&CunningStrikeWithdrawPlugin)
            .expect("cunning strike withdraw applies")
            .build()
            .expect("builds");
        assert_eq!(built.riders, vec![Rider::CunningStrikeWithdraw]);
    }

    /// End to end: a level-5 Rogue built from Sneak Attack, Cunning Strike,
    /// and both new option plugins can fund a real Trip attempt (against a
    /// legal, Large-or-smaller target) and a real Withdraw from the exact
    /// pool a qualifying Sneak Attack would otherwise roll whole - the same
    /// framework the stand-in test above exercises, now with the actual
    /// effects instead of "does nothing" placeholders.
    #[test]
    fn a_level_five_rogue_can_trip_and_withdraw_from_one_sneak_attack() {
        use crate::rules::creature::{Condition, Size};

        let builder = CreatureBuilder::new("Rogue", 15, 40);
        let creature = builder
            .apply_feature(&SneakAttackPlugin::new(4))
            .expect("sneak attack applies")
            .apply_feature(&CunningStrikePlugin::new(4, 3))
            .expect("cunning strike applies")
            .apply_feature(&CunningStrikeTripPlugin)
            .expect("trip applies")
            .apply_feature(&CunningStrikeWithdrawPlugin)
            .expect("withdraw applies")
            .build()
            .expect("builds");

        let dc = creature
            .riders
            .iter()
            .find_map(Rider::cunning_strike_dc)
            .expect("cunning strike is unlocked");
        assert_eq!(dc, 15);

        let attack = Attack::new(7, 1, 6, 4)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        let sneak_attack_rider = creature
            .riders
            .iter()
            .find(|r| matches!(r, Rider::ConditionalExtraDamage { .. }))
            .expect("the sneak attack rider is present");
        let full = sneak_attack_rider
            .extra_damage_for(&attack, false)
            .expect("qualifies");
        assert_eq!(full, DamageRider::new(4, 6));

        // Trip a Large ogre-sized target: a save bonus far below any d20
        // roll always fails, so it goes down Prone.
        let trip_rider = creature
            .riders
            .iter()
            .find(|r| matches!(r, Rider::CunningStrikeTrip))
            .expect("trip is unlocked");
        let mut rng = Rng::new(42);
        let (after_trip, prone) = trip_rider
            .resolve_cunning_strike_trip(full, Size::Large, -100, dc, &mut rng)
            .expect("a Large target is a legal Trip target");
        assert_eq!(after_trip.dice_count, 3, "1d6 spent on the Trip attempt");
        assert_eq!(prone, Some(Condition::Prone));

        // Fund a Withdraw from what is left of the same pool.
        let withdraw_rider = creature
            .riders
            .iter()
            .find(|r| matches!(r, Rider::CunningStrikeWithdraw))
            .expect("withdraw is unlocked");
        let (after_both, repositioned) = withdraw_rider
            .resolve_cunning_strike_withdraw(after_trip)
            .expect("2 dice can afford the 1d6 Withdraw cost");
        assert_eq!(after_both.dice_count, 2, "two combined 1d6 spends");
        assert!(repositioned);

        // What actually gets rolled for damage is the twice-reduced pool -
        // checked against the exact distribution, the same way ROG-02's own
        // framework test holds itself to.
        let defense = Defense::new(1, 60); // AC 1: every non-fumble roll hits
        let reduced = damage_pmf(&attack.clone().with_damage_rider(after_both), &defense);
        let unspent = damage_pmf(&attack.with_damage_rider(full), &defense);
        assert!(reduced.mean() < unspent.mean());

        // A Gargantuan target cannot be Tripped at all: the attempt is
        // refused and the die is never spent.
        assert_eq!(
            trip_rider.resolve_cunning_strike_trip(full, Size::Gargantuan, -100, dc, &mut rng),
            None
        );
    }

    /// The first real Cunning Strike option (ROG-03): spending it takes 1d6
    /// out of the pool and hands back the exact `SaveOrCondition` effect a
    /// failed Constitution save should produce - reusing that mechanism
    /// rather than a new one, and reading the Cunning Strike DC rather than
    /// inventing an ability-to-DC pipeline of its own.
    #[test]
    fn cunning_strike_poison_spends_1d6_and_builds_the_con_save_effect() {
        let full = DamageRider::new(4, 6);
        let dc = CunningStrikePlugin::new(4, 3).dc();
        assert_eq!(dc, 15);

        let (reduced, effect) = cunning_strike_poison(full, dc).expect("4 dice afford 1d6");
        assert_eq!(reduced.dice_count, 3, "1d6 came out of the pool");
        assert_eq!(
            effect,
            Rider::SaveOrCondition {
                ability: Ability::Con,
                dc,
                condition: Condition::Poisoned,
                duration: Duration::SaveEndTurn {
                    ability: Ability::Con,
                    dc,
                },
                cost: None,
                once_per_turn: false,
            },
            "cost is None: the price already came out of the Sneak Attack pool above, \
             not a separate resource"
        );
    }

    /// Spending more dice than remain is refused, not silently clamped - the
    /// same refusal `DamageRider::spend` itself gives, and here it is
    /// checked at the point Cunning Strike's own options draw from the pool.
    #[test]
    fn cunning_strike_poison_refuses_to_overdraw_an_empty_pool() {
        let empty = DamageRider::new(0, 6);
        assert_eq!(cunning_strike_poison(empty, 15), None);
    }

    /// The dice spent on Poison must actually not be thrown for damage, not
    /// merely be counted out afterwards - the same exact-vs-sampled
    /// agreement check every other rider in this codebase is held to.
    #[test]
    fn cunning_strike_poison_reduced_pool_agrees_with_the_exact_path() {
        let defense = Defense::new(12, 60);
        let attack = Attack::new(6, 1, 8, 4)
            .with_mode(RollMode::Advantage)
            .with_finesse_or_ranged(true);
        let full = rider().extra_damage_for(&attack, false).expect("qualifies");
        let dc = CunningStrikePlugin::new(4, 3).dc();

        let (reduced, effect) = cunning_strike_poison(full, dc).expect("4 dice afford 1d6");
        assert_eq!(reduced.dice_count, 3);
        assert_eq!(
            effect.cunning_strike_dc(),
            None,
            "a SaveOrCondition effect, not a new marker"
        );

        let full_attack = attack.clone().with_damage_rider(full);
        let reduced_attack = attack.with_damage_rider(reduced);
        let exact_full = damage_pmf(&full_attack, &defense);
        let exact_reduced = damage_pmf(&reduced_attack, &defense);
        assert!(
            exact_reduced.mean() < exact_full.mean(),
            "spending a die on Poison must lower expected damage, not just relabel it"
        );

        let (lo, hi) = (exact_reduced.min(), exact_reduced.max());
        let mut rng = Rng::new(2100);
        let n = 100_000;
        let mut counts = vec![0usize; (hi - lo + 1) as usize];
        for _ in 0..n {
            let d = sample_damage(&mut rng, &reduced_attack, &defense);
            assert!(d >= lo && d <= hi, "sampled {d} outside {lo}..={hi}");
            counts[(d - lo) as usize] += 1;
        }
        for (i, &c) in counts.iter().enumerate() {
            let value = lo + i as i32;
            let want = exact_reduced.prob(value);
            let got = c as f64 / n as f64;
            let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
            assert!(
                (got - want).abs() < tol,
                "P(damage = {value}) sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
            );
        }
    }

    /// ITM-01 composed with ROG-03: a Rogue who also carries the generic
    /// immunity-downgrade trait can actually poison a poison-immune target
    /// through Cunning Strike Poison, where without that trait the same
    /// option would leave it entirely unaffected.
    ///
    /// This is the acceptance test for the two features working together,
    /// not just side by side: `cunning_strike_poison` builds exactly the
    /// `Rider::SaveOrCondition` a failed Constitution save turns into
    /// Poisoned, and `saving_throw_against_condition` is what actually
    /// resolves that save against a target's condition immunity - see
    /// `crate::rules::creature::rider` for both.
    #[test]
    fn cunning_strike_poison_can_land_on_an_immune_target_with_the_downgrade_trait() {
        use crate::rules::creature::{saving_throw_against_condition, Creature};

        let mut immune_target = Creature::new("Zombie", 8, 22);
        immune_target.condition_immunities.push(Condition::Poisoned);

        let plain_rogue = Creature::new("Rogue", 15, 40);
        let corrosive_rogue = Creature::new("Rogue", 15, 40).with_rider(Rider::DowngradeImmunity {
            damage: None,
            condition: Some(Condition::Poisoned),
        });

        let dc = CunningStrikePlugin::new(4, 3).dc();
        let full = DamageRider::new(4, 6);
        let (_, effect) = cunning_strike_poison(full, dc).expect("4 dice afford 1d6");
        let Rider::SaveOrCondition {
            ability,
            dc,
            condition,
            ..
        } = effect
        else {
            panic!("cunning_strike_poison must build a SaveOrCondition effect");
        };
        assert_eq!(condition, Condition::Poisoned);

        // Without the downgrade trait: the zombie's immunity is untouched,
        // so it is unaffected outright, however the dice would have landed.
        let mut rng = Rng::new(5300);
        for _ in 0..1000 {
            assert!(
                saving_throw_against_condition(
                    &mut rng,
                    &immune_target,
                    &plain_rogue,
                    ability,
                    dc,
                    condition
                ),
                "a Rogue without the downgrade trait can never poison an immune target"
            );
        }

        // With the downgrade trait: the free pass is gone, and across enough
        // attempts the save is actually failed at least once - the target
        // can genuinely be poisoned by Cunning Strike Poison now.
        let mut rng = Rng::new(5301);
        let failed_at_least_once = (0..1000).any(|_| {
            !saving_throw_against_condition(
                &mut rng,
                &immune_target,
                &corrosive_rogue,
                ability,
                dc,
                condition,
            )
        });
        assert!(
            failed_at_least_once,
            "a Rogue with the downgrade trait must be able to land Poisoned on an immune target"
        );
    }
}
