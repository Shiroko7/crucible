//! What a landed hit sets off: on-hit riders' saves and conditions, a
//! Cunning Strike effect, an injury poison dose.

use crate::creature::Rider;
use crate::prob::Rng;
use crate::rules::{Ability, Condition, Duration};
use crate::sim::fight::saves::saving_throw;
use crate::sim::fight::{Cunning, Fight};

impl<'a> Fight<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fire_on_hit(
        &mut self,
        rng: &mut Rng,
        me: usize,
        target: usize,
        move_riders: &[Rider],
        record: bool,
        notes: &mut Vec<String>,
        landed_conditions: &mut Vec<(usize, Condition)>,
    ) {
        for rider in move_riders {
            match rider {
                Rider::SaveOrCondition {
                    ability,
                    dc,
                    condition,
                    duration,
                    cost,
                    once_per_turn,
                } => {
                    if *once_per_turn && self.fighters[me].once_per_turn_spent {
                        continue;
                    }
                    let (conditions, advantage) =
                        self.conditions_against(me, target, &[(*condition, *duration)]);
                    if conditions.is_empty() {
                        // Immune: nothing to spend the rider on.
                        continue;
                    }
                    if !self.fighters[me].can_pay(*cost) {
                        continue;
                    }
                    self.fighters[me].pay(*cost);
                    if *once_per_turn {
                        self.fighters[me].once_per_turn_spent = true;
                    }

                    let (saved, resisted) =
                        saving_throw(&mut self.fighters, rng, target, *ability, *dc, advantage);
                    if !saved {
                        self.land_condition(me, target, *condition, *duration, landed_conditions);
                    }
                    if record {
                        notes.push(format!(
                            "{} {}",
                            condition.name(),
                            match (saved, resisted) {
                                (true, true) => "shrugged off (legendary resistance)",
                                (true, false) => "saved",
                                _ => "LANDED",
                            }
                        ));
                    }
                }
                Rider::ConditionOnHit {
                    condition,
                    duration,
                } => {
                    let (conditions, _) =
                        self.conditions_against(me, target, &[(*condition, *duration)]);
                    for (condition, duration) in conditions {
                        self.land_condition(me, target, condition, duration, landed_conditions);
                        if record {
                            notes.push(format!("{} (no save)", condition.name()));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// The Cunning Strike effect [`Fight::extra_damage_plan`] already paid a
    /// Sneak Attack die for, resolved once the hit has landed: a saving throw
    /// against the rogue's Cunning Strike DC, through the same save handling
    /// as every other rider - Legendary Resistance, a condition-immunity
    /// downgrade and all.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_cunning_strike(
        &mut self,
        rng: &mut Rng,
        me: usize,
        target: usize,
        choice: Cunning,
        record: bool,
        notes: &mut Vec<String>,
        landed_conditions: &mut Vec<(usize, Condition)>,
    ) {
        if !self.fighters[target].alive() {
            return;
        }
        let (ability, dc, condition, duration) = match choice {
            // 2024 Poison: a Constitution save or Poisoned for a minute,
            // repeating the save at the end of each of its turns.
            Cunning::Poison { dc } => (
                Ability::Con,
                dc,
                Condition::Poisoned,
                Duration::SaveEndTurn {
                    ability: Ability::Con,
                    dc,
                },
            ),
            // 2024 Trip: a Dexterity save or Prone, until it stands up on its
            // own next turn.
            Cunning::Trip { dc } => (Ability::Dex, dc, Condition::Prone, Duration::VictimTurn),
        };
        let (conditions, advantage) = self.conditions_against(me, target, &[(condition, duration)]);
        if conditions.is_empty() {
            return;
        }
        let (saved, resisted) =
            saving_throw(&mut self.fighters, rng, target, ability, dc, advantage);
        if !saved {
            self.land_condition(me, target, condition, duration, landed_conditions);
        }
        if record {
            notes.push(format!(
                "cunning strike {} {}",
                condition.name(),
                match (saved, resisted) {
                    (true, true) => "shrugged off (legendary resistance)",
                    (true, false) => "saved",
                    _ => "LANDED",
                }
            ));
        }
    }

    /// Use up the attacker's injury-poison dose, if it has one left, on a
    /// weapon hit: the forcing save, and on a failure the burden and any
    /// condition it carries. See [`Rider::InjuryPoison`].
    pub(super) fn resolve_injury_poison(
        &mut self,
        rng: &mut Rng,
        me: usize,
        target: usize,
        record: bool,
        notes: &mut Vec<String>,
        landed_conditions: &mut Vec<(usize, Condition)>,
    ) {
        if !self.fighters[target].alive() {
            return;
        }
        let creature = self.fighters[me].creature;
        let Some(i) = creature.riders.iter().enumerate().position(|(i, r)| {
            matches!(r, Rider::InjuryPoison { .. }) && self.fighters[me].rider_uses[i] > 0
        }) else {
            return;
        };
        self.fighters[me].rider_uses[i] -= 1;
        let rider = &creature.riders[i];
        let Some((ability, dc, _, _)) = rider.injury_poison() else {
            return;
        };
        let (conditions, advantage) =
            self.conditions_against(me, target, &rider.injury_poison_conditions());
        if conditions.is_empty() {
            return;
        }
        let (saved, resisted) =
            saving_throw(&mut self.fighters, rng, target, ability, dc, advantage);
        if !saved {
            for (condition, duration) in &conditions {
                self.land_condition(me, target, *condition, *duration, landed_conditions);
            }
        }
        if record {
            notes.push(format!(
                "injury poison {}",
                match (saved, resisted) {
                    (true, true) => "shrugged off (legendary resistance)",
                    (true, false) => "saved",
                    _ => "LANDED",
                }
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{AttackKind, Creature, Effect};
    use crate::rules::{DamageKind, DamageRoll, RollMode, Size};
    use crate::sim::fight::saves::save_mode;
    use crate::sim::fight::test_support::{bow, fight_of, no_log, sneak_attacker, strike_once};
    use crate::sim::fight::Expiry;
    use crate::sim::{Budget, Policy, Side};

    // Every damage rider, Cunning Strike choice, injury dose, weapon buff and
    // reaction exercised here existed and was unit-tested as a mechanism (in
    // `creature::rider`) before the fight loop actually used any of them.
    // These pin them to the loop.

    /// [`Rider::ConditionOnHit`] applies its condition unconditionally on a
    /// hit - no saving throw rolled at all, unlike [`Rider::SaveOrCondition`].
    #[test]
    fn condition_on_hit_marks_its_target_unconditionally_with_no_save() {
        let caster = Creature::new("caster", 10, 20);
        let target = Creature::new("target", 10, 20);
        let roster = [(&caster, Side::A), (&target, Side::B)];
        let mut rng = Rng::new(13);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::Greedy; 2],
            5,
            Budget::default(),
            &mut log,
        );

        let riders = [Rider::ConditionOnHit {
            condition: Condition::Marked,
            duration: Duration::VictimTurn,
        }];
        let mut notes = Vec::new();
        let mut landed_conditions = Vec::new();
        fight.fire_on_hit(
            &mut rng,
            0,
            1,
            &riders,
            false,
            &mut notes,
            &mut landed_conditions,
        );

        assert!(fight.fighters[1].has(|c| c == Condition::Marked));
        assert_eq!(landed_conditions, vec![(1, Condition::Marked)]);
    }

    fn cunning_rogue() -> Creature {
        sneak_attacker("rogue")
            .with_rider(Rider::CunningStrike { dc: 99 })
            .with_rider(Rider::CunningStrikeTrip)
    }

    /// Cunning Strike, live: a die comes off Sneak Attack for Poison while
    /// the target is not yet poisoned, the save is forced (and failed, at DC
    /// 99), and once it is poisoned the rogue goes back to full damage - or
    /// trips a small enough target in melee.
    #[test]
    fn cunning_strike_spends_a_die_on_poison_then_on_trip_in_melee() {
        let rogue = cunning_rogue();
        let target = Creature::new("target", 1, 1_000);
        let roster = [(&rogue, Side::A), (&target, Side::B)];
        let (mut fight, mut rng) = fight_of(&roster, 5);
        let ranged = bow(30, RollMode::Advantage);
        let Effect::Strikes { strike, .. } = &ranged.effect else {
            unreachable!()
        };

        let plan = fight.extra_damage_plan(0, 1, strike, &[], RollMode::Advantage, true);
        assert_eq!(plan.cunning, Some(Cunning::Poison { dc: 99 }));
        assert_eq!(plan.rolls[0].count, 2, "3d6 less the die spent");

        while !fight.fighters[1].has(|c| c == Condition::Poisoned) {
            fight.fighters[0].sneak_attack_spent = false;
            strike_once(&mut fight, &mut rng, 0, 1, &ranged);
        }
        fight.fighters[0].sneak_attack_spent = false;
        let plan = fight.extra_damage_plan(0, 1, strike, &[], RollMode::Advantage, true);
        assert_eq!(plan.cunning, None, "never trips from range");
        assert_eq!(plan.rolls[0].count, 3);

        let mut melee = strike.clone();
        melee.kind = AttackKind {
            finesse: true,
            ..AttackKind::MELEE_WEAPON
        };
        let plan = fight.extra_damage_plan(0, 1, &melee, &[], RollMode::Advantage, true);
        assert_eq!(plan.cunning, Some(Cunning::Trip { dc: 99 }));

        // A Huge target cannot be tripped at all.
        let huge = Creature::new("huge", 1, 1_000).with_size(Size::Huge);
        let roster = [(&rogue, Side::A), (&huge, Side::B)];
        let (mut fight, _) = fight_of(&roster, 5);
        fight.fighters[1]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(1)));
        let plan = fight.extra_damage_plan(0, 1, &melee, &[], RollMode::Advantage, true);
        assert_eq!(plan.cunning, None);
    }

    /// A weapon buff armed by landing its trigger condition: nothing until
    /// the rogue poisons someone, then extra dice on every weapon attack -
    /// and never on a spell attack.
    #[test]
    fn a_condition_triggered_weapon_buff_arms_when_the_condition_lands() {
        let rogue = cunning_rogue().with_rider(Rider::ConditionTriggeredWeaponDamage {
            trigger: Condition::Poisoned,
            dice_count: 1,
            dice_sides: 6,
            bonus: 0,
            damage_kind: DamageKind::Poison,
        });
        let target = Creature::new("target", 1, 1_000);
        let roster = [(&rogue, Side::A), (&target, Side::B)];
        let (mut fight, mut rng) = fight_of(&roster, 7);
        let m = bow(30, RollMode::Normal);
        let Effect::Strikes { strike, .. } = &m.effect else {
            unreachable!()
        };
        assert!(fight
            .extra_damage_plan(0, 1, strike, &[], RollMode::Normal, true)
            .rolls
            .is_empty());

        let mut landed = Vec::new();
        fight.land_condition(0, 1, Condition::Poisoned, Duration::VictimTurn, &mut landed);
        let buff = rogue
            .riders
            .iter()
            .position(|r| matches!(r, Rider::ConditionTriggeredWeaponDamage { .. }))
            .unwrap();
        assert!(fight.fighters[0].armed[buff]);
        let plan = fight.extra_damage_plan(0, 1, strike, &[], RollMode::Normal, true);
        assert_eq!(
            plan.rolls,
            vec![DamageRoll::new(1, 6, 0, DamageKind::Poison)]
        );

        let mut spell = strike.clone();
        spell.kind = AttackKind::RANGED_SPELL;
        assert!(fight
            .extra_damage_plan(0, 1, &spell, &[], RollMode::Normal, true)
            .rolls
            .is_empty());
        let _ = strike_once(&mut fight, &mut rng, 0, 1, &m);
    }

    /// One dose: the first weapon hit uses it up, forces the save, and on a
    /// failure leaves both the burden and the extra condition; the burdened
    /// save then really rolls at disadvantage.
    #[test]
    fn an_injury_poison_dose_is_used_up_by_the_first_weapon_hit() {
        let poisoner = Creature::new("poisoner", 15, 50).with_rider(Rider::InjuryPoison {
            ability: Ability::Con,
            dc: 99,
            debuffed_ability: Ability::Wis,
            condition: Some(Condition::Poisoned),
            duration: Duration::Rounds(10),
        });
        let target = Creature::new("target", 1, 1_000);
        let roster = [(&poisoner, Side::A), (&target, Side::B)];
        let (mut fight, mut rng) = fight_of(&roster, 9);
        let m = bow(30, RollMode::Normal);
        while fight.fighters[0].rider_uses[0] > 0 {
            strike_once(&mut fight, &mut rng, 0, 1, &m);
        }
        assert!(fight.fighters[1].has(|c| c == Condition::Poisoned));
        assert!(fight.fighters[1].has(|c| c == Condition::SaveDisadvantage(Ability::Wis)));
        assert_eq!(
            save_mode(&fight.fighters[1], Ability::Wis, false),
            RollMode::Disadvantage
        );
        assert_eq!(
            save_mode(&fight.fighters[1], Ability::Con, false),
            RollMode::Normal
        );
    }
}
