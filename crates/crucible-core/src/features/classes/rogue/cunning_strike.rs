//! Cunning Strike (2024 Rogue 5): the framework that spends Sneak Attack
//! dice, and the Poison, Trip and Withdraw options it can buy.

use crate::creature::Rider;
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{Ability, Condition, DamageRider, Duration, SpellCastingProfile};

/// Cunning Strike (2024 Rogue 5): forgo some of a qualifying Sneak Attack's
/// dice, 1d6 at a time, to fund a rider effect at this creature's own Cunning
/// Strike DC instead of rolling that share for damage.
///
/// This plugin is deliberately the whole framework and nothing else: it only
/// unlocks the DC and the ability to spend from Sneak Attack's pool (see
/// [`Rider::CunningStrike`] and [`crate::rules::DamageRider::spend`]),
/// which with a Poisoner's Kit includes Poison. Trip and Withdraw are their
/// own plugins, each reading this same DC. Which effect a given hit buys is
/// chosen in the fight itself - see `sim::fight`'s Cunning Strike choice.
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
/// [`crate::features::classes::rogue::SneakAttackPlugin`] and
/// [`CunningStrikePlugin`]: there is nothing to add to a creature's static
/// rider list here. The DC already lives on the
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

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Cunning Strike (2024 Rogue 5)
    registry.register("cunning_strike", |val| {
        let dex_modifier = val
            .get("dex_modifier")
            .and_then(|v| v.as_integer())
            .ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "cunning_strike needs a `dex_modifier` (the Rogue's Dexterity modifier)"
                        .to_string(),
                )
            })? as i32;
        let proficiency_bonus = val
            .get("proficiency_bonus")
            .and_then(|v| v.as_integer())
            .ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "cunning_strike needs a `proficiency_bonus`".to_string(),
                )
            })? as i32;
        // A magic item's flat bonus to the DC (ITM-06) - optional, and
        // zero (no change at all) when the config never mentions it.
        let item_bonus = val
            .get("item_bonus")
            .and_then(|v| v.as_integer())
            .unwrap_or(0) as i32;
        Ok(Box::new(
            CunningStrikePlugin::new(dex_modifier, proficiency_bonus).with_item_bonus(item_bonus),
        ))
    });

    // Cunning Strike: Trip (2024 Rogue 5)
    registry.register("cunning_strike_trip", |_val| {
        Ok(Box::new(CunningStrikeTripPlugin))
    });

    // Cunning Strike: Withdraw (2024 Rogue 5)
    registry.register("cunning_strike_withdraw", |_val| {
        Ok(Box::new(CunningStrikeWithdrawPlugin))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::classes::rogue::SneakAttackPlugin;
    use crate::prob::Rng;
    use crate::rules::{damage_pmf, sample_damage, Attack, Defense, RollMode};

    fn rider() -> Rider {
        Rider::ConditionalExtraDamage {
            dice_count: 4,
            dice_sides: 6,
            once_per_turn: true,
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
        use crate::rules::{Condition, Size};

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
    /// `crate::creature::rider` for both.
    #[test]
    fn cunning_strike_poison_can_land_on_an_immune_target_with_the_downgrade_trait() {
        use crate::creature::{saving_throw_against_condition, Creature};

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

    #[test]
    fn cunning_strike_reads_its_dc_inputs_from_toml() {
        let registry = FeatureRegistry::new();
        let params: toml::Value =
            toml::from_str("plugin = \"cunning_strike\"\ndex_modifier = 4\nproficiency_bonus = 3")
                .unwrap();
        let plugin = registry
            .build_plugin("cunning_strike", &params)
            .expect("cunning_strike builds from toml");
        assert_eq!(plugin.id(), "cunning_strike");
        assert_eq!(plugin.name(), "Cunning Strike");

        let mut builder = crate::features::CreatureBuilder::new("Rogue", 15, 40);
        plugin.apply(&mut builder).unwrap();
        assert_eq!(
            builder.creature.riders,
            vec![crate::creature::Rider::CunningStrike { dc: 15 }]
        );
    }

    /// ITM-06: an optional `item_bonus` in the TOML flows through to the DC,
    /// and is zero - unchanged from before this field existed - when the
    /// config never mentions it.
    #[test]
    fn cunning_strike_reads_an_optional_item_bonus_from_toml() {
        let registry = FeatureRegistry::new();

        let without: toml::Value =
            toml::from_str("plugin = \"cunning_strike\"\ndex_modifier = 4\nproficiency_bonus = 3")
                .unwrap();
        let plugin = registry.build_plugin("cunning_strike", &without).unwrap();
        let mut builder = crate::features::CreatureBuilder::new("Rogue", 15, 40);
        plugin.apply(&mut builder).unwrap();
        assert_eq!(
            builder.creature.riders,
            vec![crate::creature::Rider::CunningStrike { dc: 15 }]
        );

        let with: toml::Value = toml::from_str(
            "plugin = \"cunning_strike\"\ndex_modifier = 4\nproficiency_bonus = 3\nitem_bonus = 2",
        )
        .unwrap();
        let plugin = registry.build_plugin("cunning_strike", &with).unwrap();
        let mut builder = crate::features::CreatureBuilder::new("Rogue", 15, 40);
        plugin.apply(&mut builder).unwrap();
        assert_eq!(
            builder.creature.riders,
            vec![crate::creature::Rider::CunningStrike { dc: 17 }]
        );
    }

    #[test]
    fn cunning_strike_requires_both_dc_inputs() {
        let registry = FeatureRegistry::new();
        let missing_proficiency: toml::Value =
            toml::from_str("plugin = \"cunning_strike\"\ndex_modifier = 4").unwrap();
        assert!(matches!(
            registry.build_plugin("cunning_strike", &missing_proficiency),
            Err(FeatureError::InvalidConfiguration(_))
        ));

        let missing_dex: toml::Value =
            toml::from_str("plugin = \"cunning_strike\"\nproficiency_bonus = 3").unwrap();
        assert!(matches!(
            registry.build_plugin("cunning_strike", &missing_dex),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn cunning_strike_trip_and_withdraw_register_their_marker_riders() {
        let registry = FeatureRegistry::new();
        let no_params: toml::Value = toml::from_str("plugin = \"cunning_strike_trip\"").unwrap();

        let trip = registry
            .build_plugin("cunning_strike_trip", &no_params)
            .expect("cunning_strike_trip builds from toml");
        assert_eq!(trip.id(), "cunning_strike_trip");
        assert_eq!(trip.name(), "Cunning Strike: Trip");
        let mut builder = crate::features::CreatureBuilder::new("Rogue", 15, 40);
        trip.apply(&mut builder).unwrap();
        assert_eq!(
            builder.creature.riders,
            vec![crate::creature::Rider::CunningStrikeTrip]
        );

        let withdraw = registry
            .build_plugin("cunning_strike_withdraw", &no_params)
            .expect("cunning_strike_withdraw builds from toml");
        assert_eq!(withdraw.id(), "cunning_strike_withdraw");
        assert_eq!(withdraw.name(), "Cunning Strike: Withdraw");
        let mut builder = crate::features::CreatureBuilder::new("Rogue", 15, 40);
        withdraw.apply(&mut builder).unwrap();
        assert_eq!(
            builder.creature.riders,
            vec![crate::creature::Rider::CunningStrikeWithdraw]
        );
    }
}
