//! Hold Person (SRD 5.2, 2024 rules).
//!
//! A 2nd-level spell that paralyzes a humanoid who fails a Wisdom saving
//! throw, for as long as the caster keeps concentrating (up to 1 minute),
//! repeating the save at the end of the target's own turns.
//!
//! Every clause of that sentence is machinery this engine already has,
//! rather than anything new:
//! - the save and its DC are `Effect::Save`'s ordinary business, with the DC
//!   read off the caster's own [`crate::rules::SpellCastingProfile`]
//!   rather than hardcoded;
//! - "a humanoid" is [`crate::creature::SaveEffect::requires_type`],
//!   checked before a save is even rolled - a non-humanoid is not caught at
//!   all, not caught-and-then-unaffected;
//! - "repeating the save at the end of its turns" is
//!   [`crate::rules::Duration::SaveEndTurn`], which already drives
//!   `sim::duel`'s end-of-turn save loop for any condition that carries it;
//! - "for as long as the caster concentrates" is
//!   [`crate::creature::Move::concentration`] - `sim::duel`'s
//!   concentration tracker clears the condition from every target it is
//!   maintaining the instant that ends, save or no save;
//! - the auto-crit a Paralyzed target grants an attacker is
//!   [`crate::rules::Condition::auto_crits`], already read by every
//!   strike this engine resolves.
//!
//! Hold Person is not modelled beyond its base cast either, for the same
//! reasons as every other spell here (see the [`super`] module docs):
//! range/positioning is out of scope, and upcasting (catching more than one
//! humanoid) is skipped.

use crate::creature::{Effect, Move, MoveKind, SaveEffect};
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{Ability, Condition, Duration};

/// Hold Person is always cast from a 2nd-level slot here; see the module doc.
const SLOT_LEVEL: u32 = 2;

/// The base, non-upcast version catches exactly one creature; see the module
/// doc's note on upcasting.
const MAX_TARGETS: u32 = 1;

/// Hold Person's own targeting restriction: the SRD names the creature type
/// by this exact word, matched case-insensitively by
/// [`crate::creature::Creature::is_creature_type`].
const TARGET_TYPE: &str = "Humanoid";

/// Hold Person (SRD 5.2): Action, 2nd level, concentration up to 1 minute.
/// One humanoid within range makes a Wisdom save or becomes Paralyzed,
/// repeating the save at the end of each of its own turns.
#[derive(Debug, Clone, Copy, Default)]
pub struct HoldPersonPlugin;

