//! Landing conditions and ending them: immunity (and downgrading it), each
//! condition's lifetime, and the Suppressed debuff halving its victim's
//! damage.

use crate::rules::{Condition, Duration};
use crate::sim::fight::{Expiry, Fight, Fighter};

impl<'a> Fight<'a> {
    /// Which of `conditions` `applier` can actually give `victim`, and
    /// whether the save against them is rolled with advantage.
    ///
    /// A condition the victim is immune to is dropped - unless the applier
    /// carries a [`crate::creature::Rider::DowngradeImmunity`] naming it, in
    /// which case it stays and the victim saves with advantage instead of
    /// shrugging it off.
    pub(super) fn conditions_against(
        &self,
        applier: usize,
        victim: usize,
        conditions: &[(Condition, Duration)],
    ) -> (Vec<(Condition, Duration)>, bool) {
        let attacker = self.fighters[applier].creature;
        let target = self.fighters[victim].creature;
        let mut advantage = false;
        let kept = conditions
            .iter()
            .copied()
            .filter(|&(c, _)| {
                if !target.immune_to_condition(c) {
                    return true;
                }
                let downgraded = attacker
                    .riders
                    .iter()
                    .any(|r| r.downgrades_condition_immunity(c));
                advantage |= downgraded;
                downgraded
            })
            .collect();
        (kept, advantage)
    }

    /// Give `victim` a condition `applier` inflicted, and arm any of
    /// `applier`'s weapon buffs that wait for exactly that condition to land
    /// on an enemy - see [`crate::creature::Rider::arms_on_condition`].
    pub(super) fn land_condition(
        &mut self,
        applier: usize,
        victim: usize,
        condition: Condition,
        duration: Duration,
        landed_conditions: &mut Vec<(usize, Condition)>,
    ) {
        let expiry = self.expiry(applier, victim, duration);
        self.apply_condition(victim, condition, expiry);
        landed_conditions.push((victim, condition));
        if self.fighters[applier].side != self.fighters[victim].side {
            let creature = self.fighters[applier].creature;
            for (i, rider) in creature.riders.iter().enumerate() {
                if rider.arms_on_condition(condition) {
                    self.fighters[applier].armed[i] = true;
                }
            }
        }
    }

    /// How a just-applied condition ends, once `duration` is pinned to the
    /// specific applier and victim that made it real.
    fn expiry(&self, applier: usize, victim: usize, duration: Duration) -> Expiry {
        match duration {
            Duration::ApplierTurn => Expiry::TurnStart(applier),
            Duration::VictimTurn => Expiry::TurnStart(victim),
            // Applied on the applier's own turn, "the end of your next turn"
            // is the second turn end from now; applied at any other moment,
            // the first.
            Duration::ApplierNextTurnEnd => Expiry::TurnEnd {
                who: applier,
                ends_left: if self.acting == Some(applier) { 2 } else { 1 },
            },
            Duration::Rounds(n) => Expiry::Rounds {
                who: applier,
                left: n.max(1),
            },
            Duration::SaveEndTurn { ability, dc } => Expiry::SaveEachTurn {
                victim,
                ability,
                dc,
            },
        }
    }

    /// Add `condition` to `victim`, then end their concentration if this
    /// takes away their turn. An Incapacitated creature cannot concentrate on
    /// anything, and 5e offers no save against losing it that way - unlike
    /// damage, which gets one.
    pub(super) fn apply_condition(&mut self, victim: usize, condition: Condition, expiry: Expiry) {
        self.fighters[victim].add_condition(condition, expiry);
        if condition.incapacitated() {
            self.end_concentration(victim);
        }
    }
}

