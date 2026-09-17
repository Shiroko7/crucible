//! Healing and triage spells: Healing Word and Cure Wounds (SRD 5.2, 2024
//! rules). Also Guiding Bolt: see [`GuidingBoltPlugin`] below.
//!
//! Both healing spells restore hit points using `1..2 dice + the caster's
//! spellcasting ability modifier`, read from the creature's own
//! [`crate::rules::creature::SpellCastingProfile`] rather than a hardcoded
//! number, and both spend one 1st-level spell slot from the caster's
//! [`crate::rules::creature::SpellSlots`].
//!
//! Reviving a creature at 0 HP is not spell-specific text - it is 5e's
//! general "a creature that regains any hit points while it has 0 becomes
//! conscious" rule - so it is implemented once, in
//! [`crate::rules::creature::apply_healing`], and inherited by both spells
//! (and anything else that ever heals) rather than re-implemented per spell.
//!
//! Not modelled, on purpose:
//! - **Range.** `DESIGN.md` already rules positioning out of scope entirely
//!   ("Positioning is the gap that matters") - there is no notion of distance
//!   for a melee weapon either, so Healing Word's 60 feet and Cure Wounds'
//!   touch are flavour text here, not a mechanic.
//! - **Ally targeting.** The duel engine (`sim::duel`) only ever targets the
//!   opposing side right now - no move of any kind can target a friendly
//!   creature yet. These plugins produce fully-formed, fully-testable
//!   `Move`s (the right action economy, slot cost, and heal formula), but
//!   wiring "cast this on a bloodied ally" into the automated turn engine is
//!   a separate, considerably larger feature (self/ally targeting for every
//!   effect, plus a policy that decides when to heal) and is left for a
//!   follow-up rather than bolted on here.
//! - **Upcasting.** Both healing spells are implemented as their base
//!   1st-level cast only; scaling the healing dice with a higher slot is
//!   skipped.

use crate::rules::creature::{
    Condition, Cost, DamageKind, DamageRoll, Duration, Effect, HealRoll, Move, Rider, Strike,
};

use super::traits::{CreatureBuilder, FeatureError, FeaturePlugin, FeatureResult};

/// Both healing spells are cast here at their base, 1st-level, rate. See the
/// module doc: upcasting is out of scope.
const BASE_SLOT_LEVEL: u32 = 1;

/// The ability modifier a healing spell adds, read off the creature's own
/// casting profile - never a hardcoded number.
fn spellcasting_ability_modifier(
    builder: &CreatureBuilder,
    spell_name: &str,
) -> FeatureResult<i32> {
    builder
        .creature
        .spellcasting
        .map(|profile| profile.ability_modifier)
        .ok_or_else(|| {
            FeatureError::InvalidConfiguration(format!(
                "{spell_name} needs a [*.spellcasting] profile to compute its healing"
            ))
        })
}

/// Healing Word (SRD 5.2): Bonus Action, 60 feet, 1d4 + spellcasting ability
/// modifier. If the target is at 0 HP, it revives instead of only healing -
/// see [`crate::rules::creature::apply_healing`].
#[derive(Debug, Clone, Copy, Default)]
pub struct HealingWordPlugin;

impl HealingWordPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FeaturePlugin for HealingWordPlugin {
    fn id(&self) -> &'static str {
        "healing_word"
    }

    fn name(&self) -> &str {
        "Healing Word"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let modifier = spellcasting_ability_modifier(builder, "Healing Word")?;
        let heal = Move::new("Healing Word", Effect::Heal(HealRoll::new(1, 4, modifier)))
            .with_spell_slot(BASE_SLOT_LEVEL);
        builder.add_bonus_action(heal);
        Ok(())
    }
}

/// Cure Wounds (SRD 5.2, 2024 rules): Action, touch, 2d8 + spellcasting
/// ability modifier. Base 1st-level cast only - see the module doc.
#[derive(Debug, Clone, Copy, Default)]
pub struct CureWoundsPlugin;

impl CureWoundsPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FeaturePlugin for CureWoundsPlugin {
    fn id(&self) -> &'static str {
        "cure_wounds"
    }

    fn name(&self) -> &str {
        "Cure Wounds"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let modifier = spellcasting_ability_modifier(builder, "Cure Wounds")?;
        let heal = Move::new("Cure Wounds", Effect::Heal(HealRoll::new(2, 8, modifier)))
            .with_spell_slot(BASE_SLOT_LEVEL);
        builder.add_action(heal);
        Ok(())
    }
}

