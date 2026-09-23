//! Swallowing: who is inside whom, what that cuts them off from, what it
//! costs them - digestion every turn, a squeeze on demand - and how they get
//! out: regurgitated, or freed when the swallower drops.

use crate::creature::Zone;
use crate::prob::Rng;
use crate::rules::{Condition, DamageRoll, Duration, Size};
use crate::sim::fight::saves::saving_throw;
use crate::sim::fight::threshold::reducer;
use crate::sim::fight::{Expiry, Fight};

impl<'a> Fight<'a> {
    /// Can `from` affect `to` at all - aim at it, catch it in an area, heal
    /// it?
    ///
    /// A swallowed creature has total cover against everything outside the
    /// creature that swallowed it, and nothing to aim at but that creature's
    /// insides. That includes the swallower's own attacks, which come from
    /// outside; only its digestion and a squeeze of its gullet
    /// ([`crate::creature::Effect::HarmSwallowed`]) reach inside. Otherwise
    /// everyone reaches everyone: there are no positions.
    pub(super) fn reaches(&self, from: usize, to: usize) -> bool {
        if from == to {
            return true;
        }
        // Banished is the stronger version of the same idea: not tucked
        // inside something reachable, but out of the fight altogether, so it
        // touches nothing and nothing touches it - not an attack, not an
        // area, not a heal.
        if self.fighters[from].has(|c| c == Condition::Banished)
            || self.fighters[to].has(|c| c == Condition::Banished)
        {
            return false;
        }
        match self.fighters[from].swallowed_by() {
            Some(holder) => to == holder,
            None => self.fighters[to].swallowed_by().is_none(),
        }
    }

    /// Everything `holder` has swallowed that is still alive.
    pub(super) fn held_by(&self, holder: usize) -> Vec<usize> {
        (0..self.fighters.len())
            .filter(|&i| {
                self.fighters[i].alive() && self.fighters[i].swallowed_by() == Some(holder)
            })
            .collect()
    }

    /// Swallow `victim` if it is `max_size` or smaller and not already inside
    /// something. Whether it is still conscious does not matter: a body
    /// inside cannot be reached by a healer outside.
    pub(super) fn swallow(&mut self, me: usize, victim: usize, max_size: Size) -> bool {
        let v = &self.fighters[victim];
        if victim == me || v.swallowed_by().is_some() || v.creature.size > max_size {
            return false;
        }
        self.fighters[victim].add_condition(Condition::Swallowed, Expiry::HeldBy(me));
        true
    }

    /// Let go of everything `holder` has swallowed, each landing Prone if
    /// `prone` - until it stands up on its own turn - and at its mouth, if it
    /// has one.
    pub(super) fn release_all(&mut self, holder: usize, prone: bool) -> Vec<usize> {
        let freed: Vec<usize> = (0..self.fighters.len())
            .filter(|&i| self.fighters[i].swallowed_by() == Some(holder))
            .collect();
        for &i in &freed {
            self.fighters[i]
                .conditions
                .retain(|&(_, expiry)| expiry != Expiry::HeldBy(holder));
            if self.has_mouth(holder) {
                self.set_zone(i, holder, Zone::Mouth);
            }
            if prone {
                let (conditions, _) =
                    self.conditions_against(holder, i, &[(Condition::Prone, Duration::VictimTurn)]);
                for (condition, duration) in conditions {
                    self.land_condition(holder, i, condition, duration, &mut Vec::new());
                }
            }
        }
        freed
    }

    /// `damage` to every creature `me` holds, with no roll - digestion, or a
    /// squeeze of its gullet.
    pub(super) fn harm_swallowed(
        &mut self,
        rng: &mut Rng,
        me: usize,
        damage: &[DamageRoll],
        record: bool,
        notes: &mut Vec<String>,
    ) {
        let attacker = self.fighters[me].creature;
        for i in self.held_by(me) {
            let against = self.fighters[i].creature;
            let defending = self.boon_resistances(i);
            let reduce = reducer(attacker, against, false, &defending);
            let raw: i32 = damage
                .iter()
                .map(|roll| roll.sample(rng, false, roll.reduction_against(&reduce)))
                .sum();
            let (dealt, _) = self.deal(rng, me, i, raw, false);
            if record {
                notes.push(format!("{} {dealt}", against.name));
            }
        }
    }

