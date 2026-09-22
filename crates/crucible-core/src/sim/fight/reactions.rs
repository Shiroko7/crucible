//! Reactions: a creature's one per round, spent on an AC boost against an
//! incoming attack, on cutting a hit's damage, or on a move of its own when
//! its trigger happens.

use crate::creature::{AttackKind, ReactionTrigger, Rider, Strike};
use crate::prob::Rng;
use crate::rules::Condition;
use crate::sim::fight::{Answer, Fight, Fighter};

impl<'a> Fight<'a> {
    /// Take `me`'s first [`crate::creature::Reaction`] answering `trigger`,
    /// aimed at `at` alone - if `me` is up, can act, still has its reaction
    /// this round, and can reach `at`, and the move is one it can take right
    /// now.
    pub(super) fn react(
        &mut self,
        rng: &mut Rng,
        me: usize,
        trigger: ReactionTrigger,
        at: usize,
        record: bool,
        notes: &mut Vec<String>,
    ) {
        let f = &self.fighters[me];
        if !f.alive()
            || !f.reaction
            || f.incapacitated()
            || !self.fighters[at].alive()
            || !self.reaches(me, at)
        {
            return;
        }
        let creature = f.creature;
        // Only one that can land on `at` from where it stands: a bite waits
        // for a pull that brings its prey all the way to the mouth.
        let Some(i) = creature.reactions.iter().enumerate().position(|(i, r)| {
            r.trigger == trigger
                && f.reactions[i].available()
                && f.can_pay(r.action.cost)
                && f.can_cast(r.action.spell_slot_level)
                && self.can_take(me, &r.action)
                && self.move_lands(me, at, &r.action)
        }) else {
            return;
        };
        let m = &creature.reactions[i].action;
        self.fighters[me].pay(m.cost);
        self.fighters[me].cast_spell_slot(m.spell_slot_level);
        self.fighters[me].spend_reaction(i);
        if let Some(spend) = m.spends {
            self.spend(me, spend);
        }
        let previous = self.sole_target.replace(at);
        let mut line = String::new();
        self.apply(m, rng, me, at, record, &mut line);
        self.sole_target = previous;
        if record {
            notes.push(format!("{} reacts: {line}", creature.name));
        }
    }

    /// Answer the conditions `me` has just given enemies with the reaction
    /// waiting for one - [`ReactionTrigger::EnemyGains`] - the first that
    /// fires, since there is one reaction a round.
    pub(super) fn react_to_landed(
        &mut self,
        rng: &mut Rng,
        me: usize,
        landed: &[(usize, Condition)],
        record: bool,
        notes: &mut Vec<String>,
    ) {
        let side = self.fighters[me].side;
        for &(victim, condition) in landed {
            if self.fighters[victim].side != side {
                self.react(
                    rng,
                    me,
                    ReactionTrigger::EnemyGains(condition),
                    victim,
                    record,
                    notes,
                );
            }
        }
    }
}

/// The AC bonus a [`Rider::ReactionOnTargeted`] offers against an incoming
/// attack of `kind`, if this creature has one that answers it and a reaction
/// left to spend on it - never while Incapacitated - and whether spending it
/// leaves the bonus up until the start of its next turn.
pub(super) fn ac_boost_reaction(f: &Fighter<'_>, kind: AttackKind) -> Option<(i32, bool)> {
    if !f.reaction || f.incapacitated() {
        return None;
    }
    f.creature.riders.iter().find_map(|rider| match rider {
        Rider::ReactionOnTargeted {
            trigger,
            ac_bonus,
            lasting,
        } if trigger.answers(kind) => Some((*ac_bonus, *lasting)),
        _ => None,
    })
}

/// The AC bonus a reaction *already spent* is still holding up against an
/// attack of `kind` - a raised blade, a shield still glowing. Zero unless
/// this creature has spent a `lasting` [`Rider::ReactionOnTargeted`] since
/// the start of its last turn on something this attack answers.
///
/// Unlike [`ac_boost_reaction`] this costs nothing and is not a decision: it
/// is simply how hard this creature is to hit right now, so it is added to
/// its Armor Class rather than routed through the reaction machinery.
pub(super) fn standing_ac_boost(f: &Fighter<'_>, kind: AttackKind) -> i32 {
    match f.reactive_ac {
        Some((trigger, bonus)) if trigger.answers(kind) => bonus,
        _ => 0,
    }
}