/// Guiding Bolt (SRD 5.2, 1st-level Evocation): a ranged spell attack for
/// 4d6 Radiant damage, using the caster's own spell attack bonus. On a hit,
/// the target is marked - [`Condition::Marked`] - so the next attack roll
/// made against it before the start of its own next turn, by anyone, has
/// Advantage.
///
/// `dice_count`/`dice_sides` are plugin parameters rather than `4` and `6`
/// baked in, the same way [`super::rogue::SneakAttackPlugin`]'s dice are - a
/// homebrew variant or a future upcast-aware caller can hand this a
/// different pool without a code change. Upcasting Guiding Bolt itself (more
/// dice from a higher-level slot) is not modelled: this always spends
/// exactly one 1st-level slot.
///
/// Consuming a spell slot during a live fight has no existing machinery of
/// its own yet - `SpellSlots` (ARCH-05) only ever tracks the *declared*
/// maximum and is never read by `sim::duel`, which only knows how to spend
/// from the named [`crate::rules::creature::Resource`] pool
/// [`crate::rules::creature::Cost`] already points at. Rather than build a
/// second, parallel spend-tracking mechanism for this one spell, this plugin
/// mirrors the caster's already-declared 1st-level slot count into a
/// same-named live resource (`spell_slot_1`) via the existing, idempotent
/// [`CreatureBuilder::ensure_resource`] - so a second 1st-level spell from a
/// sibling plugin naturally shares the same pool instead of getting its own.
/// That is a deliberately small bridge, not the final shape of spell-slot
/// spending; a real "cast at exactly this level" primitive belongs to a
/// later task once more than one spell needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuidingBoltPlugin {
    pub dice_count: u32,
    pub dice_sides: u32,
}

impl GuidingBoltPlugin {
    /// The printed 4d6.
    pub fn new() -> Self {
        Self::with_dice(4, 6)
    }

    /// As [`GuidingBoltPlugin::new`], with an overridden dice pool.
    pub fn with_dice(dice_count: u32, dice_sides: u32) -> Self {
        Self {
            dice_count,
            dice_sides,
        }
    }
}

