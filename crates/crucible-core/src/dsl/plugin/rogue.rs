//! Rogue class feature plugins.

use crate::rules::creature::{Ability, Rider, SpellCastingProfile};

use super::traits::{CreatureBuilder, FeaturePlugin, FeatureResult};

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

/// Cunning Strike (2024 Rogue 5): forgo some of a qualifying Sneak Attack's
/// dice, 1d6 at a time, to fund a rider effect at this creature's own Cunning
/// Strike DC instead of rolling that share for damage.
///
/// This plugin is deliberately the whole framework and nothing else: it only
/// unlocks the DC and the ability to spend from Sneak Attack's pool (see
/// [`Rider::CunningStrike`] and [`crate::rules::combat::DamageRider::spend`]).
/// Which effects a spend actually buys - poison, a shove, breaking a grapple -
/// is future work, one plugin per effect, each reading this same DC.
///
/// `dex_modifier` and `proficiency_bonus` are plugin parameters rather than a
/// baked-in `dc`, for the same reason [`crate::dsl::config::SpellcastingConfig`]
/// carries its own ability modifier and proficiency bonus instead of a single
/// precomputed number: a magic item or a level-up changes one of the inputs
/// without this plugin's shape changing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CunningStrikePlugin {
    pub dex_modifier: i32,
    pub proficiency_bonus: i32,
}

impl CunningStrikePlugin {
    pub fn new(dex_modifier: i32, proficiency_bonus: i32) -> Self {
        Self {
            dex_modifier,
            proficiency_bonus,
        }
    }

    /// The Cunning Strike DC: `8 + Dexterity modifier + proficiency bonus`.
    ///
    /// That is exactly [`SpellCastingProfile::save_dc`]'s `8 + ability
    /// modifier + proficiency bonus` shape, reused here rather than
    /// reimplemented - constructed on the fly and keyed to
    /// [`Ability::Dex`] specifically, never read off `creature.spellcasting`.
    /// Cunning Strike is not spellcasting: it uses this same formula even for
    /// a Rogue with no spellcasting profile at all (every base Rogue) and
    /// even for one whose actual spellcasting ability is something else
    /// entirely (an Arcane Trickster's Intelligence).
    pub fn dc(&self) -> i32 {
        SpellCastingProfile::new(Ability::Dex, self.dex_modifier, self.proficiency_bonus).save_dc()
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
}
