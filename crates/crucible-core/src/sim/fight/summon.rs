//! Summons and boons: calling a double up beside you, what it takes to
//! command one, and the lasting boons a creature puts on itself.
//!
//! Both are state a move can need before it is worth anything -
//! [`Requirement`] - and both are cleared by the same things that clear
//! everything else: a summon goes when its summoner does, a boon when its
//! duration runs out or the concentration holding it breaks.

use crate::creature::{Move, Requirement, Spend, Strike};
use crate::rules::{Condition, DamageKind, DamageRoll};
use crate::sim::fight::{Fight, Fighter};

impl<'a> Fight<'a> {
    /// How many of `me`'s `which`th summon are still standing.
    pub(super) fn summons_alive(&self, me: usize, which: usize) -> u32 {
        self.fighters
            .iter()
            .filter(|f| f.summoned_by == Some((me, which)) && f.alive())
            .count() as u32
    }

    /// Call up `me`'s `which`th summon beside it, returning its new roster
    /// seat - or `None` if there is no such summon declared.
    ///
    /// It joins the fight on its summoner's side and under its policy, but
    /// takes no turn of its own: the initiative order is untouched, and the
    /// only way it ever acts is its summoner spending a move on it (see
    /// [`Requirement::Summon`]). It starts wherever its summoner is standing,
    /// which is the only sense in which it has a position at all.
    pub(super) fn summon(&mut self, me: usize, which: usize) -> Option<usize> {
        let creature: &'a crate::creature::Creature =
            self.fighters[me].creature.summons.get(which)?;
        let seat = self.fighters.len();
        let mut summoned = Fighter::new(
            creature,
            self.fighters[me].side,
            self.fighters[me].policy,
            seat,
        );
        summoned.summoned_by = Some((me, which));
        // Everyone gains a place for the newcomer to stand in relative to
        // them, and it stands where its summoner does.
        let mut zones = self.fighters[me].zones.clone();
        let here = *zones.first().unwrap_or(&crate::creature::Zone::Range);
        for f in self.fighters.iter_mut() {
            let filler = f.zones.last().copied().unwrap_or(here);
            f.zones.push(filler);
        }
        zones.push(here);
        summoned.zones = zones;
        self.fighters.push(summoned);
        Some(seat)
    }

    /// Destroy every summon `holder` called up - what happens when it drops,
    /// the same way a swallower that drops lets go of everything it holds.
    pub(super) fn release_summons(&mut self, holder: usize) {
        for f in self.fighters.iter_mut() {
            if f.summoned_by.is_some_and(|(by, _)| by == holder) {
                f.hp = 0;
            }
        }
    }

    /// Destroy one of `me`'s `which`th summons - the one a move spends,
    /// letting it go in a burst. The nearest thing to a choice here is which
    /// one, and they are identical, so it takes the first still standing.
    pub(super) fn spend_summon(&mut self, me: usize, which: usize) {
        if let Some(f) = self
            .fighters
            .iter_mut()
            .find(|f| f.summoned_by == Some((me, which)) && f.alive())
        {
            f.hp = 0;
        }
    }

    /// Use up what a move spends - see [`Spend`].
    pub(super) fn spend(&mut self, me: usize, spend: Spend) {
        match spend {
            Spend::Summon(which) => self.spend_summon(me, which),
            Spend::Boon(which) => {
                self.fighters[me]
                    .conditions
                    .retain(|&(c, _)| !matches!(c, Condition::Boon(i) if usize::from(i) == which));
            }
        }
    }

    /// Does `me` hold the `which`th of its own boons right now?
    pub(super) fn has_boon(&self, me: usize, which: usize) -> bool {
        self.fighters[me].has(|c| matches!(c, Condition::Boon(i) if usize::from(i) == which))
    }

    /// Every damage type `who`'s active boons let it resist, on top of
    /// whatever its stat block already does.
    pub(super) fn boon_resistances(&self, who: usize) -> Vec<DamageKind> {
        let creature = self.fighters[who].creature;
        self.fighters[who]
            .conditions
            .iter()
            .filter_map(|&(c, _)| creature.boon(c))
            .flat_map(|boon| boon.resist.iter().copied())
            .collect()
    }

    /// The extra damage `me`'s active boons put on `strike` - a form that
    /// makes its bearer's blows land harder, an enchantment on the one blade
    /// this swing is made with.
    pub(super) fn boon_damage(&self, me: usize, strike: &Strike) -> Vec<DamageRoll> {
        let creature = self.fighters[me].creature;
        self.fighters[me]
            .conditions
            .iter()
            .filter_map(|&(c, _)| creature.boon(c))
            .filter_map(|boon| boon.damage_on(strike))
            .collect()
    }

    /// Is everything `m` needs to exist actually there - see
    /// [`Requirement`]? A move whose double is dead, or whose blade is no
    /// longer lit, is not a choice.
    pub(super) fn requirement_met(&self, me: usize, m: &Move) -> bool {
        match m.requires {
            None => true,
            Some(Requirement::Summon { which, count }) => self.summons_alive(me, which) >= count,
            Some(Requirement::SummonRoom { which, max }) => self.summons_alive(me, which) < max,
            Some(Requirement::Boon { which }) => self.has_boon(me, which),
        }
    }