    /// The start of `who`'s turn: everything it holds takes its
    /// [`crate::creature::Rider::Digestion`].
    pub(super) fn digest(
        &mut self,
        round: u32,
        who: usize,
        rng: &mut Rng,
        log: &mut Option<Vec<String>>,
    ) {
        let creature = self.fighters[who].creature;
        let damage = creature.digestion();
        if damage.is_empty() || self.held_by(who).is_empty() {
            return;
        }
        let mut notes = Vec::new();
        self.harm_swallowed(rng, who, damage, log.is_some(), &mut notes);
        if let Some(l) = log.as_mut() {
            l.push(format!(
                "r{round} {}: digests ({})",
                creature.name,
                notes.join(", ")
            ));
        }
    }

    /// The end of a turn: a swallower that took enough damage from inside it
    /// during that turn saves, or regurgitates everything it holds, Prone.
    /// See [`crate::creature::Rider::Regurgitate`].
    pub(super) fn check_regurgitation(
        &mut self,
        round: u32,
        rng: &mut Rng,
        log: &mut Option<Vec<String>>,
    ) {
        for i in 0..self.fighters.len() {
            let taken = std::mem::take(&mut self.fighters[i].inside_damage);
            let Some((threshold, ability, dc)) = self.fighters[i].creature.regurgitation() else {
                continue;
            };
            if taken < threshold || !self.fighters[i].alive() {
                continue;
            }
            let (saved, resisted) = saving_throw(&mut self.fighters, rng, i, ability, dc, false);
            let freed = if saved {
                Vec::new()
            } else {
                self.release_all(i, true)
            };
            if let Some(l) = log.as_mut() {
                let name = &self.fighters[i].creature.name;
                l.push(if saved {
                    let how = if resisted {
                        "legendary resistance"
                    } else {
                        "a save"
                    };
                    format!(
                        "r{round} {name}: keeps its meal down after {taken} from inside ({how})"
                    )
                } else {
                    let names: Vec<&str> = freed
                        .iter()
                        .map(|&f| self.fighters[f].creature.name.as_str())
                        .collect();
                    format!(
                        "r{round} {name}: regurgitates {} after {taken} from inside",
                        names.join(", ")
                    )
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Creature, Effect, Move, Rider};
    use crate::rules::{Ability, DamageKind};
    use crate::sim::fight::test_support::{fight_of, no_log, puncher, strike_once};
    use crate::sim::fight::value::Boost;
    use crate::sim::Side;

    /// A worm (seat 0) against a hero (1) and the hero's friend (2).
    fn worm() -> Creature {
        let mut c = puncher("worm", 10, 500, 100, 0);
        c.actions[0].riders.push(Rider::Swallow {
            max_size: Size::Medium,
        });
        c.riders.push(Rider::Digestion {
            damage: vec![DamageRoll::new(0, 1, 10, DamageKind::Acid)],
        });
        c.riders.push(Rider::Regurgitate {
            threshold: 20,
            ability: Ability::Con,
            dc: 99,
        });
        c
    }

    fn party() -> (Creature, Creature) {
        (
            Creature::new("hero", 5, 100),
            Creature::new("friend", 5, 100),
        )
    }

    #[test]
    fn a_swallowed_creature_reaches_only_its_swallower_and_nothing_outside_reaches_it() {
        let w = worm();
        let (hero, friend) = party();
        let (mut fight, _) = fight_of(&[(&w, Side::B), (&hero, Side::A), (&friend, Side::A)], 1);
        assert!(fight.swallow(0, 1, Size::Medium));

        assert!(fight.reaches(1, 0), "the hero reaches the worm's insides");
        assert!(!fight.reaches(1, 2), "and nothing else");
        assert!(!fight.reaches(2, 1), "its friend cannot reach it");
        assert!(!fight.reaches(0, 1), "nor can the worm's own attacks");
        assert_eq!(fight.pick_target(1), Some(0));
        assert_eq!(
            fight.pick_target(0),
            Some(2),
            "the worm turns to the friend"
        );
        assert_eq!(fight.heal_target(2), 2, "no healing reaches inside");
        assert_eq!(fight.heal_target(1), 1, "but the hero can heal itself");
        assert_eq!(
            fight.caught(0, crate::creature::Reach::Any),
            vec![2],
            "an area catches nobody inside"
        );
    }

    #[test]
    fn only_a_target_small_enough_and_not_already_inside_is_swallowed() {
        let w = worm();
        let (mut hero, friend) = party();
        hero.size = Size::Large;
        let (mut fight, _) = fight_of(&[(&w, Side::B), (&hero, Side::A), (&friend, Side::A)], 1);
        assert!(!fight.swallow(0, 1, Size::Medium), "too big");
        assert!(fight.swallow(0, 2, Size::Medium));
        assert!(!fight.swallow(0, 2, Size::Medium), "already inside");
        assert_eq!(fight.fighters[2].swallowed_by(), Some(0));
    }

    #[test]
    fn a_swallowing_hit_takes_the_target_in() {
        let w = worm();
        let (hero, friend) = party();
        let (mut fight, mut rng) =
            fight_of(&[(&w, Side::B), (&hero, Side::A), (&friend, Side::A)], 3);
        let bite = w.actions[0].clone();
        // +100 against AC 5 misses only on a natural 1.
        for _ in 0..20 {
            strike_once(&mut fight, &mut rng, 0, 1, &bite);
            if fight.fighters[1].swallowed_by().is_some() {
                break;
            }
        }
        assert_eq!(fight.fighters[1].swallowed_by(), Some(0));
        assert!(fight.fighters[1].has(|c| c == Condition::Swallowed));
    }

    #[test]
    fn digestion_and_a_squeeze_hurt_only_whoever_is_inside() {
        let w = worm();
        let (hero, friend) = party();
        let (mut fight, mut rng) =
            fight_of(&[(&w, Side::B), (&hero, Side::A), (&friend, Side::A)], 1);
        fight.swallow(0, 1, Size::Medium);

        fight.digest(1, 0, &mut rng, &mut no_log());
        assert_eq!(fight.fighters[1].hp, 90);
        assert_eq!(fight.fighters[2].hp, 100);

        let squeeze = Move::new(
            "Squeeze",
            Effect::HarmSwallowed {
                damage: vec![DamageRoll::new(0, 1, 5, DamageKind::Bludgeoning)],
            },
        );
        assert!((fight.move_value(0, 2, &squeeze, Boost::default()) - 5.0).abs() < 1e-9);
        strike_once(&mut fight, &mut rng, 0, 2, &squeeze);
        assert_eq!(fight.fighters[1].hp, 85);
        assert_eq!(fight.fighters[2].hp, 100);
    }

    #[test]
    fn enough_damage_from_inside_in_one_turn_forces_a_regurgitation_save() {
        let w = worm();
        let (hero, friend) = party();
        let (mut fight, mut rng) =
            fight_of(&[(&w, Side::B), (&hero, Side::A), (&friend, Side::A)], 1);
        fight.swallow(0, 1, Size::Medium);

        fight.deal(&mut rng, 1, 0, 19, true);
        fight.check_regurgitation(1, &mut rng, &mut no_log());
        assert_eq!(
            fight.fighters[1].swallowed_by(),
            Some(0),
            "19 is not enough"
        );
        assert_eq!(
            fight.fighters[0].inside_damage, 0,
            "and the count starts over"
        );

        fight.deal(&mut rng, 1, 0, 12, true);
        fight.deal(&mut rng, 1, 0, 12, true);
        fight.check_regurgitation(1, &mut rng, &mut no_log());
        assert_eq!(fight.fighters[1].swallowed_by(), None, "24 is, at DC 99");
        assert!(fight.fighters[1].has(|c| c == Condition::Prone));
        assert!(!fight.fighters[1].has(|c| c == Condition::Swallowed));
    }

    #[test]
    fn a_swallower_that_drops_lets_go() {
        let w = worm();
        let (hero, friend) = party();
        let (mut fight, mut rng) =
            fight_of(&[(&w, Side::B), (&hero, Side::A), (&friend, Side::A)], 1);
        fight.swallow(0, 1, Size::Medium);
        fight.apply_damage(&mut rng, 0, 1_000);
        assert_eq!(fight.fighters[1].swallowed_by(), None);
        assert!(fight.reaches(2, 1));
    }
}
