//! Where enemies stand around a creature with a mouth, and what moves them:
//! its own tactic as it swims, each enemy's move on its turn, a pull, a push.
//! See [`crate::creature::Creature::mouth`] and [`Zone`].
//!
//! Only a creature with a mouth has enemies placed around it; everyone else
//! still reaches everyone, so a fight without one plays exactly as before.

use crate::creature::{AttackKind, Effect, Move, Reach, Tactic, Zone};
use crate::rules::Condition;
use crate::sim::fight::value::Boost;
use crate::sim::fight::{Fight, Slot};
use crate::sim::Policy;

impl<'a> Fight<'a> {
    /// Is `i` a creature its enemies stand around?
    pub(super) fn has_mouth(&self, i: usize) -> bool {
        self.fighters[i].creature.mouth
    }

    /// Where `who` stands around `anchor`.
    pub(super) fn zone(&self, who: usize, anchor: usize) -> Zone {
        self.fighters[who].zones[anchor]
    }

    pub(super) fn set_zone(&mut self, who: usize, anchor: usize, zone: Zone) {
        self.fighters[who].zones[anchor] = zone;
    }

    /// Everyone starts in `zone` around everyone else.
    pub(super) fn place_everyone(&mut self, zone: Zone) {
        let n = self.fighters.len();
        for f in &mut self.fighters {
            f.zones = vec![zone; n];
        }
    }

    fn opposed(&self, a: usize, b: usize) -> bool {
        self.fighters[a].side != self.fighters[b].side
    }

    /// Can `from` reach `to` with something reaching `reach` - an attack roll
    /// of `attack`'s kind, or an area or other effect when `None`?
    ///
    /// Total cover comes first (see [`Fight::reaches`]), and from inside a
    /// creature everything reaches it. Then, around a creature with a mouth:
    /// its own moves reach the zones their reach names, and an enemy's melee
    /// attack reaches it only from beside it. A ranged attack or an area
    /// reaches it from anywhere.
    pub(super) fn in_reach(
        &self,
        from: usize,
        to: usize,
        reach: Reach,
        attack: Option<AttackKind>,
    ) -> bool {
        if !self.reaches(from, to) {
            return false;
        }
        if self.fighters[from].swallowed_by() == Some(to) {
            return true;
        }
        if self.opposed(from, to) && self.has_mouth(from) {
            return reach.covers(self.zone(to, from));
        }
        if self.opposed(from, to) && self.has_mouth(to) {
            return match attack {
                Some(kind) if !kind.ranged => self.zone(from, to).in_melee(),
                _ => true,
            };
        }
        true
    }

    /// Does any part of `m` land on `target` - or need no target at all? A
    /// move that can do nothing from here is not a choice.
    pub(super) fn move_lands(&self, me: usize, target: usize, m: &Move) -> bool {
        self.effect_lands(me, target, &m.effect, m.reach)
    }

    fn effect_lands(&self, me: usize, target: usize, effect: &Effect, reach: Reach) -> bool {
        match effect {
            Effect::Strikes { strike, .. } => self.in_reach(me, target, reach, Some(strike.kind)),
            Effect::Save(_) | Effect::AutoHit { .. } => self.in_reach(me, target, reach, None),
            // Nothing in it needs a target - a Dash - or something in it lands.
            Effect::Sequence(parts) => {
                parts.is_empty()
                    || parts
                        .iter()
                        .any(|p| self.effect_lands(me, target, p, reach))
            }
            Effect::Part {
                effect, reach: own, ..
            } => self.effect_lands(me, target, effect, own.within(reach)),
            // A squeeze of an empty gullet lands on nobody.
            Effect::HarmSwallowed { .. } => !self.held_by(me).is_empty(),
            // A mark has to reach whoever it marks; a boon and a summon are
            // the user's own business, wherever anyone is standing.
            Effect::Afflict { .. } => self.in_reach(me, target, reach, None),
            Effect::Stance { .. }
            | Effect::Heal(_)
            | Effect::TempHp(_)
            | Effect::Buff { .. }
            | Effect::SaveOrModifier { .. }
            | Effect::Boon { .. }
            | Effect::Aura { .. }
            | Effect::Summon { .. } => true,
        }
    }

    /// The start of `me`'s turn, for a creature with a mouth: it swims by its
    /// tactic. Charging - alone or before running - it closes on the nearest
    /// enemy it can reach until that enemy is at its mouth. It is always the
    /// faster swimmer, so nothing stops it. Returns who it closed on, and
    /// from where.
    pub(super) fn close_in(&mut self, me: usize) -> Option<(usize, Zone)> {
        if !self.has_mouth(me) || self.fighters[me].creature.tactic == Tactic::Hold {
            return None;
        }
        let target = self.pick_target(me)?;
        let from = self.zone(target, me);
        if from == Zone::Mouth {
            return None;
        }
        self.set_zone(target, me, Zone::Mouth);
        Some((target, from))
    }