impl HoldPersonPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl FeaturePlugin for HoldPersonPlugin {
    fn id(&self) -> &'static str {
        "hold_person"
    }

    fn name(&self) -> &str {
        "Hold Person"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = builder.creature.spell_save_dc().ok_or_else(|| {
            FeatureError::InvalidConfiguration(
                "Hold Person needs a [*.spellcasting] profile to compute its save DC".to_string(),
            )
        })?;

        let hold_person = Move::new(
            "Hold Person",
            Effect::Save(SaveEffect {
                ability: Ability::Wis,
                dc,
                damage: Vec::new(),
                half_on_success: false,
                on_failure: vec![(
                    Condition::Paralyzed,
                    Duration::SaveEndTurn {
                        ability: Ability::Wis,
                        dc,
                    },
                )],
                max_targets: Some(MAX_TARGETS),
                requires_type: Some(TARGET_TYPE.to_string()),
            }),
        )
        .with_spell_slot(SLOT_LEVEL)
        .with_concentration()
        .with_kind(MoveKind::Spell);

        builder.add_action(hold_person);
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Hold Person
    registry.register("hold_person", |_val| Ok(Box::new(HoldPersonPlugin::new())));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Creature;
    use crate::creature::Strike;
    use crate::prob::Rng;
    use crate::rules::{CreatureType, DamageKind, DamageRoll, SpellCastingProfile};
    use crate::sim::{run, run_teams, Budget, Policy};

    fn wisdom_caster(ability_modifier: i32, proficiency_bonus: i32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Cleric", 14, 30);
        builder.set_spellcasting(SpellCastingProfile::new(
            Ability::Wis,
            ability_modifier,
            proficiency_bonus,
        ));
        builder.set_spell_slot_max(1, 2);
        builder.set_spell_slot_max(2, 1);
        builder
    }

    #[test]
    fn hold_person_is_a_concentration_action_with_the_right_slot_and_save() {
        let builder = wisdom_caster(3, 3)
            .apply_feature(&HoldPersonPlugin::new())
            .expect("Hold Person applies to a caster");

        assert!(builder.creature.bonus_actions.is_empty());
        assert_eq!(builder.creature.actions.len(), 1);
        let mv = &builder.creature.actions[0];
        assert_eq!(mv.name, "Hold Person");
        assert_eq!(mv.spell_slot_level, Some(2));
        assert!(mv.concentration, "Hold Person must require concentration");

        match &mv.effect {
            Effect::Save(save) => {
                assert_eq!(save.ability, Ability::Wis);
                assert_eq!(save.dc, 14); // 8 + 3 (mod) + 3 (prof)
                assert!(save.damage.is_empty(), "Hold Person deals no damage");
                assert_eq!(save.max_targets, Some(1));
                assert_eq!(save.requires_type.as_deref(), Some("Humanoid"));
                assert_eq!(
                    save.on_failure,
                    vec![(
                        Condition::Paralyzed,
                        Duration::SaveEndTurn {
                            ability: Ability::Wis,
                            dc: 14,
                        },
                    )]
                );
            }
            other => panic!("expected a Save effect, got {other:?}"),
        }
    }

    #[test]
    fn a_non_caster_is_rejected_rather_than_baking_in_no_dc() {
        let builder = CreatureBuilder::new("Mute", 10, 10);
        let err = builder
            .apply_feature(&HoldPersonPlugin::new())
            .expect_err("no spellcasting profile means no DC to bake in");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    /// Two different casters must bake in two different DCs: nothing here is
    /// allowed to hardcode "DC 13" the way a homebrew shortcut would.
    #[test]
    fn the_dc_comes_from_the_casting_profile_not_a_constant() {
        let low = wisdom_caster(0, 2)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap();
        let high = wisdom_caster(5, 4)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap();
        let Effect::Save(low_save) = &low.creature.actions[0].effect else {
            unreachable!()
        };
        let Effect::Save(high_save) = &high.creature.actions[0].effect else {
            unreachable!()
        };
        assert_eq!(low_save.dc, 10); // 8 + 0 + 2
        assert_eq!(high_save.dc, 17); // 8 + 5 + 4
        assert_ne!(low_save.dc, high_save.dc);
    }

    /// The core invariant this whole project is built on, applied to a save
    /// that produces no damage at all: the exact failure chance and a large
    /// sample of rolled saves must agree.
    #[test]
    fn hold_person_failure_rate_matches_between_exact_and_sampled() {
        let builder = wisdom_caster(4, 3)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap();
        let Effect::Save(save) = &builder.creature.actions[0].effect else {
            unreachable!()
        };

        let mut target = Creature::new("Guard", 12, 11);
        target.creature_type = Some(CreatureType::Humanoid);
        target.saves[Ability::Wis.index()] = 1;

        let exact = save.failure_chance(&target);
        let mut rng = Rng::new(99);
        const N: usize = 200_000;
        let mut fails = 0usize;
        for _ in 0..N {
            let (_, saved) = save.sample(&mut rng, &target);
            if !saved {
                fails += 1;
            }
        }
        let got = fails as f64 / N as f64;
        let tol = 5.0 * (exact * (1.0 - exact) / N as f64).sqrt() + 1e-4;
        assert!(
            (got - exact).abs() < tol,
            "sampled failure rate {got:.5}, exact {exact:.5}, tolerance {tol:.5}"
        );
    }

    /// End to end: casting Hold Person on a humanoid that fails its save
    /// paralyzes it, and a follow-up strike in the *same* turn (a bonus
    /// action, after the action that cast the spell) auto-crits - the whole
    /// reason Paralyzed is worse than Stunned.
    #[test]
    fn casting_hold_person_paralyzes_a_humanoid_and_a_same_turn_strike_auto_crits() {
        let mut caster = wisdom_caster(4, 3)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap()
            .creature;
        caster.initiative = 100;
        caster.bonus_actions.push(Move::new(
            "Dagger",
            Effect::Strikes {
                strike: Strike::new(30, vec![DamageRoll::new(1, 4, 0, DamageKind::Piercing)]),
                count: 1,
            },
        ));

        let mut victim = Creature::new("Bandit", 10, 20);
        victim.creature_type = Some(CreatureType::Humanoid);
        victim.saves[Ability::Wis.index()] = -50; // fails every Wisdom save, unconditionally
        victim.initiative = -100;

        let mut rng = Rng::new(7);
        let mut log = Some(Vec::new());
        run(
            &mut rng,
            [&caster, &victim],
            [Policy::Greedy; 2],
            1,
            &mut log,
        );
        let narration = log.unwrap().join("\n");
        assert!(
            narration.contains("paralyzed"),
            "the save should fail and land Paralyzed:\n{narration}"
        );
        assert!(
            narration.contains("crit"),
            "a follow-up hit against a paralyzed target must be an automatic crit:\n{narration}"
        );
    }

    /// The type restriction is checked before a save is even rolled: a
    /// non-humanoid is never caught, no matter how unbeatable the save would
    /// have been.
    #[test]
    fn hold_person_never_catches_a_non_humanoid() {
        let mut caster = wisdom_caster(4, 3)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap()
            .creature;
        caster.initiative = 100;

        let mut dragon = Creature::new("Wyrmling", 17, 60);
        dragon.creature_type = Some(CreatureType::Dragon);
        dragon.saves[Ability::Wis.index()] = -50; // would always fail, if it were even caught
        dragon.initiative = -100;

        let mut rng = Rng::new(3);
        let mut log = Some(Vec::new());
        let o = run(
            &mut rng,
            [&caster, &dragon],
            [Policy::Greedy; 2],
            3,
            &mut log,
        );
        let narration = log.unwrap().join("\n");
        assert!(
            !narration.contains("paralyzed"),
            "a dragon must never be caught by a humanoid-only spell:\n{narration}"
        );
        assert_eq!(
            o.turns_lost[1], 0,
            "never paralyzed, so it never loses a turn to it"
        );
    }

    /// A single 2nd-level slot casts Hold Person exactly once, even when the
    /// caster's only action is Hold Person and the target's save can never
    /// succeed: after the first cast, the empty pool - not a lack of
    /// desire - is what stops a second one.
    #[test]
    fn hold_person_only_casts_as_many_times_as_it_has_slots_for() {
        let caster = wisdom_caster(4, 3)
            .apply_feature(&HoldPersonPlugin::new())
            .unwrap()
            .creature;
        assert_eq!(caster.spell_slots.available(2), 1);
        let mut caster = caster;
        caster.initiative = 100;

        let mut victim = Creature::new("Bandit", 10, 20);
        victim.creature_type = Some(CreatureType::Humanoid);
        victim.saves[Ability::Wis.index()] = -50;
        victim.initiative = -100;

        let mut rng = Rng::new(11);
        let mut log = None;
        let o = run(
            &mut rng,
            [&caster, &victim],
            [Policy::Greedy; 2],
            4,
            &mut log,
        );
        assert_eq!(
            o.resources_spent[0], 1,
            "one slot means one cast, however many turns follow"
        );
    }

    /// Hold Person's own repeat save is not academic: with a beatable Wisdom
    /// save, the check `sim::duel`'s end-of-turn loop runs at the end of
    /// every one of the victim's turns actually clears the condition, the
    /// same persistent-vs-easy contrast `sim::duel`'s own
    /// `a_repeatable_save_can_end_a_condition_the_turn_it_lands` draws for
    /// `Duration::SaveEndTurn` in general, reproduced here for the concrete
    /// spell rather than a synthetic rider.
    #[test]
    fn hold_persons_own_repeat_save_can_end_it_the_turn_it_lands() {
        let tally = |save_bonus: i32| {
            let mut rng = Rng::new(23);
            let (mut lost, mut fights) = (0u32, 0u32);
            for _ in 0..300 {
                let mut caster = wisdom_caster(2, 2) // dc = 8 + 2 + 2 = 12
                    .apply_feature(&HoldPersonPlugin::new())
                    .unwrap()
                    .creature;
                caster.initiative = 100;

                let mut victim = Creature::new("Bandit", 10, 20);
                victim.creature_type = Some(CreatureType::Humanoid);
                victim.saves[Ability::Wis.index()] = save_bonus;
                victim.initiative = -100;

                let mut log = None;
                let o = run(
                    &mut rng,
                    [&caster, &victim],
                    [Policy::Greedy; 2],
                    6,
                    &mut log,
                );
                lost += o.turns_lost[1];
                fights += 1;
            }
            f64::from(lost) / f64::from(fights)
        };

        let persistent = tally(-50); // needs a 62 on a d20: never happens
        let escapable = tally(8); // dc 12 needs only a 4+: usually saves

        assert!(
            persistent > 3.0,
            "an unbeatable Hold Person save should cost most turns: {persistent}"
        );
        assert!(
            escapable < persistent,
            "a beatable repeat save should free the victim sooner: {escapable} vs {persistent}"
        );
    }

    /// Hold Person is a concentration spell: when the caster's concentration
    /// breaks, the paralysis it was maintaining lifts immediately, without
    /// waiting for - or needing - the victim's own repeated save at all.
    #[test]
    fn losing_concentration_ends_hold_persons_paralysis_early() {
        fn caster() -> Creature {
            let mut c = wisdom_caster(4, 3)
                .apply_feature(&HoldPersonPlugin::new())
                .unwrap()
                .creature;
            c.initiative = 100;
            // Forced to roll, this save always fails, so any damage that
            // reaches the caster ends concentration outright.
            c.saves[Ability::Con.index()] = -50;
            c
        }

        fn victim() -> Creature {
            let mut v = Creature::new("Bandit", 10, 20);
            v.creature_type = Some(CreatureType::Humanoid);
            v.team = 1;
            v.initiative = -50;
            v.saves[Ability::Wis.index()] = -50; // never saves on its own
            v
        }

        // Control: nothing threatens the caster, so concentration never
        // breaks and the victim's own Wisdom save never succeeds either -
        // the paralysis should hold for every round that follows.
        let held = {
            let c = caster();
            let v = victim();
            let mut rng = Rng::new(4);
            let mut log = None;
            run(&mut rng, [&c, &v], [Policy::Greedy; 2], 3, &mut log).turns_lost[1]
        };
        assert!(
            held >= 2,
            "an unbroken Hold Person should cost the victim multiple turns: {held}"
        );

        // An ally that reaches the caster forces - and always fails - the
        // concentration save, ending Hold Person before the victim's own
        // save ever gets the credit for freeing it.
        let broken = {
            let c = caster();
            let v = victim();
            let mut ally = Creature::new("Wolf", 12, 15).with_action(Move::new(
                "Bite",
                Effect::Strikes {
                    strike: Strike::new(30, vec![DamageRoll::new(2, 6, 4, DamageKind::Piercing)]),
                    count: 1,
                },
            ));
            ally.creature_type = Some(CreatureType::Beast);
            ally.team = 1;
            ally.initiative = 50; // after the caster, before the victim

            let mut rng = Rng::new(4);
            let mut log = None;
            run_teams(
                &mut rng,
                &[&c, &v, &ally],
                [Policy::Greedy; 2],
                3,
                Budget::default(),
                &mut log,
            )
            .turns_lost[1]
        };
        assert!(
            broken < held,
            "a broken concentration should cost the victim fewer turns than an unbroken one: {broken} vs {held}"
        );
    }
}