    /// Everything `sim::fight` asks before letting `me` take `m` that does
    /// not depend on what it is aimed at: the conditions on it
    /// ([`Fighter::move_allowed`]) and what the move needs to exist
    /// ([`Fight::requirement_met`]).
    pub(super) fn can_take(&self, me: usize, m: &Move) -> bool {
        self.fighters[me].move_allowed(m) && self.requirement_met(me, m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Boon, Creature, Effect, Move};
    use crate::rules::Duration;
    use crate::sim::fight::test_support::{fight_of, puncher};
    use crate::sim::fight::Expiry;
    use crate::sim::Side;

    fn summoner() -> Creature {
        let mut c = puncher("summoner", 15, 40, 5, 2);
        let double = Creature::new("double", 12, 20);
        c.add_summon(double);
        c.add_boon(
            Boon::new("Lit Blade")
                .with_damage(DamageRoll::new(2, 8, 0, DamageKind::Radiant))
                .weapon_attacks_only(),
        );
        c.bonus_actions.push(
            Move::new("Call Double", Effect::Summon { which: 0 })
                .requiring(Requirement::SummonRoom { which: 0, max: 2 }),
        );
        c.bonus_actions.push(
            Move::new(
                "Command Double",
                Effect::Boon {
                    which: 0,
                    duration: Duration::Rounds(10),
                },
            )
            .requiring(Requirement::Summon { which: 0, count: 1 }),
        );
        c
    }

    /// A summon joins its summoner's side, takes no place in the initiative
    /// order, and everybody - itself included - gains somewhere to stand
    /// relative to it.
    #[test]
    fn a_summon_joins_the_side_but_not_the_order() {
        let me = summoner();
        let foe = puncher("foe", 12, 40, 5, 2);
        let roster = [(&me, Side::A), (&foe, Side::B)];
        let (mut fight, _rng) = fight_of(&roster, 4);
        let order_before = fight.order.clone();

        let seat = fight.summon(0, 0).expect("the summon is declared");
        assert_eq!(seat, 2);
        assert_eq!(fight.fighters[seat].side, Side::A);
        assert_eq!(fight.fighters[seat].summoned_by, Some((0, 0)));
        assert!(fight.fighters[seat].is_summon());
        assert_eq!(
            fight.order, order_before,
            "a summon never gets a turn of its own"
        );
        for f in &fight.fighters {
            assert_eq!(
                f.zones.len(),
                3,
                "everyone needs somewhere to stand relative to the newcomer"
            );
        }
        assert_eq!(fight.summons_alive(0, 0), 1);
    }

    /// What a move needs to exist is checked before it can be taken: no
    /// double, no command; a full house, no more summoning; an unlit blade,
    /// no burst.
    #[test]
    fn a_requirement_gates_the_move_that_needs_it() {
        let me = summoner();
        let foe = puncher("foe", 12, 40, 5, 2);
        let roster = [(&me, Side::A), (&foe, Side::B)];
        let (mut fight, _rng) = fight_of(&roster, 5);

        let call = &me.bonus_actions[0];
        let command = &me.bonus_actions[1];
        assert!(fight.can_take(0, call), "there is room for a first double");
        assert!(!fight.can_take(0, command), "nothing to command yet");

        fight.summon(0, 0);
        assert!(fight.can_take(0, command));
        fight.summon(0, 0);
        assert_eq!(fight.summons_alive(0, 0), 2);
        assert!(!fight.can_take(0, call), "two is the declared maximum");

        // Its summoner dropping takes them both with it.
        fight.release_summons(0);
        assert_eq!(fight.summons_alive(0, 0), 0);
        assert!(!fight.can_take(0, command));
        assert!(fight.can_take(0, call));
    }

    /// A double is kit, not a combatant: it can be destroyed without that
    /// being a death, its hit points are not part of how healthy its side
    /// finished, and a side down to nothing but doubles has lost.
    #[test]
    fn a_destroyed_double_is_not_a_death_and_cannot_hold_a_side_up() {
        let me = summoner();
        let foe = puncher("foe", 12, 40, 5, 2);
        let roster = [(&me, Side::A), (&foe, Side::B)];
        let (mut fight, _rng) = fight_of(&roster, 7);
        fight.summon(0, 0);

        let standing = fight.unresolved(1);
        assert_eq!(standing.survivors, [1, 1], "one combatant a side");
        assert_eq!(
            standing.hp_left[0], 40,
            "the double's hit points are not its summoner's"
        );

        fight.fighters[2].hp = 0;
        let lost = fight.unresolved(2);
        assert_eq!(lost.deaths, [0, 0], "a destroyed double is not a death");
        assert!(fight.finished(2).is_none(), "both sides are still standing");

        // With its summoner down, the side is finished however many doubles
        // were still up.
        fight.fighters[2].hp = 57;
        fight.fighters[0].hp = 0;
        let over = fight.finished(3).expect("the side has lost");
        assert_eq!(over.winner, Some(Side::B));
    }

    /// A boon is held as a condition naming it, so it answers both halves:
    /// what it adds to a swing, and what its holder shrugs off.
    #[test]
    fn a_held_boon_answers_for_damage_and_resistance() {
        let mut me = summoner();
        me.boons[0].resist = vec![DamageKind::Fire];
        let foe = puncher("foe", 12, 40, 5, 2);
        let roster = [(&me, Side::A), (&foe, Side::B)];
        let (mut fight, _rng) = fight_of(&roster, 6);

        let swing = Strike::new(7, vec![DamageRoll::new(1, 6, 4, DamageKind::Slashing)]);
        assert!(fight.boon_damage(0, &swing).is_empty());
        assert!(fight.boon_resistances(0).is_empty());
        assert!(!fight.has_boon(0, 0));

        fight.fighters[0]
            .conditions
            .push((Condition::Boon(0), Expiry::TurnStart(0)));
        assert!(fight.has_boon(0, 0));
        assert_eq!(
            fight.boon_damage(0, &swing),
            vec![DamageRoll::new(2, 8, 0, DamageKind::Radiant)]
        );
        assert_eq!(fight.boon_resistances(0), vec![DamageKind::Fire]);
    }
}