impl Default for GuidingBoltPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl FeaturePlugin for GuidingBoltPlugin {
    fn id(&self) -> &'static str {
        "guiding_bolt"
    }

    fn name(&self) -> &str {
        "Guiding Bolt"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let profile = builder.creature.spellcasting.ok_or_else(|| {
            FeatureError::InvalidConfiguration(
                "guiding_bolt needs the creature's spellcasting profile set first \
                 (declare `[*.spellcasting]`, or list a spellcasting-granting feature \
                 before `guiding_bolt` in `features`)"
                    .to_string(),
            )
        })?;
        let to_hit = profile.attack_bonus();

        let slot_max = builder.creature.spell_slots.max(1);
        let slot_resource = builder.ensure_resource("spell_slot_1", slot_max);

        let strike = Strike::new(
            to_hit,
            vec![DamageRoll::new(
                self.dice_count,
                self.dice_sides,
                0,
                DamageKind::Radiant,
            )],
        );

        let action = Move::new("Guiding Bolt", Effect::Strikes { strike, count: 1 })
            .with_cost(Cost {
                resource: slot_resource,
                amount: 1,
            })
            .with_rider(Rider::ConditionOnHit {
                condition: Condition::Marked,
                duration: Duration::VictimTurn,
            });

        builder.add_action(action);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::plugin::traits::CreatureBuilder;
    use crate::prob::rng::Rng;
    use crate::rules::combat::{damage_pmf, sample_damage, Attack, Defense, RollMode};
    use crate::rules::creature::{apply_healing, is_down, Ability, Creature, SpellCastingProfile};

    fn wisdom_caster(ability_modifier: i32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 14, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, ability_modifier, 3));
        builder.set_spell_slot_max(1, 2);
        builder
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    fn caster_builder(ability_modifier: i32, proficiency_bonus: i32, slots: u32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 16, 30);
        builder.set_spellcasting(SpellCastingProfile::new(
            Ability::Wis,
            ability_modifier,
            proficiency_bonus,
        ));
        builder.set_spell_slot_max(1, slots);
        builder
    }

    #[test]
    fn healing_word_is_a_bonus_action_with_the_right_formula_and_slot() {
        let builder = wisdom_caster(3)
            .apply_feature(&HealingWordPlugin::new())
            .expect("Healing Word applies to a caster");

        assert!(builder.creature.actions.is_empty());
        assert_eq!(builder.creature.bonus_actions.len(), 1);
        let mv = &builder.creature.bonus_actions[0];
        assert_eq!(mv.name, "Healing Word");
        assert_eq!(mv.spell_slot_level, Some(1));
        match &mv.effect {
            Effect::Heal(roll) => {
                assert_eq!((roll.count, roll.sides, roll.bonus), (1, 4, 3));
            }
            other => panic!("expected a Heal effect, got {other:?}"),
        }
    }

    #[test]
    fn cure_wounds_is_an_action_with_the_right_formula_and_slot() {
        let builder = wisdom_caster(2)
            .apply_feature(&CureWoundsPlugin::new())
            .expect("Cure Wounds applies to a caster");

        assert!(builder.creature.bonus_actions.is_empty());
        assert_eq!(builder.creature.actions.len(), 1);
        let mv = &builder.creature.actions[0];
        assert_eq!(mv.name, "Cure Wounds");
        assert_eq!(mv.spell_slot_level, Some(1));
        match &mv.effect {
            Effect::Heal(roll) => {
                assert_eq!((roll.count, roll.sides, roll.bonus), (2, 8, 2));
            }
            other => panic!("expected a Heal effect, got {other:?}"),
        }
    }

    /// Neither spell hardcodes its modifier: two different casters produce
    /// two different heal formulas.
    #[test]
    fn the_modifier_comes_from_the_casting_profile_not_a_constant() {
        let low = wisdom_caster(0)
            .apply_feature(&CureWoundsPlugin::new())
            .unwrap();
        let high = wisdom_caster(5)
            .apply_feature(&CureWoundsPlugin::new())
            .unwrap();
        let Effect::Heal(low_roll) = &low.creature.actions[0].effect else {
            unreachable!()
        };
        let Effect::Heal(high_roll) = &high.creature.actions[0].effect else {
            unreachable!()
        };
        assert_eq!(low_roll.bonus, 0);
        assert_eq!(high_roll.bonus, 5);
    }

    #[test]
    fn a_non_caster_is_rejected_rather_than_silently_healing_for_zero() {
        let builder = CreatureBuilder::new("Mute", 10, 10);
        let err = builder
            .apply_feature(&HealingWordPlugin::new())
            .expect_err("no spellcasting profile means no formula to bake in");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    #[test]
    fn casting_either_spell_deducts_a_first_level_slot() {
        let builder = wisdom_caster(3)
            .apply_feature(&HealingWordPlugin::new())
            .unwrap();
        let mut caster = builder.creature;
        let word = caster.bonus_actions[0].clone();

        assert_eq!(caster.spell_slots.available(1), 2);
        assert!(word.pay_spell_cost(&mut caster));
        assert_eq!(caster.spell_slots.available(1), 1);
        assert!(word.pay_spell_cost(&mut caster));
        assert_eq!(caster.spell_slots.available(1), 0);
        // The pool is empty: casting fails rather than going negative.
        assert!(!word.pay_spell_cost(&mut caster));
        assert_eq!(caster.spell_slots.available(1), 0);
    }

    /// A move with no `spell_slot_level` (every other move in the game so
    /// far) must not be affected by this: `pay_spell_cost` is a no-op that
    /// always succeeds.
    #[test]
    fn a_move_without_a_spell_slot_pays_nothing() {
        let mut caster = Creature::new("Fighter", 16, 40);
        let punch = Move::new(
            "Punch",
            Effect::Strikes {
                strike: crate::rules::creature::Strike::new(
                    5,
                    vec![crate::rules::creature::DamageRoll::new(
                        1,
                        4,
                        2,
                        crate::rules::creature::DamageKind::Bludgeoning,
                    )],
                ),
                count: 1,
            },
        );
        assert!(punch.pay_spell_cost(&mut caster));
    }

    /// The core invariant this whole project is built on: the exact
    /// distribution and many samples of the same roll must agree, applied
    /// here to healing instead of damage.
    #[test]
    fn healing_word_amount_matches_between_exact_and_sampled() {
        let roll = HealRoll::new(1, 4, 3);
        let exact = roll.pmf();
        assert!((exact.total() - 1.0).abs() < 1e-12);
        assert_eq!((exact.min(), exact.max()), (4, 7));

        let mut rng = Rng::new(7);
        const N: usize = 200_000;
        let mut total = 0i64;
        for _ in 0..N {
            let sampled = roll.sample(&mut rng);
            assert!((4..=7).contains(&sampled));
            total += i64::from(sampled);
        }
        let mean_sampled = total as f64 / N as f64;
        let tol = 5.0 * (exact.variance() / N as f64).sqrt() + 1e-3;
        assert!(
            (mean_sampled - exact.mean()).abs() < tol,
            "sampled mean {mean_sampled} vs exact {}",
            exact.mean()
        );
    }

    #[test]
    fn cure_wounds_amount_matches_between_exact_and_sampled() {
        let roll = HealRoll::new(2, 8, 4);
        let exact = roll.pmf();
        assert_eq!((exact.min(), exact.max()), (6, 20));

        let mut rng = Rng::new(11);
        const N: usize = 200_000;
        let mut total = 0i64;
        for _ in 0..N {
            let sampled = roll.sample(&mut rng);
            assert!((6..=20).contains(&sampled));
            total += i64::from(sampled);
        }
        let mean_sampled = total as f64 / N as f64;
        let tol = 5.0 * (exact.variance() / N as f64).sqrt() + 1e-3;
        assert!(
            (mean_sampled - exact.mean()).abs() < tol,
            "sampled mean {mean_sampled} vs exact {}",
            exact.mean()
        );
    }

    #[test]
    fn healing_from_zero_revives_and_healing_from_positive_does_not() {
        let (new_hp, revived) = apply_healing(0, 30, 5);
        assert_eq!(new_hp, 5);
        assert!(revived, "regaining HP from 0 wakes the creature up");

        let (new_hp, revived) = apply_healing(12, 30, 5);
        assert_eq!(new_hp, 17);
        assert!(!revived, "never went down, so there is nothing to revive");
    }

    /// This engine never clamps HP at zero (a fighter's HP can read
    /// negative from overkill damage), so "down" has to mean "at or below
    /// zero", not "exactly zero".
    #[test]
    fn a_deeply_negative_target_still_revives_once_healed_past_zero() {
        assert!(is_down(-15));
        let (new_hp, revived) = apply_healing(-15, 30, 20);
        assert_eq!(new_hp, 5);
        assert!(revived);
    }

    /// Healing that does not clear zero leaves the creature down - reaching
    /// exactly 0 is still "at 0 HP", not revived, matching 5e's own wording.
    #[test]
    fn healing_that_does_not_cross_zero_does_not_revive() {
        let (new_hp, revived) = apply_healing(-15, 30, 10);
        assert_eq!(new_hp, -5);
        assert!(!revived);

        let (new_hp, revived) = apply_healing(-10, 30, 10);
        assert_eq!(new_hp, 0);
        assert!(!revived, "landing exactly on 0 is still down");
    }

    #[test]
    fn healing_never_exceeds_max_hp() {
        let (new_hp, _) = apply_healing(28, 30, 100);
        assert_eq!(new_hp, 30);
    }

    /// End to end: cast Healing Word on a downed ally - pay the slot, roll
    /// the heal, apply it, and confirm the revive.
    #[test]
    fn casting_healing_word_on_a_downed_ally_revives_them() {
        let builder = wisdom_caster(4)
            .apply_feature(&HealingWordPlugin::new())
            .unwrap();
        let mut caster = builder.creature;
        let word = caster.bonus_actions[0].clone();

        assert!(word.pay_spell_cost(&mut caster));

        let Effect::Heal(roll) = &word.effect else {
            unreachable!()
        };
        let mut rng = Rng::new(42);
        let healed = roll.sample(&mut rng);
        assert!((5..=8).contains(&healed), "1d4 + 4 is 5..=8");

        let ally_max_hp = 24;
        let (new_hp, revived) = apply_healing(0, ally_max_hp, healed);
        assert!(new_hp > 0);
        assert!(revived);
    }

    #[test]
    fn applying_grants_an_action_using_the_casters_computed_attack_bonus() {
        let builder = caster_builder(3, 2, 2); // attack bonus 5
        let built = builder
            .apply_feature(&GuidingBoltPlugin::new())
            .expect("guiding bolt applies to a caster")
            .build()
            .expect("builds");

        assert_eq!(built.actions.len(), 1);
        let action = &built.actions[0];
        assert_eq!(action.name, "Guiding Bolt");
        match &action.effect {
            Effect::Strikes { strike, count } => {
                assert_eq!(*count, 1);
                assert_eq!(strike.to_hit, 5, "5 = 3 (ability mod) + 2 (proficiency)");
                assert_eq!(
                    strike.damage,
                    vec![DamageRoll::new(4, 6, 0, DamageKind::Radiant)]
                );
            }
            other => panic!("expected a Strikes effect, got {other:?}"),
        }

        // A 1st-level slot, spent from a resource pool mirroring the
        // caster's own declared maximum.
        assert_eq!(
            action.cost,
            Some(Cost {
                resource: 0,
                amount: 1
            })
        );
        assert_eq!(built.resources[0].name, "spell_slot_1");
        assert_eq!(built.resources[0].max, 2);

        // The mark, applied unconditionally on a hit.
        assert_eq!(
            action.riders,
            vec![Rider::ConditionOnHit {
                condition: Condition::Marked,
                duration: Duration::VictimTurn,
            }]
        );
    }

    /// A different profile and slot count produce a different bonus and a
    /// different pool size, proving neither was a hardcoded constant.
    #[test]
    fn a_differently_configured_caster_gets_its_own_numbers() {
        let mut builder = caster_builder(4, 3, 1); // attack bonus 7
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 4, 3).with_item_bonus(1));
        let built = builder
            .apply_feature(&GuidingBoltPlugin::new())
            .expect("guiding bolt applies")
            .build()
            .expect("builds");

        let Effect::Strikes { strike, .. } = &built.actions[0].effect else {
            panic!("expected Strikes");
        };
        assert_eq!(strike.to_hit, 8, "4 (mod) + 3 (proficiency) + 1 (item)");
        assert_eq!(built.resources[0].max, 1);
    }

    /// `dice_count`/`dice_sides` are parameters, not the printed 4d6 baked
    /// in - the same test shape `sneak_attack`'s own parameterization gets.
    #[test]
    fn dice_are_a_parameter_not_a_hardcoded_constant() {
        let mut builder = caster_builder(2, 2, 1);
        let built = builder
            .apply_feature(&GuidingBoltPlugin::with_dice(5, 6))
            .expect("guiding bolt applies")
            .build()
            .expect("builds");

        let Effect::Strikes { strike, .. } = &built.actions[0].effect else {
            panic!("expected Strikes");
        };
        assert_eq!(
            strike.damage,
            vec![DamageRoll::new(5, 6, 0, DamageKind::Radiant)]
        );
    }

    #[test]
    fn applying_without_a_spellcasting_profile_is_rejected() {
        let mut builder = CreatureBuilder::new("Not A Caster", 16, 30);
        let err = builder
            .apply_feature(&GuidingBoltPlugin::new())
            .unwrap_err();
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
        assert!(builder.creature.actions.is_empty());
    }

    /// A sibling 1st-level spell feature sharing the same slot level should
    /// end up drawing from the very same resource pool rather than getting
    /// its own - `ensure_resource` is idempotent by name, and this proves
    /// this plugin actually leans on that rather than reinventing a pool.
    #[test]
    fn a_second_first_level_spell_shares_the_same_slot_pool() {
        let mut builder = caster_builder(3, 2, 3);
        builder.ensure_resource("spell_slot_1", 3);
        let idx_before = builder.resource_index("spell_slot_1").unwrap();

        let built = builder
            .apply_feature(&GuidingBoltPlugin::new())
            .expect("guiding bolt applies")
            .build()
            .expect("builds");

        assert_eq!(built.resources.len(), 1, "no second pool was created");
        assert_eq!(built.actions[0].cost.unwrap().resource, idx_before);
    }

    /// The `Strike`'s own damage distribution: a miss deals nothing, a hit
    /// or crit deals 4d6, and the sampled path must land on exactly the
    /// exact one - the project's standing exact-vs-sampled contract.
    #[test]
    fn sampled_damage_agrees_with_the_exact_path() {
        let mut builder = caster_builder(4, 3, 1); // attack bonus 7
        let built = builder
            .apply_feature(&GuidingBoltPlugin::new())
            .expect("guiding bolt applies")
            .build()
            .expect("builds");
        let Effect::Strikes { strike, .. } = &built.actions[0].effect else {
            panic!("expected Strikes");
        };

        let target = crate::rules::creature::Creature::new("target", 15, 40);
        let exact = strike.damage_pmf(&target);
        assert!(close(exact.total(), 1.0));
        assert!(exact.min() >= 0, "damage floors at zero");
        assert!(exact.prob(0) > 0.0, "a miss must be possible");
        // 4d6 tops at 24, doubled to 48 on a crit.
        assert_eq!(exact.max(), 48);

        let (lo, hi) = (exact.min(), exact.max());
        let mut rng = Rng::new(4_200);
        let n = 100_000;
        let mut counts = vec![0usize; (hi - lo + 1) as usize];
        for _ in 0..n {
            let d = strike.sample(&mut rng, &target);
            assert!(d >= lo && d <= hi, "sampled {d} outside {lo}..={hi}");
            counts[(d - lo) as usize] += 1;
        }
        for (i, &c) in counts.iter().enumerate() {
            let value = lo + i as i32;
            let want = exact.prob(value);
            let got = c as f64 / n as f64;
            let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
            assert!(
                (got - want).abs() < tol,
                "P(damage = {value}) sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
            );
        }
    }

    /// Acceptance criterion: Guiding Bolt is a spell attack roll
    /// ([`Attack::is_spell_attack`]) whose damage profile composes with the
    /// AT-02 sneak-attack-style extension exactly like a weapon attack does,
    /// as soon as the attack has Advantage - from this exact mark's own
    /// prior use, or any other source. Verified at the [`rules::combat`]
    /// level, the same way AT-02's own tests prove the gate, and checked
    /// exact-vs-sampled.
    ///
    /// [`rules::combat`]: crate::rules::combat
    #[test]
    fn guiding_bolts_profile_composes_with_the_sneak_attack_style_spell_extension_under_advantage()
    {
        let attack = Attack::new(7, 4, 6, 0)
            .with_is_spell_attack(true)
            .with_mode(RollMode::Advantage);

        // A rogue-shaped rider that opted into the spell-attack extension.
        let sneak = Rider::ConditionalExtraDamage {
            dice_count: 3,
            dice_sides: 6,
            once_per_turn: true,
        };
        let extra = sneak
            .extra_damage_for_with_spell_attack_extension(&attack, false, true)
            .expect("a spell attack at advantage should qualify with the extension");
        let with_sneak = attack.with_damage_rider(extra);

        let defense = Defense::new(14, 60);
        let exact = damage_pmf(&with_sneak, &defense);
        let (lo, hi) = (exact.min(), exact.max());
        let mut rng = Rng::new(4_300);
        let n = 100_000;
        let mut counts = vec![0usize; (hi - lo + 1) as usize];
        for _ in 0..n {
            let d = sample_damage(&mut rng, &with_sneak, &defense);
            assert!(d >= lo && d <= hi, "sampled {d} outside {lo}..={hi}");
            counts[(d - lo) as usize] += 1;
        }
        for (i, &c) in counts.iter().enumerate() {
            let value = lo + i as i32;
            let want = exact.prob(value);
            let got = c as f64 / n as f64;
            let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
            assert!(
                (got - want).abs() < tol,
                "P(damage = {value}) sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
            );
        }

        // Without the extension, the very same spell attack must not
        // qualify - the extension is what makes it additive, not automatic.
        let attack_unextended = Attack::new(7, 4, 6, 0)
            .with_is_spell_attack(true)
            .with_mode(RollMode::Advantage);
        assert_eq!(
            sneak.extra_damage_for_with_spell_attack_extension(&attack_unextended, false, false),
            None
        );
    }
}