    /// After a hit-and-run creature's action: it swims clear, leaving every
    /// enemy it can reach [`Zone::Far`]. Returns whether it did.
    pub(super) fn withdraw(&mut self, me: usize) -> bool {
        if !self.has_mouth(me) || self.fighters[me].creature.tactic != Tactic::HitAndRun {
            return false;
        }
        for i in 0..self.fighters.len() {
            if self.opposed(me, i) && self.reaches(me, i) {
                self.set_zone(i, me, Zone::Far);
            }
        }
        true
    }

    /// Where `me` can get to around `anchor` on this turn: one move through
    /// difficult terrain, two otherwise, half that while
    /// [`Condition::Slowed`], and one fewer after standing up.
    pub(super) fn zone_options(&self, me: usize, anchor: usize) -> Vec<Zone> {
        let f = &self.fighters[me];
        let mut moves: u32 = if self.fighters[anchor].creature.difficult_terrain {
            1
        } else {
            2
        };
        if f.has(|c| c == Condition::Slowed) {
            moves /= 2;
        }
        moves = moves.saturating_sub(u32::from(f.stood_up));
        self.zone(me, anchor).within(moves)
    }

    /// Does `me` stand somewhere around `target` - an enemy with a mouth,
    /// which `me` is not inside?
    pub(super) fn zone_choice_applies(&self, me: usize, target: usize) -> bool {
        self.opposed(me, target)
            && self.has_mouth(target)
            && self.fighters[me].swallowed_by().is_none()
    }

    /// If `me` is going after a creature with a mouth, where it moves before
    /// acting; `None` when there is nowhere to move around.
    pub(super) fn zone_choice(&mut self, me: usize, target: usize) -> Option<Zone> {
        self.zone_choice_applies(me, target)
            .then(|| self.choose_zone(me, target))
    }

    /// Where a playstyle that does not search stands: wherever its best
    /// action is worth the most, and of equally good places the one furthest
    /// from the mouth. A defensive one that is bloodied puts distance first.
    ///
    /// A melee fighter therefore goes to the mouth whenever the mouth is
    /// where its blows land, and waits beside the body when it is sealed; an
    /// archer stands off in front, where it still sees into the mouth.
    fn choose_zone(&mut self, me: usize, anchor: usize) -> Zone {
        let here = self.zone(me, anchor);
        let f = &self.fighters[me];
        let cautious = f.policy == Policy::Defensive && f.bloodied();
        let mut best: Option<(Zone, f64, u32)> = None;
        for zone in self.zone_options(me, anchor) {
            self.set_zone(me, anchor, zone);
            let value = self.best_action_value(me, anchor);
            let safety = zone.distance();
            let better = best.is_none_or(|(_, v, s)| match cautious {
                true => (safety, value) > (s, v),
                false => (value, safety) > (v, s),
            });
            if better {
                best = Some((zone, value, safety));
            }
        }
        self.set_zone(me, anchor, here);
        best.map_or(here, |(zone, _, _)| zone)
    }

