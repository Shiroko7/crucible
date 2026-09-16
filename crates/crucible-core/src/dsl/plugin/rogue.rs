//! Rogue class feature plugins.

use crate::rules::creature::Rider;

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
}
