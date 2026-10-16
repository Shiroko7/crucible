//! Reactions: a creature's one per round, spent on an AC boost against an
//! incoming attack, on cutting a hit's damage, or on a move of its own when
//! its trigger happens.

use crate::creature::{AttackKind, Effect, Move, MoveKind, Reach, ReactionTrigger, Rider, Strike};
use crate::prob::Rng;
use crate::rules::Condition;
use crate::sim::fight::record::FrameKind;
use crate::sim::fight::{Answer, Expiry, Fight, Fighter};

/// How far a reaction that stops a spell reaches when it does not say.
const COUNTER_RANGE: u32 = 60;

/// The first part of an effect that is aimed at somebody to hurt them.
pub(super) fn first_harm(effect: &Effect) -> Option<&Effect> {
    match effect {
        Effect::Strikes { .. } | Effect::Save(_) | Effect::AutoHit { .. } | Effect::Displace(_) => {
            Some(effect)
        }
        Effect::Sequence(parts) => parts.iter().find_map(first_harm),
        Effect::Part { effect, .. } => first_harm(effect),
        _ => None,
    }
}

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
        self.react_matching(rng, me, |t| t == trigger, at, record, notes);
    }

    /// Answer an attack that has just hit `me`, landed by `attacker`: the
    /// first [`ReactionTrigger::Hit`] reaction `me` has that answers an
    /// attack of this `kind`.
    ///
    /// Takes the attack's kind rather than an exact trigger because the
    /// reaction is the side that says which blows it answers - "hits you
    /// with a melee attack" - exactly as a raised shield does; see
    /// [`ReactionTrigger::Hit`].
    #[allow(clippy::too_many_arguments)]
    pub(super) fn react_to_being_hit(
        &mut self,
        rng: &mut Rng,
        me: usize,
        attacker: usize,
        kind: AttackKind,
        hurt: bool,
        record: bool,
        notes: &mut Vec<String>,
    ) {
        if !self.fighters[me].creature.reactions.iter().any(|r| {
            matches!(
                r.trigger,
                ReactionTrigger::Hit(_) | ReactionTrigger::Damaged
            )
        }) {
            return;
        }
        self.react_matching(
            rng,
            me,
            |t| match t {
                ReactionTrigger::Hit(trigger) => trigger.answers(kind),
                // A hit that got nothing through is not damage.
                ReactionTrigger::Damaged => hurt,
                _ => false,
            },
            attacker,
            record,
            notes,
        );
    }

    /// The body both of the above share: take `me`'s first reaction whose
    /// trigger `answers`, aimed at `at` alone.
    fn react_matching(
        &mut self,
        rng: &mut Rng,
        me: usize,
        answers: impl Fn(ReactionTrigger) -> bool,
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
            answers(r.trigger)
                && f.reactions[i].available()
                && f.affords(&r.action)
                && f.can_cast(r.action.spell_slot_level)
                && self.can_take(me, &r.action)
                && self.move_lands(me, at, &r.action)
        }) else {
            return;
        };
        let m = &creature.reactions[i].action;
        self.fighters[me].charge(m);
        self.fighters[me].cast_spell_slot(m.spell_slot_level);
        self.fighters[me].spend_reaction(i);
        if let Some(spend) = m.spends {
            self.spend(me, spend);
        }
        let previous = self.sole_target.replace(at);
        let mut line = String::new();
        let slot = std::mem::replace(&mut self.cue.slot, "reaction");
        self.apply(m, rng, me, at, record, &mut line);
        self.cue.slot = slot;
        self.sole_target = previous;
        if record {
            notes.push(format!("{} reacts: {line}", creature.name));
        }
    }

    /// Answer the conditions `me` has just given enemies with the reaction
    /// waiting for one - [`ReactionTrigger::EnemyGains`] - the first that
    /// fires, since there is one reaction a round.
    /// `caster` is casting `m` at `target`: does anybody on the other side
    /// stop it? Each creature with a reaction for that - see
    /// [`ReactionTrigger::Cast`] - and the means to take it gets its chance,
    /// in roster order, until one of them works.
    pub(super) fn countered(
        &mut self,
        rng: &mut Rng,
        caster: usize,
        target: usize,
        m: &Move,
        record: bool,
        line: &mut String,
    ) -> bool {
        if m.kind != MoveKind::Spell {
            return false;
        }
        let side = self.fighters[caster].side;
        // Aimed at one creature: an attack roll, or a save only one makes.
        // A weapon attack made as part of a spell is not one of them: that
        // spell is cast on its caster, and what flies at the target is the
        // weapon.
        let single = match first_harm(&m.effect) {
            Some(Effect::Save(save)) => save.max_targets == Some(1),
            Some(Effect::Strikes { strike, .. }) => !(strike.kind.weapon && strike.kind.spell),
            Some(_) => true,
            None => false,
        };
        for e in 0..self.fighters.len() {
            let f = &self.fighters[e];
            if f.side == side
                || !f.alive()
                || !f.reaction
                || f.incapacitated()
                || !self.reaches(e, caster)
            {
                continue;
            }
            let creature = f.creature;
            let Some(i) = creature.reactions.iter().enumerate().position(|(i, r)| {
                let ReactionTrigger::Cast { aimed } = r.trigger else {
                    return false;
                };
                let costly = r.action.cost.is_some() || r.action.spell_slot_level.is_some();
                (!aimed || (single && target == e))
                    && (!costly || m.spell_slot_level.is_some())
                    && f.reactions[i].available()
                    && f.affords(&r.action)
                    && f.can_cast(r.action.spell_slot_level)
                    && self.can_take(e, &r.action)
                    && self.within(e, caster, r.action.range.unwrap_or(COUNTER_RANGE))
            }) else {
                continue;
            };
            let answer = &creature.reactions[i].action;
            self.fighters[e].charge(answer);
            self.fighters[e].cast_spell_slot(answer.spell_slot_level);
            self.fighters[e].spend_reaction(i);
            // What is stopped outright is drunk in, by anything that keeps
            // spells - see `Rider::SpellStore`.
            if let (Effect::Counter, Some((pool, _, holds))) =
                (&answer.effect, self.fighters[e].spell_store())
            {
                let stored = &mut self.fighters[e].resources[pool];
                *stored = (*stored + m.spell_slot_level.unwrap_or(0)).min(holds);
            }
            // A save is the caster's, to keep hold of its spell; anything
            // else simply stops it.
            let stopped = match &answer.effect {
                Effect::Save(save) => {
                    !self
                        .saving_throw(rng, caster, save.ability, save.dc, false)
                        .0
                }
                _ => true,
            };
            let how = if stopped { "stops" } else { "fails to stop" };
            let (stopper, casting) = (&creature.name, &self.fighters[caster].creature.name);
            let text = format!("{stopper}'s {} {how} {casting}'s {}", answer.name, m.name);
            if record {
                if !line.is_empty() {
                    line.push_str(" | ");
                }
                line.push_str(&format!("{} ({text})", m.name));
            }
            self.beat(FrameKind::Other, Some(e), Some(caster), text, None);
            if stopped {
                return true;
            }
        }
        false
    }

    /// `me` has just dealt `victim` `dealt` damage, made up of `kinds` as it
    /// was rolled: whatever of it `victim` throws back - see
    /// [`Rider::ReflectDamage`] - `me` takes, as that same type.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn reflect(
        &mut self,
        rng: &mut Rng,
        me: usize,
        victim: usize,
        dealt: i32,
        kinds: &[(crate::rules::DamageKind, i32)],
        record: bool,
        notes: &mut Vec<String>,
    ) {
        if dealt <= 0 || me == victim {
            return;
        }
        let Some(reflected) = self.fighters[victim]
            .creature
            .riders
            .iter()
            .find_map(|r| match r {
                Rider::ReflectDamage { kinds } => Some(kinds),
                _ => None,
            })
        else {
            return;
        };
        let rolled: i32 = kinds.iter().map(|&(_, n)| n).sum();
        let Some(&(kind, _)) = kinds.iter().find(|(k, n)| reflected.contains(k) && *n > 0) else {
            return;
        };
        let matching: i32 = kinds
            .iter()
            .filter(|(k, _)| reflected.contains(k))
            .map(|&(_, n)| n)
            .sum();
        // What got through of it, when something - a ward, a shell, a
        // reaction - took a share of the whole.
        let back = (i64::from(matching) * i64::from(dealt) / i64::from(rolled.max(1))) as i32;
        let hurt = self.fighters[me].creature.reduction(kind).apply(back);
        if hurt <= 0 {
            return;
        }
        self.fighters[victim].dealt += i64::from(hurt);
        self.apply_damage(rng, me, hurt);
        if record {
            notes.push(format!("{hurt} thrown back"));
        }
    }

    /// `me` has just hurt `victim` with a move carrying `riders`: if it is
    /// dead of it and the move raises what it kills, that is noted for the
    /// start of `me`'s next turn. Dead, not down: a player character that
    /// has only dropped is still its own.
    pub(super) fn slain(&mut self, me: usize, victim: usize, riders: &[Rider]) {
        let Some(which) = riders.iter().find_map(|r| match *r {
            Rider::RaiseSlain { which } => Some(which),
            _ => None,
        }) else {
            return;
        };
        let f = &self.fighters[victim];
        let gone = !f.alive() && (f.dead || !f.creature.player_character);
        if gone
            && !f.is_summon()
            && f.creature.is_creature_type("humanoid")
            && !self.fighters[me]
                .raising
                .iter()
                .any(|&(_, corpse)| corpse == victim)
        {
            self.fighters[me].raising.push((which, victim));
        }
    }

    /// What `me` killed and can raise gets up, where it fell, with a turn of
    /// its own from here on. The start of `me`'s turn.
    pub(super) fn raise(&mut self, me: usize, log: &mut Option<Vec<String>>) {
        for (which, corpse) in std::mem::take(&mut self.fighters[me].raising) {
            let Some(seat) = self.summon(me, which) else {
                continue;
            };
            self.place_beside(seat, corpse);
            self.order.push(seat);
            let text = format!(
                "{} rises as {}'s {}",
                self.fighters[corpse].creature.name,
                self.fighters[me].creature.name,
                self.fighters[seat].creature.name
            );
            if let Some(l) = log.as_mut() {
                l.push(format!("r{} {text}", self.round));
            }
            self.beat(FrameKind::Other, Some(seat), Some(corpse), text, None);
        }
    }

    /// `me` is about to land `conditions` on `victim` with a move carrying
    /// `riders`: one that is borne by one creature at a time is lifted from
    /// whoever had it, and one that can be broken out of leaves its price
    /// with whoever it holds.
    pub(super) fn hold(
        &mut self,
        me: usize,
        victim: usize,
        riders: &[Rider],
        conditions: &[(Condition, crate::rules::Duration)],
    ) {
        for rider in riders {
            match *rider {
                Rider::OneAtATime => {
                    for (i, f) in self.fighters.iter_mut().enumerate() {
                        if i == victim {
                            continue;
                        }
                        f.conditions.retain(|&(c, expiry)| {
                            let mine = matches!(
                                expiry,
                                Expiry::TurnStart(who)
                                    | Expiry::TurnEnd { who, .. }
                                    | Expiry::Rounds { who, .. }
                                    | Expiry::RoundsOrDamaged { who, .. } if who == me
                            );
                            !(mine && conditions.iter().any(|&(landed, _)| landed == c))
                        });
                    }
                }
                Rider::EscapeFor { roll } => self.fighters[victim].escape = Some((me, roll)),
                _ => {}
            }
        }
    }

    /// Whoever `me` is about to swing at may turn the blow on one of `me`'s
    /// own side - see [`Rider::RedirectAttacks`]. Who the swing lands on.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn redirect(
        &mut self,
        rng: &mut Rng,
        me: usize,
        target: usize,
        reach: Reach,
        kind: AttackKind,
        record: bool,
        notes: &mut Vec<String>,
    ) -> usize {
        // Whoever of its own side it can reach instead: the one closest to
        // falling, which is who anybody choosing would choose.
        let instead = |fight: &Self| {
            let side = fight.fighters[me].side;
            (0..fight.fighters.len())
                .filter(|&i| {
                    i != me
                        && fight.fighters[i].side == side
                        && fight.fighters[i].alive()
                        && fight.in_reach(me, i, reach, Some(kind))
                })
                .min_by_key(|&i| fight.fighters[i].hp)
        };
        if self.fighters[me].turned_from != Some(target) {
            let holder = &self.fighters[target];
            let Some((ability, dc)) = holder.creature.riders.iter().find_map(|r| match *r {
                Rider::RedirectAttacks { ability, dc } => Some((ability, dc)),
                _ => None,
            }) else {
                return target;
            };
            // Not twice a turn, not while it cannot act, and not with nobody
            // there to turn the blow on.
            if holder.redirect_spent || holder.incapacitated() || instead(self).is_none() {
                return target;
            }
            self.fighters[target].redirect_spent = true;
            let (saved, _) = self.saving_throw(rng, me, ability, dc, false);
            if saved {
                return target;
            }
            self.fighters[me].turned_from = Some(target);
        }
        match instead(self) {
            Some(other) => {
                if record {
                    notes.push(format!("turned on {}", self.fighters[other].creature.name));
                }
                if self.recording() {
                    let name = |i: usize| self.fighters[i].creature.name.clone();
                    let text = format!(
                        "{} turns {}'s attack on {}",
                        name(target),
                        name(me),
                        name(other)
                    );
                    self.beat(FrameKind::Other, Some(target), Some(me), text, None);
                }
                other
            }
            None => target,
        }
    }

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
    use crate::sim::fight::test_support::{fight_of, no_log, puncher};
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

    /// A storm priest answering whoever hits it: a creature whose reaction
    /// waits for a melee hit ([`ReactionTrigger::Hit`]).
    fn storm_priest(uses: crate::creature::Uses) -> Creature {
        let mut c = Creature::new("priest", 18, 60);
        c.reactions.push(crate::creature::Reaction {
            trigger: ReactionTrigger::Hit(AttackTrigger::MeleeAttack),
            action: Move::new(
                "Wrath",
                Effect::Save(crate::creature::SaveEffect {
                    ability: crate::rules::Ability::Dex,
                    // Unbeatable, so what the test measures is when the
                    // reaction fires rather than how a d20 fell.
                    dc: 99,
                    damage: vec![DamageRoll::new(0, 1, 10, DamageKind::Lightning)],
                    half_on_success: true,
                    on_failure: Vec::new(),
                    max_targets: Some(1),
                    requires_type: None,
                }),
            )
            .with_uses(uses),
        });
        c
    }

    /// The whole mechanism, one step at a time: a melee hit sets it off, the
    /// blow's own damage still lands, the reaction is spent for the round,
    /// and an arrow never sets it off at all.
    #[test]
    fn a_reaction_answers_whoever_hits_it_in_melee_and_ignores_an_arrow() {
        let priest = storm_priest(crate::creature::Uses::Unlimited);
        let raider = puncher("raider", 10, 200, 30, 4);
        let roster = [(&priest, Side::A), (&raider, Side::B)];
        let (mut fight, mut rng) = fight_of(&roster, 21);
        let mut notes = Vec::new();

        fight.react_to_being_hit(
            &mut rng,
            0,
            1,
            AttackKind::MELEE_WEAPON,
            true,
            true,
            &mut notes,
        );
        assert_eq!(fight.fighters[1].hp, 190, "the storm answers the blade");
        assert!(
            !fight.fighters[0].reaction,
            "and the round's reaction is gone"
        );
        assert!(notes.join(" ").contains("Wrath"), "{notes:?}");

        // Spent: a second hit in the same round answers nothing.
        fight.react_to_being_hit(
            &mut rng,
            0,
            1,
            AttackKind::MELEE_WEAPON,
            true,
            false,
            &mut notes,
        );
        assert_eq!(fight.fighters[1].hp, 190);

        // Back on its own turn - but an arrow is not what it answers.
        refresh(&mut fight.fighters[0], &mut rng);
        fight.react_to_being_hit(
            &mut rng,
            0,
            1,
            AttackKind::RANGED_WEAPON,
            true,
            false,
            &mut notes,
        );
        assert_eq!(fight.fighters[1].hp, 190);
        assert!(fight.fighters[0].reaction, "nothing was spent on an arrow");
    }

    /// Its budget is its own: three uses is three answers across the fight,
    /// however many turns refresh the reaction itself.
    #[test]
    fn a_limited_answer_runs_out_even_though_the_reaction_comes_back() {
        let priest = storm_priest(crate::creature::Uses::Limited(2));
        let raider = puncher("raider", 10, 200, 30, 4);
        let roster = [(&priest, Side::A), (&raider, Side::B)];
        let (mut fight, mut rng) = fight_of(&roster, 22);
        for expected in [190, 180, 180, 180] {
            refresh(&mut fight.fighters[0], &mut rng);
            fight.react_to_being_hit(
                &mut rng,
                0,
                1,
                AttackKind::MELEE_WEAPON,
                true,
                false,
                &mut Vec::new(),
            );
            assert_eq!(fight.fighters[1].hp, expected);
        }
    }

    /// Live, through the fight loop rather than by hand: a defender that
    /// answers blows costs its attacker hit points, and the answer really is
    /// capped at one a round - three swings a turn cannot draw three.
    #[test]
    fn answering_blows_is_capped_at_one_a_round_in_a_real_fight() {
        let priest = {
            let mut c = storm_priest(crate::creature::Uses::Unlimited);
            c.hp = 10_000;
            c.initiative = -100;
            c
        };
        let raider = {
            let mut c = puncher("raider", 10, 10_000, 30, 4);
            c.actions[0].effect = Effect::Strikes {
                strike: Strike::new(30, vec![DamageRoll::new(1, 4, 0, DamageKind::Slashing)]),
                count: 3,
            };
            c.initiative = 100;
            c
        };
        let mut rng = Rng::new(23);
        let mut answered = 0;
        for _ in 0..50 {
            let mut log = Some(Vec::new());
            let outcome = run(
                &mut rng,
                [&priest, &raider],
                [Policy::Greedy; 2],
                1,
                &mut log,
            );
            let round = log
                .unwrap()
                .join(
                    "
",
                )
                .matches("Wrath")
                .count();
            assert!(round <= 1, "one reaction a round, whatever hits it");
            answered += round;
            assert_eq!(
                outcome.damage_dealt[0] % 10,
                0,
                "every answer is the same flat 10"
            );
        }
        assert!(answered > 0, "three swings a round should have drawn some");
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