/// Spend this creature's reaction to cut an incoming hit's damage, if it has
/// one left and a rider that answers this hit - the first in declaration
/// order: [`Rider::ReduceDamage`] against the damage types it names, or
/// [`Rider::HalveAttackDamage`] against anything.
pub(super) fn react_to_hit(
    rng: &mut Rng,
    f: &mut Fighter<'_>,
    strike: &Strike,
    damage: i32,
) -> (i32, Option<Answer>) {
    if damage <= 0 || !f.reaction || f.incapacitated() {
        return (damage, None);
    }
    let creature = f.creature;
    for rider in &creature.riders {
        match rider {
            Rider::ReduceDamage { kinds, roll } if strike.deals_any(kinds) => {
                f.spend_reaction_budget();
                let cut = roll.sample_raw(rng).min(damage);
                return (damage - cut, Some(Answer::Deflected(cut)));
            }
            Rider::HalveAttackDamage => {
                f.spend_reaction_budget();
                return (damage / 2, Some(Answer::Halved));
            }
            _ => {}
        }
    }
    (damage, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{AttackTrigger, Creature, Effect, Move};
    use crate::rules::{Condition, DamageKind, DamageRoll};
    use crate::sim::fight::fighter::refresh;
    use crate::sim::fight::test_support::{no_log, puncher};
    use crate::sim::fight::Expiry;
    use crate::sim::{run, Policy, Side};

    // Every damage rider, Cunning Strike choice, injury dose, weapon buff and
    // reaction exercised here existed and was unit-tested as a mechanism (in
    // `creature::rider`) before the fight loop actually used any of them.
    // These pin them to the loop.

    /// Dodge and a reaction that cuts damage both have to actually reduce what
    /// lands, and the stance has to expire on its own.
    #[test]
    fn dodging_and_a_damage_reducing_reaction_both_bite() {
        let attacker = {
            let mut c = puncher("attacker", 10, 10_000, 5, 0);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(5, vec![DamageRoll::new(2, 6, 4, DamageKind::Slashing)]),
                count: 3,
            };
            c.initiative = -100;
            c
        };
        let plain = {
            let mut c = Creature::new("plain", 15, 10_000);
            c.initiative = 100;
            c
        };
        let dodger = {
            let mut c = plain.clone();
            c.bonus_actions.push(Move::new(
                "Dodge",
                Effect::Stance {
                    condition: Condition::Dodging,
                },
            ));
            c
        };
        let deflector = {
            let mut c = plain.clone();
            c.riders.push(Rider::ReduceDamage {
                kinds: vec![DamageKind::Slashing],
                roll: DamageRoll::new(1, 10, 7, DamageKind::Slashing),
            });
            c
        };

        let taken = |defender: &Creature| {
            let mut rng = Rng::new(8);
            let mut total = 0i64;
            for _ in 0..400 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [defender, &attacker],
                    [Policy::Greedy; 2],
                    5,
                    &mut log,
                );
                total += o.damage_dealt[1];
            }
            total
        };

        let base = taken(&plain);
        assert!(taken(&dodger) < base, "dodging must reduce incoming damage");
        assert!(
            taken(&deflector) < base,
            "a damage-reducing reaction must reduce incoming damage"
        );
        // One reaction a round, so it cannot blunt all three attacks.
        assert!(taken(&deflector) > base / 2);
    }

    /// [`Rider::ReactionOnTargeted`] is the mirror of [`Rider::ReduceDamage`]:
    /// it acts before the hit is even decided, converting what would have
    /// been a hit into a miss instead of shaving damage off one that already
    /// landed.
    #[test]
    fn a_reactive_ac_boost_converts_a_would_be_hit_into_a_miss() {
        let attacker = {
            let mut c = puncher("attacker", 10, 10_000, 5, 0);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(5, vec![DamageRoll::new(2, 6, 4, DamageKind::Slashing)]),
                count: 3,
            };
            c.initiative = -100;
            c
        };
        let plain = {
            let mut c = Creature::new("plain", 15, 10_000);
            c.initiative = 100;
            c
        };
        let shielded = {
            let mut c = plain.clone();
            c.riders.push(Rider::ReactionOnTargeted {
                trigger: AttackTrigger::AnyAttack,
                // Large enough that, whenever the reaction fires, it always
                // succeeds in turning the hit into a miss.
                ac_bonus: 100,
                lasting: false,
            });
            c
        };

        let taken = |defender: &Creature| {
            let mut rng = Rng::new(8);
            let mut total = 0i64;
            for _ in 0..400 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [defender, &attacker],
                    [Policy::Greedy; 2],
                    5,
                    &mut log,
                );
                total += o.damage_dealt[1];
            }
            total
        };

        let base = taken(&plain);
        let with_reaction = taken(&shielded);
        assert!(
            with_reaction < base,
            "a reactive AC boost must reduce incoming damage: {with_reaction} vs {base}"
        );
        // One reaction a round protects at most one of the three attacks each
        // round, so it cannot blunt them all.
        assert!(with_reaction > base / 4);
    }

    /// The reaction in isolation: available at the start, gone the instant
    /// it is spent, and back only once the creature's turn refreshes it.
    #[test]
    fn a_reaction_on_targeted_is_available_once_then_spent_for_the_round() {
        let mut c = Creature::new("defender", 15, 20);
        c.riders.push(Rider::ReactionOnTargeted {
            trigger: AttackTrigger::AnyAttack,
            ac_bonus: 5,
            lasting: false,
        });
        let mut f = Fighter::new(&c, Side::A, Policy::Greedy, 0);

        assert_eq!(
            ac_boost_reaction(&f, AttackKind::MELEE_WEAPON),
            Some((5, false)),
            "the reaction should be available before anything spends it"
        );

        // Spend it exactly the way the strike-resolution loop does.
        f.reaction = false;
        assert_eq!(
            ac_boost_reaction(&f, AttackKind::MELEE_WEAPON),
            None,
            "spent this round, it must not be offered again"
        );

        // Refreshing at the start of a turn is the only thing that brings a
        // reaction back.
        let mut rng = Rng::new(1);
        refresh(&mut f, &mut rng);
        assert_eq!(
            ac_boost_reaction(&f, AttackKind::MELEE_WEAPON),
            Some((5, false)),
            "a new turn should refresh the reaction"
        );

        // And nothing is spent while Incapacitated.
        f.conditions
            .push((Condition::Stunned, Expiry::TurnStart(0)));
        assert_eq!(ac_boost_reaction(&f, AttackKind::MELEE_WEAPON), None);
    }

    /// A parry stays up: one reaction, but the Armor Class it buys answers
    /// every melee attack until the start of its holder's next turn - and
    /// nothing else. An arrow goes straight past it.
    #[test]
    fn a_lasting_reactive_boost_answers_every_melee_attack_until_its_next_turn() {
        let mut c = Creature::new("duelist", 19, 60);
        c.riders.push(Rider::ReactionOnTargeted {
            trigger: AttackTrigger::MeleeAttack,
            ac_bonus: 4,
            lasting: true,
        });
        let mut f = Fighter::new(&c, Side::A, Policy::Greedy, 0);

        assert_eq!(
            ac_boost_reaction(&f, AttackKind::MELEE_WEAPON),
            Some((4, true))
        );
        assert_eq!(
            ac_boost_reaction(&f, AttackKind::RANGED_WEAPON),
            None,
            "a parry answers a blade, not an arrow"
        );
        assert_eq!(standing_ac_boost(&f, AttackKind::MELEE_WEAPON), 0);

        // Spent the way the strike loop spends it.
        f.reaction = false;
        f.reactive_ac = Some((AttackTrigger::MeleeAttack, 4));
        assert_eq!(
            ac_boost_reaction(&f, AttackKind::MELEE_WEAPON),
            None,
            "the reaction itself is gone"
        );
        assert_eq!(
            standing_ac_boost(&f, AttackKind::MELEE_WEAPON),
            4,
            "but the blade is still raised"
        );
        assert_eq!(standing_ac_boost(&f, AttackKind::RANGED_SPELL), 0);

        // Its own turn takes the raised blade down with it.
        let mut rng = Rng::new(2);
        refresh(&mut f, &mut rng);
        assert_eq!(standing_ac_boost(&f, AttackKind::MELEE_WEAPON), 0);
        assert_eq!(
            ac_boost_reaction(&f, AttackKind::MELEE_WEAPON),
            Some((4, true))
        );
    }

    /// End to end: a duelist with a lasting parry takes clearly less from a
    /// multiattacking enemy than the same duelist without one, because the
    /// boost answers the second and third swing too.
    #[test]
    fn a_lasting_parry_blunts_a_whole_multiattack() {
        let attacker = {
            let mut c = puncher("attacker", 10, 10_000, 5, 0);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(8, vec![DamageRoll::new(2, 6, 4, DamageKind::Slashing)]),
                count: 3,
            };
            c.initiative = -100;
            c
        };
        let plain = {
            let mut c = Creature::new("plain", 16, 10_000);
            c.initiative = 100;
            c
        };
        let duelist = {
            let mut c = plain.clone();
            c.riders.push(Rider::ReactionOnTargeted {
                trigger: AttackTrigger::MeleeAttack,
                ac_bonus: 4,
                lasting: true,
            });
            c
        };
        let one_shot = {
            let mut c = plain.clone();
            c.riders.push(Rider::ReactionOnTargeted {
                trigger: AttackTrigger::MeleeAttack,
                ac_bonus: 4,
                lasting: false,
            });
            c
        };

        let taken = |defender: &Creature| {
            let mut rng = Rng::new(5);
            let mut total = 0i64;
            for _ in 0..400 {
                let mut log = no_log();
                let o = run(
                    &mut rng,
                    [defender, &attacker],
                    [Policy::Greedy; 2],
                    5,
                    &mut log,
                );
                total += o.damage_dealt[1];
            }
            total
        };
        let (base, once, lasting) = (taken(&plain), taken(&one_shot), taken(&duelist));
        assert!(
            once < base,
            "even one parried swing helps: {once} vs {base}"
        );
        assert!(
            lasting < once,
            "a parry that stays up should blunt the whole multiattack: {lasting} vs {once}"
        );
    }

    /// A titan whose stat block grants several reactions a round gets one
    /// per turn - its own and everybody else's - until the round's budget is
    /// gone, and the whole budget is back at the start of its own turn.
    #[test]
    fn several_reactions_a_round_is_still_one_a_turn() {
        let mut titan = Creature::new("titan", 19, 200);
        titan.reactions_per_round = 3;
        let mut f = Fighter::new(&titan, Side::B, Policy::Greedy, 0);
        let mut rng = Rng::new(1);

        for spent in 1..=3 {
            assert!(f.reaction, "reaction {spent} of the round");
            f.spend_reaction_budget();
            assert!(!f.reaction, "and only one inside a turn");
            f.offer_reaction();
        }
        assert!(
            !f.reaction,
            "three is three: the fourth turn of the round offers nothing"
        );

        refresh(&mut f, &mut rng);
        assert!(f.reaction, "its own turn brings the whole budget back");

        // Everything that has not said otherwise is unchanged: one reaction,
        // and no more until its own turn comes round.
        let plain = Creature::new("plain", 15, 30);
        assert_eq!(plain.reactions_per_round, 1);
        let mut f = Fighter::new(&plain, Side::A, Policy::Greedy, 1);
        f.spend_reaction_budget();
        f.offer_reaction();
        assert!(!f.reaction);
        refresh(&mut f, &mut rng);
        assert!(f.reaction);
    }

    /// In a live fight: a defender with three reactions a round cuts damage
    /// from three different attackers in the same round, where one with a
    /// single reaction cuts one.
    #[test]
    fn several_reactions_a_round_answer_several_attackers() {
        let attacker = |name: &str| {
            let mut c = puncher(name, 10, 200, 20, 0);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(20, vec![DamageRoll::new(2, 6, 10, DamageKind::Slashing)]),
                count: 1,
            };
            c.initiative = 50;
            c.team = 0;
            c
        };
        let (a, b, d) = (attacker("a"), attacker("b"), attacker("c"));
        let defender = |per_round: u32| {
            let mut c = Creature::new("titan", 10, 10_000);
            c.initiative = -50;
            c.team = 1;
            c.reactions_per_round = per_round;
            c.riders.push(Rider::ReduceDamage {
                kinds: vec![DamageKind::Slashing],
                roll: DamageRoll::new(1, 4, 100, DamageKind::Slashing),
            });
            c
        };

        // Counted over twenty rounds rather than one, because a natural 1
        // always misses and a swing that misses is nothing to react to.
        let deflections = |per_round: u32| {
            let target = defender(per_round);
            let mut rng = Rng::new(4);
            let mut total = 0;
            for _ in 0..20 {
                let mut log = Some(Vec::new());
                crate::sim::run_teams(
                    &mut rng,
                    &[&a, &b, &d, &target],
                    [Policy::Greedy; 2],
                    1,
                    crate::sim::Budget::default(),
                    &mut log,
                );
                let round = log
                    .unwrap()
                    .join(
                        "
",
                    )
                    .matches("deflected")
                    .count();
                assert!(round <= per_round as usize, "never more than the budget");
                total += round;
            }
            total
        };
        let (one, three) = (deflections(1), deflections(3));
        assert!(one <= 20, "one a round, twenty rounds");
        assert!(
            three > 2 * one,
            "three reactions should answer far more swings: {three} against {one}"
        );
    }

    /// A reaction limited to ranged weapon attacks answers exactly those:
    /// not a sword, and not a spell attack from range.
    #[test]
    fn a_ranged_weapon_reaction_ignores_melee_and_spell_attacks() {
        let mut c = Creature::new("defender", 15, 20);
        c.riders.push(Rider::ReactionOnTargeted {
            trigger: AttackTrigger::RangedWeaponAttack,
            ac_bonus: 5,
            lasting: false,
        });
        let f = Fighter::new(&c, Side::A, Policy::Greedy, 0);
        assert_eq!(
            ac_boost_reaction(&f, AttackKind::RANGED_WEAPON),
            Some((5, false))
        );
        assert_eq!(ac_boost_reaction(&f, AttackKind::MELEE_WEAPON), None);
        assert_eq!(ac_boost_reaction(&f, AttackKind::RANGED_SPELL), None);
    }

    /// A multiattack throws several attack rolls in one turn, but a reaction
    /// is still only spendable once: at most one of them should ever be the
    /// one it was spent on.
    #[test]
    fn the_reaction_only_converts_one_attack_per_round_even_in_a_multiattack() {
        let attacker = {
            // AC 1 and a +30 to hit: every roll but a natural 1 is an
            // ordinary hit against the base AC, so the reaction has every
            // chance to fire on each of the three swings if it could.
            let mut c = puncher("attacker", 10, 10_000, 30, 0);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(30, vec![DamageRoll::new(1, 4, 0, DamageKind::Bludgeoning)]),
                count: 3,
            };
            c.initiative = -100;
            c
        };
        let mut defender = Creature::new("defender", 1, 10_000);
        defender.initiative = 100;
        defender.riders.push(Rider::ReactionOnTargeted {
            trigger: AttackTrigger::AnyAttack,
            ac_bonus: 100,
            lasting: false,
        });

        let mut rng = Rng::new(3);
        let mut saw_a_boosted_miss = false;
        for _ in 0..200 {
            let mut log = Some(Vec::new());
            run(
                &mut rng,
                [&defender, &attacker],
                [Policy::Greedy; 2],
                1,
                &mut log,
            );
            let narration = log.unwrap().join("\n");
            let boosted = narration.matches("AC boosted").count();
            assert!(
                boosted <= 1,
                "one reaction a round should never convert more than one attack:\n{narration}"
            );
            saw_a_boosted_miss |= boosted == 1;
        }
        assert!(
            saw_a_boosted_miss,
            "an available reaction against a would-be hit should have fired at least once in 200 rounds"
        );
    }

    /// Uncanny Dodge halves one hit a round, and shares that one reaction
    /// with every other reaction the creature has.
    #[test]
    fn halving_an_attack_spends_the_one_reaction_a_round() {
        let rogue = Creature::new("rogue", 1, 1_000)
            .with_rider(Rider::HalveAttackDamage)
            .with_rider(Rider::ReactionOnTargeted {
                trigger: AttackTrigger::AnyAttack,
                ac_bonus: 100,
                lasting: false,
            });
        let mut f = Fighter::new(&rogue, Side::A, Policy::Greedy, 0);
        let strike = Strike::new(5, vec![DamageRoll::new(1, 6, 0, DamageKind::Slashing)]);
        let mut rng = Rng::new(1);
        assert_eq!(
            react_to_hit(&mut rng, &mut f, &strike, 21),
            (10, Some(Answer::Halved))
        );
        assert_eq!(react_to_hit(&mut rng, &mut f, &strike, 21), (21, None));
        assert_eq!(
            ac_boost_reaction(&f, AttackKind::MELEE_WEAPON),
            None,
            "the same reaction cannot also boost AC"
        );
        f.conditions
            .push((Condition::Stunned, Expiry::TurnStart(0)));
        refresh(&mut f, &mut rng);
        assert_eq!(
            react_to_hit(&mut rng, &mut f, &strike, 21),
            (21, None),
            "nothing while Incapacitated"
        );
    }

    /// A whirlpool (seat 0) that drags whoever starts a turn in it, and snaps
    /// at whoever it drags in - with its one reaction a round.
    fn whirlpool() -> Creature {
        let mut c = Creature::new("whirlpool", 10, 1_000);
        c.auras.push(Move::new(
            "Undertow",
            Effect::Save(crate::creature::SaveEffect {
                ability: crate::rules::Ability::Str,
                dc: 99,
                damage: vec![],
                half_on_success: false,
                on_failure: vec![(Condition::Pulled, crate::rules::Duration::ApplierTurn)],
                max_targets: None,
                requires_type: None,
            }),
        ));
        c.reactions.push(crate::creature::Reaction {
            trigger: ReactionTrigger::EnemyGains(Condition::Pulled),
            action: Move::new(
                "Snap",
                Effect::AutoHit {
                    damage: vec![DamageRoll::new(0, 1, 5, DamageKind::Piercing)],
                },
            ),
        });
        c
    }

    #[test]
    fn an_aura_pull_sets_off_a_snap_at_whoever_it_drags_in_once_a_round() {
        let pool = whirlpool();
        let a = Creature::new("a", 10, 100);
        let b = Creature::new("b", 10, 100);
        let roster = [(&pool, Side::B), (&a, Side::A), (&b, Side::A)];
        let (mut fight, mut rng) = crate::sim::fight::test_support::fight_of(&roster, 1);
        let mut log = Some(Vec::new());

        fight.take_turn(1, 1, &mut rng, &mut log, None);
        assert!(fight.fighters[1].has(|c| c == Condition::Pulled));
        assert_eq!(fight.fighters[1].hp, 95, "dragged in and bitten");
        fight.take_turn(1, 2, &mut rng, &mut log, None);
        assert!(fight.fighters[2].has(|c| c == Condition::Pulled));
        assert_eq!(fight.fighters[2].hp, 100, "the reaction is already spent");

        fight.take_turn(2, 0, &mut rng, &mut log, None);
        fight.take_turn(2, 2, &mut rng, &mut log, None);
        assert_eq!(fight.fighters[2].hp, 95, "back on the whirlpool's turn");
        let narration = log.unwrap().join("\n");
        assert!(narration.contains("(aura)"), "{narration}");
        assert!(narration.contains("whirlpool reacts: Snap"), "{narration}");
    }

    /// A reaction waits for its own trigger: a breach does not set off one
    /// waiting for a pull.
    #[test]
    fn a_reaction_waits_for_its_own_trigger() {
        let pool = whirlpool();
        let a = Creature::new("a", 10, 100);
        let roster = [(&pool, Side::B), (&a, Side::A)];
        let (mut fight, mut rng) = crate::sim::fight::test_support::fight_of(&roster, 1);
        fight.react(
            &mut rng,
            0,
            ReactionTrigger::Breached,
            1,
            false,
            &mut Vec::new(),
        );
        assert_eq!(fight.fighters[1].hp, 100);
        assert!(fight.fighters[0].reaction);
    }
}