    /// The most any action `me` could take right now is worth against
    /// `target`, or minus infinity with nothing it can do.
    fn best_action_value(&self, me: usize, target: usize) -> f64 {
        let f = &self.fighters[me];
        Slot::Action
            .moves(f.creature)
            .iter()
            .zip(Slot::Action.states(f))
            .filter(|(m, state)| {
                state.available()
                    && f.can_pay(m.cost)
                    && f.can_cast(m.spell_slot_level)
                    && self.can_take(me, m)
                    && (m.is_free() || f.will_spend())
            })
            .map(|(m, _)| self.move_value(me, target, m, Boost::default()))
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// Move `me` toward `zone` around `target`, as far as it gets this turn.
    /// Returns where it came from and where it ended up, when it moved.
    pub(super) fn step_to(&mut self, me: usize, target: usize, zone: Zone) -> Option<(Zone, Zone)> {
        let from = self.zone(me, target);
        let to = self
            .zone_options(me, target)
            .into_iter()
            .min_by_key(|z| (*z != zone, z.distance().abs_diff(zone.distance())))?;
        if to == from {
            return None;
        }
        self.set_zone(me, target, to);
        Some((from, to))
    }

    /// A pull or a push, landed by a creature with a mouth on an enemy:
    /// [`Condition::Pulled`] drags it one move closer to the mouth,
    /// [`Condition::Pushed`] shoves it one move away.
    pub(super) fn shift(&mut self, applier: usize, victim: usize, condition: Condition) {
        if !self.opposed(applier, victim)
            || !self.has_mouth(applier)
            || self.fighters[victim].swallowed_by().is_some()
        {
            return;
        }
        let zone = self.zone(victim, applier);
        match condition {
            Condition::Pulled => self.set_zone(victim, applier, zone.pulled()),
            Condition::Pushed => self.set_zone(victim, applier, zone.pushed()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{
        Creature, Effect, Move, Reaction, ReactionTrigger, Rider, SaveEffect, Strike,
    };
    use crate::rules::{Ability, DamageKind, DamageRoll, Duration, Size};
    use crate::sim::fight::test_support::{fight_of, no_log, strike_once};
    use crate::sim::fight::Expiry;
    use crate::sim::Side;

    const MELEE: Option<AttackKind> = Some(AttackKind::MELEE_WEAPON);
    const RANGED: Option<AttackKind> = Some(AttackKind::RANGED_WEAPON);

    fn zap(name: &str, reach: Reach) -> Move {
        Move::new(
            name,
            Effect::AutoHit {
                damage: vec![DamageRoll::new(0, 1, 10, DamageKind::Force)],
            },
        )
        .with_reach(reach)
    }

    /// A maw (seat 0): a mouth, a shell nothing here breaches, and a bite that
    /// reaches only its mouth.
    fn maw() -> Creature {
        let mut c = Creature::new("maw", 15, 100_000);
        c.mouth = true;
        c.difficult_terrain = true;
        c.riders.push(Rider::DamageThreshold {
            threshold: 100,
            cracks: false,
            weak_spot_resists: vec![],
        });
        c.actions.push(zap("Bite", Reach::Mouth));
        c
    }

    fn hero(name: &str, ranged: bool) -> Creature {
        let kind = if ranged {
            AttackKind::RANGED_WEAPON
        } else {
            AttackKind::MELEE_WEAPON
        };
        Creature::new(name, 10, 1_000).with_action(Move::new(
            "Swing",
            Effect::Strikes {
                strike: Strike::new(10, vec![DamageRoll::new(1, 8, 10, DamageKind::Slashing)])
                    .with_kind(kind),
                count: 1,
            },
        ))
    }

    #[test]
    fn where_a_creature_stands_decides_what_reaches_it_and_what_it_reaches() {
        let m = maw();
        let h = hero("hero", false);
        let (mut fight, _) = fight_of(&[(&m, Side::B), (&h, Side::A)], 1);
        let reaches_hero = |fight: &Fight<'_>, reach| fight.in_reach(0, 1, reach, None);

        assert_eq!(fight.zone(1, 0), Zone::Range, "everyone starts at range");
        assert!(!reaches_hero(&fight, Reach::Mouth));
        assert!(!reaches_hero(&fight, Reach::Near));
        assert!(reaches_hero(&fight, Reach::Front));
        assert!(reaches_hero(&fight, Reach::Any));
        assert!(
            !fight.in_reach(1, 0, Reach::Any, MELEE),
            "a sword needs to be close"
        );
        assert!(fight.in_reach(1, 0, Reach::Any, RANGED), "a bow does not");

        fight.set_zone(1, 0, Zone::Body);
        assert!(!reaches_hero(&fight, Reach::Mouth));
        assert!(reaches_hero(&fight, Reach::Near));
        assert!(
            !reaches_hero(&fight, Reach::Front),
            "beside it, out of the cone"
        );
        assert!(fight.in_reach(1, 0, Reach::Any, MELEE));

        fight.set_zone(1, 0, Zone::Mouth);
        assert!(reaches_hero(&fight, Reach::Mouth));
        assert!(reaches_hero(&fight, Reach::Front));
    }

    #[test]
    fn the_mouth_is_open_to_a_blow_at_it_or_a_shot_from_in_front_until_sealed() {
        let m = maw();
        let h = hero("hero", false);
        let (mut fight, _) = fight_of(&[(&m, Side::B), (&h, Side::A)], 1);
        let open = |fight: &Fight<'_>, zone, attack| {
            let mut f = fight.clone();
            f.set_zone(1, 0, zone);
            f.through_weak_spot(1, 0, attack)
        };

        assert!(open(&fight, Zone::Mouth, MELEE));
        assert!(
            !open(&fight, Zone::Body, MELEE),
            "beside it there is only shell"
        );
        assert!(open(&fight, Zone::Range, RANGED));
        assert!(open(&fight, Zone::Far, RANGED));
        assert!(
            !open(&fight, Zone::Body, RANGED),
            "no line of sight from beside it"
        );
        assert!(
            !open(&fight, Zone::Mouth, None),
            "an area hits the creature"
        );

        fight.apply_condition(0, Condition::Sealed, Expiry::TurnStart(0));
        assert!(!open(&fight, Zone::Mouth, MELEE));
        assert!(!open(&fight, Zone::Range, RANGED));
    }

    /// Any attack it makes, or any legendary action it takes, opens a
    /// sealed mouth again.
    #[test]
    fn a_seal_lapses_when_its_holder_attacks_or_takes_a_legendary_action() {
        let mut m = maw();
        m.actions[0] = Move::new(
            "Snap",
            Effect::Strikes {
                strike: Strike::new(10, vec![DamageRoll::new(1, 4, 0, DamageKind::Piercing)]),
                count: 1,
            },
        );
        m.legendary.push(zap("Ripple", Reach::Any));
        m.legendary_uses = 1;
        let h = hero("hero", false);
        let (mut fight, mut rng) = fight_of(&[(&m, Side::B), (&h, Side::A)], 1);
        let sealed = |fight: &Fight<'_>| fight.fighters[0].has(|c| c == Condition::Sealed);

        fight.apply_condition(0, Condition::Sealed, Expiry::TurnStart(0));
        strike_once(&mut fight, &mut rng, 0, 1, &m.actions[0]);
        assert!(!sealed(&fight), "the bite opened it");

        fight.apply_condition(0, Condition::Sealed, Expiry::TurnStart(0));
        fight.legendary_windows(1, 1, &mut rng, &mut no_log());
        assert!(!sealed(&fight), "so did the legendary action");
    }

    #[test]
    fn a_charge_brings_the_nearest_enemy_to_the_mouth_and_holding_brings_nobody() {
        let mut m = maw();
        let (a, b) = (hero("a", false), hero("b", false));
        let (mut fight, _) = fight_of(&[(&m, Side::B), (&a, Side::A), (&b, Side::A)], 1);
        fight.set_zone(1, 0, Zone::Far);
        fight.set_zone(2, 0, Zone::Body);
        assert_eq!(fight.pick_target(0), Some(2), "the nearest first");
        assert_eq!(fight.close_in(0), Some((2, Zone::Body)));
        assert_eq!(fight.zone(2, 0), Zone::Mouth);
        assert_eq!(fight.zone(1, 0), Zone::Far, "nobody else moves");
        assert_eq!(fight.close_in(0), None, "already at the mouth");

        m.tactic = Tactic::Hold;
        let (mut fight, _) = fight_of(&[(&m, Side::B), (&a, Side::A)], 1);
        assert_eq!(fight.close_in(0), None);
        assert_eq!(fight.zone(1, 0), Zone::Range);
    }

    /// Hit and run leaves everyone far off - and the getaway takes the bonus
    /// action, so it cannot seal as well.
    #[test]
    fn hit_and_run_leaves_everyone_far_off_and_spends_the_bonus_action() {
        let mut m = maw();
        m.tactic = Tactic::HitAndRun;
        m.initiative = 100;
        m.bonus_actions.push(Move::new(
            "Clamp",
            Effect::Stance {
                condition: Condition::Sealed,
            },
        ));
        let (a, b) = (hero("a", false), hero("b", true));
        let (mut fight, mut rng) = fight_of(&[(&m, Side::B), (&a, Side::A), (&b, Side::A)], 1);
        let mut log = Some(Vec::new());
        fight.take_turn(1, 0, &mut rng, &mut log, None);

        assert_eq!(fight.zone(1, 0), Zone::Far);
        assert_eq!(fight.zone(2, 0), Zone::Far);
        assert!(!fight.fighters[0].has(|c| c == Condition::Sealed));
        let narration = log.unwrap().join("\n");
        assert!(narration.contains("closes on a from range"), "{narration}");
        assert!(narration.contains("withdraws"), "{narration}");
    }

    #[test]
    fn a_pull_drags_one_place_in_and_a_push_throws_one_place_out() {
        let m = maw();
        let h = hero("hero", false);
        let (mut fight, _) = fight_of(&[(&m, Side::B), (&h, Side::A)], 1);
        let mut landed = Vec::new();
        let mut land = |fight: &mut Fight<'_>, c| {
            fight.land_condition(0, 1, c, Duration::ApplierTurn, &mut landed);
            fight.zone(1, 0)
        };
        assert_eq!(land(&mut fight, Condition::Pulled), Zone::Mouth);
        assert_eq!(land(&mut fight, Condition::Pushed), Zone::Range);
        assert_eq!(land(&mut fight, Condition::Pushed), Zone::Far);
        assert_eq!(land(&mut fight, Condition::Pulled), Zone::Range);
    }

    /// A snap at whoever is dragged in waits for a pull that ends at the
    /// mouth; one that only drags a creature closer spends nothing.
    #[test]
    fn a_snap_waits_for_a_pull_that_ends_at_the_mouth() {
        let mut m = maw();
        m.reactions.push(Reaction {
            trigger: ReactionTrigger::EnemyGains(Condition::Pulled),
            action: zap("Snap", Reach::Mouth),
        });
        let undertow = Move::new(
            "Undertow",
            Effect::Save(SaveEffect {
                ability: Ability::Str,
                dc: 99,
                damage: vec![],
                half_on_success: false,
                on_failure: vec![(Condition::Pulled, Duration::ApplierTurn)],
                max_targets: None,
                requires_type: None,
            }),
        );
        let h = hero("hero", false);
        let (mut fight, mut rng) = fight_of(&[(&m, Side::B), (&h, Side::A)], 1);
        fight.set_zone(1, 0, Zone::Far);

        fight.apply(&undertow, &mut rng, 0, 1, false, &mut String::new());
        assert_eq!(fight.zone(1, 0), Zone::Range);
        assert_eq!(fight.fighters[1].hp, 1_000, "still out of the bite's reach");
        assert!(fight.fighters[0].reaction, "and the reaction is kept");

        fight.apply(&undertow, &mut rng, 0, 1, false, &mut String::new());
        assert_eq!(fight.zone(1, 0), Zone::Mouth);
        assert_eq!(fight.fighters[1].hp, 990, "snapped at");
    }

    #[test]
    fn difficult_terrain_standing_up_and_slowness_each_cost_moves() {
        let mut m = maw();
        let h = hero("hero", false);
        let (mut fight, _) = fight_of(&[(&m, Side::B), (&h, Side::A)], 1);
        fight.set_zone(1, 0, Zone::Far);
        assert_eq!(
            fight.zone_options(1, 0),
            vec![Zone::Range, Zone::Far],
            "one move a turn through difficult terrain"
        );
        fight.fighters[1].stood_up = true;
        assert_eq!(
            fight.zone_options(1, 0),
            vec![Zone::Far],
            "getting up took it"
        );

        m.difficult_terrain = false;
        let (mut fight, _) = fight_of(&[(&m, Side::B), (&h, Side::A)], 1);
        fight.set_zone(1, 0, Zone::Far);
        assert_eq!(
            fight.zone_options(1, 0).len(),
            4,
            "two moves reach anywhere"
        );
        fight.apply_condition(1, Condition::Slowed, Expiry::TurnStart(0));
        assert_eq!(fight.zone_options(1, 0), vec![Zone::Range, Zone::Far]);
    }

    /// A sword goes to the mouth while it is open and waits beside the body
    /// while it is sealed; a bow stands off as far as it can while still in
    /// front; and a bloodied defensive fighter backs away.
    #[test]
    fn each_fighter_stands_where_its_best_blow_is_worth_most() {
        let m = maw();
        let (sword, bow) = (hero("sword", false), hero("bow", true));
        let (mut fight, _) = fight_of(&[(&m, Side::B), (&sword, Side::A), (&bow, Side::A)], 1);

        assert_eq!(fight.zone_choice(1, 0), Some(Zone::Mouth));
        assert_eq!(fight.zone_choice(2, 0), Some(Zone::Far));
        assert_eq!(fight.zone(1, 0), Zone::Range, "choosing is not moving");

        fight.apply_condition(0, Condition::Sealed, Expiry::TurnStart(0));
        assert_eq!(fight.zone_choice(1, 0), Some(Zone::Body));

        fight.fighters[0].conditions.clear();
        fight.fighters[1].policy = Policy::Defensive;
        fight.fighters[1].hp = 100;
        assert_eq!(fight.zone_choice(1, 0), Some(Zone::Far));
    }

    #[test]
    fn a_regurgitated_creature_lands_at_the_mouth() {
        let m = maw();
        let h = hero("hero", false);
        let (mut fight, _) = fight_of(&[(&m, Side::B), (&h, Side::A)], 1);
        fight.set_zone(1, 0, Zone::Far);
        assert!(fight.swallow(0, 1, Size::Huge));
        fight.release_all(0, true);
        assert_eq!(fight.zone(1, 0), Zone::Mouth);
    }
}