/// This creature's own outgoing damage, halved if
/// [`Condition::halves_own_damage`] is active - the attacker-side
/// counterpart to a target's own [`crate::rules::Reduction`], which
/// only ever halves by the target's damage type. Applied last, after any
/// reaction that already cut the incoming damage - the same place a target's
/// own resistance sits at the end of `damage_pmf`'s pipeline - and rounds
/// down exactly like [`crate::rules::Reduction::Resistant`].
pub(super) fn halve_if_suppressed(fighters: &[Fighter<'_>], me: usize, dealt: i32) -> i32 {
    if fighters[me].has(Condition::halves_own_damage) {
        dealt / 2
    } else {
        dealt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Creature, Effect, Move, MoveKind, SaveEffect, Uses};
    use crate::prob::Rng;
    use crate::rules::Ability;
    use crate::sim::fight::test_support::{fight_of, no_log, puncher};
    use crate::sim::{run, Budget, Policy, Side};

    #[test]
    fn halve_if_suppressed_only_halves_while_active_and_rounds_down() {
        let creature = Creature::new("x", 10, 10);
        let mut fighters = vec![Fighter::new(&creature, Side::A, Policy::Greedy, 0)];
        assert_eq!(halve_if_suppressed(&fighters, 0, 7), 7);

        fighters[0]
            .conditions
            .push((Condition::Suppressed, Expiry::TurnStart(0)));
        assert_eq!(
            halve_if_suppressed(&fighters, 0, 7),
            3,
            "rounds down, like Reduction::Resistant"
        );
        assert_eq!(halve_if_suppressed(&fighters, 0, 0), 0);

        fighters[0].conditions.clear();
        assert_eq!(
            halve_if_suppressed(&fighters, 0, 7),
            7,
            "expiry restores full damage"
        );
    }

    /// End-to-end: the item's own forced save actually lands the bundled
    /// condition, which blocks the victim's own magic-item bonus action for
    /// exactly as long as `Duration::ApplierTurn` says - through the rest of
    /// the round it landed in, gone by the start of the applier's next turn.
    /// Fully deterministic: the save DC is unbeatable and the "should be
    /// blocked" move never rolls anything, so nothing here depends on the
    /// seed.
    #[test]
    fn a_failed_save_suppresses_the_targets_own_item_use_until_the_debuff_expires() {
        let mut caster = Creature::new("caster", 10, 20);
        caster.initiative = 100;
        caster.bonus_actions.push(
            Move::new(
                "Trinket",
                Effect::Save(SaveEffect {
                    ability: Ability::Con,
                    dc: 99, // a +0 Con save can never clear this
                    damage: Vec::new(),
                    half_on_success: false,
                    on_failure: vec![(Condition::Suppressed, Duration::ApplierTurn)],
                    max_targets: Some(1),
                    requires_type: None,
                }),
            )
            .with_uses(Uses::Limited(1))
            .with_kind(MoveKind::MagicItem),
        );

        let mut target = Creature::new("target", 10, 20);
        target.initiative = -100;
        target.bonus_actions.push(
            Move::new(
                "Warded Reflex",
                Effect::Stance {
                    condition: Condition::Dodging,
                },
            )
            .with_kind(MoveKind::MagicItem),
        );

        let mut rng = Rng::new(1);
        let mut log = Some(Vec::new());
        run(
            &mut rng,
            [&caster, &target],
            [Policy::Greedy; 2],
            2,
            &mut log,
        );
        let lines = log.unwrap();
        let narration = lines.join("\n");

        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("r1 ") && l.contains("suppressed")),
            "the item's own save should land the debuff in round 1:\n{narration}"
        );
        assert!(
            !lines
                .iter()
                .any(|l| l.starts_with("r1 ") && l.contains("Warded Reflex")),
            "the target's own magic item move must not fire while suppressed:\n{narration}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("r2 ") && l.contains("Warded Reflex")),
            "once the debuff expires the target's item use is available again:\n{narration}"
        );
    }

    /// If nothing attacks the marked creature first, the mark still goes
    /// away on its own - at the end of the caster's *next* turn
    /// ([`Duration::ApplierNextTurnEnd`]). Applied during the caster's turn,
    /// it survives that turn's end and the holder's own turn in between, so
    /// the caster can still cash it in themselves.
    #[test]
    fn a_mark_lasts_until_the_end_of_the_casters_next_turn_if_never_used() {
        let a = Creature::new("a", 10, 20);
        let b = Creature::new("b", 10, 20);
        let roster = [(&a, Side::A), (&b, Side::B)];
        let mut rng = Rng::new(12);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::Greedy; 2],
            5,
            Budget::default(),
            &mut log,
        );

        // Cast on the caster's own turn.
        fight.acting = Some(0);
        let mut landed = Vec::new();
        fight.land_condition(
            0,
            1,
            Condition::Marked,
            Duration::ApplierNextTurnEnd,
            &mut landed,
        );
        fight.end_of_turn(0);
        fight.acting = None;
        assert!(
            fight.fighters[1].has(|c| c == Condition::Marked),
            "the mark survives the end of the turn it was cast on"
        );

        // The holder's own turn in between does not clear it.
        fight.start_of_turn(1);
        fight.end_of_turn(1);
        assert!(fight.fighters[1].has(|c| c == Condition::Marked));

        // The end of the caster's next turn does.
        fight.start_of_turn(0);
        assert!(fight.fighters[1].has(|c| c == Condition::Marked));
        fight.end_of_turn(0);
        assert!(
            !fight.fighters[1].has(|c| c == Condition::Marked),
            "an unused mark should clear at the end of the caster's next turn"
        );
    }

    /// A fixed number of rounds, counted on the applier's turns.
    #[test]
    fn a_condition_for_n_rounds_lasts_exactly_that_long() {
        let a = Creature::new("a", 10, 20);
        let b = Creature::new("b", 10, 20);
        let roster = [(&a, Side::A), (&b, Side::B)];
        let (mut fight, _) = fight_of(&roster, 17);
        fight.acting = Some(0);
        let mut landed = Vec::new();
        fight.land_condition(
            0,
            1,
            Condition::Suppressed,
            Duration::Rounds(3),
            &mut landed,
        );
        fight.acting = None;
        for round in 1..=2 {
            fight.start_of_turn(0);
            assert!(
                fight.fighters[1].has(|c| c == Condition::Suppressed),
                "still there at the start of round {}",
                round + 1
            );
        }
        fight.start_of_turn(0);
        assert!(!fight.fighters[1].has(|c| c == Condition::Suppressed));
    }

    /// Something that lasts "until the start of your next turn" still ends
    /// then if the one who applied it has dropped in the meantime - a stun
    /// from a monk who goes down does not last forever.
    #[test]
    fn a_turn_start_condition_ends_even_if_its_applier_is_down() {
        let monk = puncher("monk", 10, 20, 5, 2);
        let dragon = puncher("dragon", 10, 200, 5, 2);
        let roster = [(&monk, Side::A), (&dragon, Side::B)];
        let (mut fight, mut rng) = fight_of(&roster, 19);
        fight.apply_condition(1, Condition::Stunned, Expiry::TurnStart(0));
        fight.fighters[0].hp = 0;
        fight.take_turn(2, 0, &mut rng, &mut None, None);
        assert!(!fight.fighters[1].has(|c| c == Condition::Stunned));
    }
}
