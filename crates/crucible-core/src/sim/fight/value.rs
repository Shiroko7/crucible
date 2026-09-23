//! What a move is worth right now, in expected damage or healing - the
//! number every ranking policy compares.

use crate::creature::{Boon, Creature, Effect, Move, Reach, Rider, Strike, Zone};
use crate::prob::Pmf;
use crate::rules::{
    hit_outcomes_with, AttackModifier, Condition, DamageKind, DamageRoll, Duration, HealRoll,
    Reduction, RollMode,
};
use crate::sim::fight::attack::attack_mode;
use crate::sim::fight::reactions::standing_ac_boost;
use crate::sim::fight::threshold::reducer;
use crate::sim::fight::{Fight, Fighter, Slot};
use crate::sim::{Budget, Policy, Side};

/// What a move would be worth *if* something else were true - the question a
/// setup move is scored by.
///
/// A move that only helps the next blow is worth exactly what it adds to that
/// blow, so scoring it means valuing an attack under a hypothetical: with
/// advantage on the roll (Steady Aim), or with extra dice riding on every hit
/// (a mark on the target, an enchantment on the blade). One turn's worth of
/// it, which for anything lasting longer than a turn is a floor rather than
/// the whole story - see [`Fight::boost_gain`].
#[derive(Clone, Copy, Default)]
pub(super) struct Boost<'b> {
    /// ... the attacker rolled with advantage.
    pub advantage: bool,
    /// ... every hit carried something more.
    pub extra: Extra<'b>,
}

/// The extra damage half of a [`Boost`].
#[derive(Clone, Copy, Default)]
pub(super) enum Extra<'b> {
    #[default]
    None,
    /// Dice on every hit against a marked target - see
    /// [`Rider::BonusDamageVsQuarry`].
    Quarry(DamageRoll),
    /// A boon's dice, on the swings that boon actually rides - which is the
    /// boon's own question to answer, not this one's.
    Boon(&'b Boon),
}

impl<'b> Boost<'b> {
    fn with_advantage() -> Self {
        Self {
            advantage: true,
            extra: Extra::None,
        }
    }

    fn of(extra: Extra<'b>) -> Self {
        Self {
            advantage: false,
            extra,
        }
    }

    /// What this hypothetical adds to one swing, if anything.
    fn rolls_on(&self, strike: &Strike) -> Option<DamageRoll> {
        match self.extra {
            Extra::None => None,
            Extra::Quarry(roll) => Some(roll),
            Extra::Boon(boon) => boon.damage_on(strike),
        }
    }
}

impl<'a> Fight<'a> {
    /// What `m` is worth to `me` against `target` right now, in expected hit
    /// points: the damage it deals - every damage rider the attack would
    /// actually carry included, so Sneak Attack and a slaying weapon's dice
    /// count towards choosing the move that earns them - or, for a heal, the
    /// downed ally it would bring back. `boost` scores it under a
    /// hypothetical; see [`Boost`].
    ///
    /// A move none of which can land on `target` from here - out of reach
    /// around a creature with a mouth, or swallowed - is not a choice at all,
    /// and neither is one whose double is dead or whose blade has gone out
    /// ([`Fight::requirement_met`]).
    pub(super) fn move_value(&self, me: usize, target: usize, m: &Move, boost: Boost<'_>) -> f64 {
        if !self.requirement_met(me, m) || !self.move_lands(me, target, m) {
            return f64::NEG_INFINITY;
        }
        self.effect_value(me, target, &m.effect, &m.riders, m.reach, boost)
    }

    fn effect_value(
        &self,
        me: usize,
        target: usize,
        effect: &Effect,
        riders: &[Rider],
        reach: Reach,
        boost: Boost<'_>,
    ) -> f64 {
        let attacker = self.fighters[me].creature;
        let victim = self.fighters[target].creature;
        match effect {
            // A part aimed at a creature out of its reach - swallowed, out of
            // a bite's reach, behind a cone - lands nowhere.
            Effect::Strikes { strike, .. }
                if !self.in_reach(me, target, reach, Some(strike.kind)) =>
            {
                0.0
            }
            Effect::Save(_) | Effect::AutoHit { .. } if !self.in_reach(me, target, reach, None) => {
                0.0
            }
            Effect::Strikes { strike, count } => {
                self.expected_strikes(me, target, strike, *count, riders, boost)
            }
            Effect::Heal(roll) => self.heal_value(me, roll),
            Effect::Sequence(parts) => parts
                .iter()
                .map(|p| self.effect_value(me, target, p, riders, reach, boost))
                .sum(),
            Effect::Part {
                effect,
                riders: own,
                reach: own_reach,
            } => {
                let riders: Vec<Rider> = riders.iter().chain(own).cloned().collect();
                self.effect_value(me, target, effect, &riders, own_reach.within(reach), boost)
            }
            // A save only some creature types are subject to, against a
            // target that is not one: it catches nobody.
            Effect::Save(save)
                if save
                    .requires_type
                    .as_ref()
                    .is_some_and(|t| !victim.is_creature_type(t)) =>
            {
                f64::NEG_INFINITY
            }
            Effect::Save(save) => {
                let weak = self.through_weak_spot(me, target, None);
                let defending = self.boon_resistances(target);
                let pmf = save.damage_pmf_by(victim, &reducer(attacker, victim, weak, &defending));
                self.past_threshold(target, weak, pmf).mean()
            }
            Effect::AutoHit { damage } => {
                let weak = self.through_weak_spot(me, target, None);
                let defending = self.boon_resistances(target);
                let reduce = reducer(attacker, victim, weak, &defending);
                damage
                    .iter()
                    .map(|r| {
                        self.past_threshold(
                            target,
                            weak,
                            r.pmf(false, r.reduction_against(&reduce)),
                        )
                        .mean()
                    })
                    .sum()
            }
            // A mark is worth what it will add to this creature's blows over
            // the rounds it lasts - and nothing at all if no rider is waiting
            // on it. Marking what is already marked is not a worse choice
            // than swinging, it is no choice at all: it would change nothing
            // whatsoever, so it is ruled out the same way a move that cannot
            // reach is, and no playstyle - not even one reaching for the most
            // expensive thing it can find - spends a turn on it.
            Effect::Afflict {
                condition,
                duration,
            } => match condition {
                _ if self.fighters[target].has(|c| c == *condition) => f64::NEG_INFINITY,
                Condition::Quarry => self.fighters[me]
                    .creature
                    .riders
                    .iter()
                    .find_map(quarry_dice)
                    .map_or(0.0, |roll| {
                        self.lasting_value(me, target, *duration, Boost::of(Extra::Quarry(roll)))
                    }),
                _ => 0.0,
            },
            // A boon likewise: putting up one already up is no choice at all,
            // so nothing ever spends a turn on the same form twice.
            Effect::Boon { which, duration } => match self.has_boon(me, *which) {
                true => f64::NEG_INFINITY,
                false => self.fighters[me]
                    .creature
                    .boons
                    .get(*which)
                    .map_or(0.0, |boon| {
                        self.lasting_value(me, target, *duration, Boost::of(Extra::Boon(boon)))
                    }),
            },
            // An aura is worth what it collects from whoever has to stand in
            // it, once per turn for as long as it is up - not what it adds to
            // this creature's own blow, which is the boon's question. Raising
            // one already up is no choice at all, exactly as with a boon.
            Effect::Aura { which, duration } => match self.has_aura(me, *which) {
                true => f64::NEG_INFINITY,
                false => self.fighters[me]
                    .creature
                    .lasting_auras
                    .get(*which)
                    .map_or(0.0, |aura| {
                        let horizon = match duration {
                            Duration::Rounds(rounds) | Duration::RoundsOrDamaged(rounds) => {
                                (*rounds).min(self.budget.depth.max(1))
                            }
                            _ => 1,
                        };
                        f64::from(horizon) * aura.effect.mean_damage(self.fighters[target].creature)
                    }),
            },
            // A double is worth the blow it will strike once it is standing.
            Effect::Summon { which } => self.summon_value(target, me, *which),
            // Whoever is inside, not `target`, takes it.
            Effect::HarmSwallowed { damage } => self
                .held_by(me)
                .into_iter()
                .map(|i| {
                    let defending = self.boon_resistances(i);
                    let reduce = reducer(attacker, self.fighters[i].creature, false, &defending);
                    damage
                        .iter()
                        .map(|r| r.pmf(false, r.reduction_against(&reduce)).mean())
                        .sum::<f64>()
                })
                .sum(),
            other => other.mean_damage(victim),
        }
    }

    /// A bonus action's worth, given the action this turn already leads
    /// with: its own value, or - for one that sets that action up
    /// ([`Move::before_action`] with a stance granting advantage on the
    /// attacker's own roll, Steady Aim) - exactly what the advantage adds to
    /// it.
    ///
    /// Two things the action already settles are worth nothing again: a
    /// second spell slot this turn (the action spent the one allowed), and a
    /// second heal for the same downed ally the action is already bringing
    /// back.
    pub(super) fn bonus_value(
        &self,
        me: usize,
        target: usize,
        m: &Move,
        lead: Option<&Move>,
    ) -> f64 {
        if let Some(a) = lead {
            let second_slot = a.spell_slot_level.is_some() && m.spell_slot_level.is_some();
            let second_heal =
                matches!(a.effect, Effect::Heal(_)) && matches!(m.effect, Effect::Heal(_));
            if second_slot || second_heal {
                return f64::NEG_INFINITY;
            }
        }
        let sets_up = m.before_action
            && m.effect
                .stance()
                .is_some_and(Condition::advantage_on_attacks);
        if !sets_up {
            return self.move_value(me, target, m, Boost::default());
        }
        lead.map_or(0.0, |a| {
            let with = self.move_value(me, target, a, Boost::with_advantage());
            let without = self.move_value(me, target, a, Boost::default());
            with - without
        })
    }

    /// What something that lasts is worth: what it adds to this creature's
    /// best blow, over however many rounds are worth counting.
    ///
    /// The horizon is the search's own depth budget ([`Budget::depth`]) - how
    /// far ahead this fight plans - capped by how long the effect actually
    /// lasts. That is a judgement, and it is the same judgement the solver
    /// makes when it stops playing a rollout out and scores the position;
    /// borrowing it rather than inventing a second number is the point. A
    /// mark worth `g` a turn for the next four rounds scores `4g`, so a
    /// ranking playstyle puts it up once and then swings, while one that
    /// spends nothing still never bothers.
    ///
    /// Nothing here counts what a boon is worth *defensively* - resistances
    /// have no attack value to gain - so a purely defensive form scores zero
    /// and is taken only by a playstyle that reaches for expensive things, or
    /// by the solver, which plays it out and sees the difference.
    fn lasting_value(&self, me: usize, target: usize, duration: Duration, boost: Boost<'_>) -> f64 {
        let horizon = match duration {
            Duration::Rounds(rounds) | Duration::RoundsOrDamaged(rounds) => {
                rounds.min(self.budget.depth.max(1))
            }
            // Anything ending at a turn boundary is one turn of it.
            _ => 1,
        };
        f64::from(horizon) * self.boost_gain(me, target, boost)
    }

    /// What a hypothetical is worth on this creature's own best blow: the
    /// most any attacking action it could take right now gains from it.
    fn boost_gain(&self, me: usize, target: usize, boost: Boost<'_>) -> f64 {
        let f = &self.fighters[me];
        Slot::Action
            .moves(f.creature)
            .iter()
            .zip(Slot::Action.states(f))
            .filter(|(m, state)| {
                first_strike(&m.effect).is_some()
                    && state.available()
                    && f.can_pay(m.cost)
                    && f.can_cast(m.spell_slot_level)
                    && self.can_take(me, m)
            })
            .map(|(m, _)| {
                let with = self.move_value(me, target, m, boost);
                let without = self.move_value(me, target, m, Boost::default());
                match with.is_finite() && without.is_finite() {
                    true => with - without,
                    false => 0.0,
                }
            })
            .fold(0.0, f64::max)
    }

    /// What calling up `me`'s `which`th summon is worth: one turn of the best
    /// blow it can be commanded to strike against `target`.
    ///
    /// Its body - something else for an enemy to hit - is worth more than
    /// that, and is not counted: there is no threat model here to say whether
    /// anything would bother attacking it.
    fn summon_value(&self, target: usize, me: usize, which: usize) -> f64 {
        let victim = self.fighters[target].creature;
        self.fighters[me]
            .creature
            .summons
            .get(which)
            .map_or(0.0, |summon| {
                summon
                    .actions
                    .iter()
                    .map(|m| m.effect.mean_damage(victim))
                    .fold(0.0, f64::max)
            })
    }

    /// Expected damage of `count` strikes, the first with every rider that
    /// would apply to it; later swings without the once-per-turn Sneak
    /// Attack the first one would already have claimed.
    fn expected_strikes(
        &self,
        me: usize,
        target: usize,
        strike: &Strike,
        count: u32,
        riders: &[Rider],
        boost: Boost<'_>,
    ) -> f64 {
        let attacker = &self.fighters[me];
        let victim = &self.fighters[target];
        let mode = attack_mode(strike.mode, attacker, victim, boost.advantage);
        let force_crit = victim.has(Condition::auto_crits);
        // What the hypothetical would add to each of these swings, on top of
        // every rider they already carry.
        let hypothetical = boost.rolls_on(strike);
        let with_boost = |mut rolls: Vec<DamageRoll>| {
            rolls.extend(hypothetical);
            rolls
        };
        let first = self.extra_damage_plan(me, target, strike, riders, mode, true);
        let mut total = self
            .aimed_hit(
                me,
                target,
                strike,
                mode,
                force_crit,
                &with_boost(first.rolls),
            )
            .0;
        if count > 1 {
            let rest = self.extra_damage_plan(me, target, strike, riders, mode, false);
            total += f64::from(count - 1)
                * self
                    .aimed_hit(
                        me,
                        target,
                        strike,
                        mode,
                        force_crit,
                        &with_boost(rest.rolls),
                    )
                    .0;
        }
        total
    }

    /// Expected damage of one attack roll from `me` at `target` in `mode`,
    /// with `extra` riding on a hit - and whether it is aimed at `target`'s
    /// weak spot.
    ///
    /// From inside there is nothing else to aim at. From outside, an open
    /// weak spot is taken when it is worth more than the shell: it skips the
    /// threshold but resists some damage, so a hit big enough to breach can
    /// do better against the shell, where it lands whole.
    pub(super) fn aimed_hit(
        &self,
        me: usize,
        target: usize,
        strike: &Strike,
        mode: RollMode,
        force_crit: bool,
        extra: &[DamageRoll],
    ) -> (f64, bool) {
        let attacker = &self.fighters[me];
        let victim = &self.fighters[target];
        let defending = self.boon_resistances(target);
        // A blade the defender already has raised is part of how hard it is
        // to hit - see `reactions::standing_ac_boost`.
        let ac = victim.creature.ac + standing_ac_boost(victim, strike.kind);
        let hit = |weak: bool| {
            let threshold = match weak {
                true => None,
                false => victim.creature.damage_threshold().map(|(t, _)| t),
            };
            expected_hit(
                &reducer(attacker.creature, victim.creature, weak, &defending),
                threshold,
                strike,
                ac,
                mode,
                force_crit,
                &attacker.attack_modifiers,
                extra,
            )
        };
        if !self.through_weak_spot(me, target, Some(strike.kind)) {
            return (hit(false), false);
        }
        if attacker.swallowed_by() == Some(target) {
            return (hit(true), true);
        }
        let (through, shell) = (hit(true), hit(false));
        if through >= shell {
            (through, true)
        } else {
            (shell, false)
        }
    }

    /// A heal is worth a whole ally back in the fight when one is down - its
    /// hit point maximum - and nothing otherwise: topping up someone still
    /// standing is almost never worth a turn in a fight this short.
    fn heal_value(&self, me: usize, _roll: &HealRoll) -> f64 {
        let side = self.fighters[me].side;
        self.fighters
            .iter()
            .enumerate()
            .filter(|&(i, f)| f.side == side && f.can_revive() && self.reaches(me, i))
            .map(|(_, f)| f64::from(f.creature.hp))
            .fold(0.0, f64::max)
    }
}

/// The dice a [`Rider::BonusDamageVsQuarry`] would put on every hit against
/// a marked target - what marking one is worth, before it is marked.
fn quarry_dice(rider: &Rider) -> Option<DamageRoll> {
    match rider {
        Rider::BonusDamageVsQuarry {
            dice_count,
            dice_sides,
            bonus,
            damage_kind,
        } => Some(DamageRoll::new(
            *dice_count,
            *dice_sides,
            *bonus,
            *damage_kind,
        )),
        _ => None,
    }
}

pub(super) fn first_strike(effect: &Effect) -> Option<&Strike> {
    match effect {
        Effect::Strikes { strike, .. } => Some(strike),
        Effect::Sequence(parts) => parts.iter().find_map(first_strike),
        Effect::Part { effect, .. } => first_strike(effect),
        _ => None,
    }
}

/// Expected damage of one attack roll in `mode` against `ac`, with `extra`
/// riding on a hit: the probability of each outcome times the mean of each
/// damage component, reduced by `reduce`. A `force_crit` target turns every
/// hit into a critical one.
///
/// Against a damage `threshold` the components no longer simply add - a hit
/// either clears it whole or lands nothing - so the hit's full distribution
/// is built and cut at the threshold instead.
#[allow(clippy::too_many_arguments)]
fn expected_hit(
    reduce: &dyn Fn(DamageKind) -> Reduction,
    threshold: Option<i32>,
    strike: &Strike,
    ac: i32,
    mode: RollMode,
    force_crit: bool,
    modifiers: &[AttackModifier],
    extra: &[DamageRoll],
) -> f64 {
    let o = hit_outcomes_with(strike.to_hit, mode, ac, modifiers);
    let rolls = || strike.damage.iter().chain(extra);
    let landed = |crit: bool| -> f64 {
        match threshold {
            None => rolls()
                .map(|r| r.pmf(crit, r.reduction_against(reduce)).mean())
                .sum(),
            Some(t) => rolls()
                .fold(Pmf::constant(0), |acc, r| {
                    acc.convolve(&r.pmf(crit, r.reduction_against(reduce)))
                })
                .map_values(move |d| if d >= t { d } else { 0 })
                .mean(),
        }
    };
    let (hit, crit) = if force_crit {
        (0.0, o.hit + o.crit)
    } else {
        (o.hit, o.crit)
    };
    hit * landed(false) + crit * landed(true)
}

/// What `m` is worth when `attacker` uses it on `target` at the start of a
/// fight, in expected damage - the number the ranking policies compare, with
/// every rider that would apply to the attack included (a slaying weapon's
/// dice, say) and none that needs a situation the fight has not produced yet
/// (Sneak Attack without advantage or an ally beside the target). Around a
/// creature with a mouth, the two are face to face at its mouth, where
/// everything reaches.
pub fn expected_damage(attacker: &Creature, m: &Move, target: &Creature) -> f64 {
    let mut fight = Fight {
        fighters: vec![
            Fighter::new(attacker, Side::A, Policy::Greedy, 0),
            Fighter::new(target, Side::B, Policy::Greedy, 1),
        ],
        order: vec![0, 1],
        turns_lost: [0; 2],
        max_rounds: 1,
        budget: Budget::default(),
        rollout_plan: None,
        acting: None,
        sole_target: None,
    };
    fight.place_everyone(Zone::Mouth);
    // It is the attacker's own turn, which is what a once-a-turn rider needs
    // to be true before it counts.
    fight.acting = Some(0);
    fight.move_value(0, 1, m, Boost::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::SaveEffect;
    use crate::prob::Rng;
    use crate::rules::{Ability, DamageKind, Duration};
    use crate::sim::fight::test_support::{fight_of, puncher};

    /// A spell only one creature type is subject to is never taken against
    /// anything else - not even by a policy that ranks on what it spends.
    #[test]
    fn a_type_restricted_save_is_never_chosen_against_the_wrong_type() {
        let mut caster = puncher("caster", 10, 30, 5, 2);
        caster.spell_slots.set_max(2, 3);
        caster.actions.insert(
            0,
            Move::new(
                "Hold",
                Effect::Save(SaveEffect {
                    ability: Ability::Wis,
                    dc: 15,
                    damage: Vec::new(),
                    half_on_success: false,
                    on_failure: vec![(Condition::Paralyzed, Duration::VictimTurn)],
                    max_targets: Some(1),
                    requires_type: Some("humanoid".to_string()),
                }),
            )
            .with_spell_slot(2),
        );
        let dragon =
            puncher("dragon", 10, 100, 5, 2).with_creature_type(crate::rules::CreatureType::Dragon);
        let bandit = puncher("bandit", 10, 100, 5, 2)
            .with_creature_type(crate::rules::CreatureType::Humanoid);
        for (foe, expected) in [(&dragon, Some(1)), (&bandit, Some(0))] {
            let roster = [(&caster, Side::A), (foe, Side::B)];
            let mut rng = Rng::new(1);
            let mut fight = Fight::new(
                &mut rng,
                &roster,
                [Policy::Nova; 2],
                5,
                Budget::default(),
                &mut None,
            );
            assert_eq!(fight.decide(1, 0, 1, &mut rng).action, expected);
            fight.fighters[0].policy = Policy::InOrder;
            assert_eq!(fight.decide(1, 0, 1, &mut rng).action, expected);
        }
    }

    /// A ranking policy spends its bonus action bringing a downed ally back
    /// rather than on a little damage.
    #[test]
    fn a_heal_is_worth_a_downed_ally() {
        let mut healer = puncher("healer", 10, 20, 5, 2);
        healer.bonus_actions.push(Move::new(
            "Jab",
            Effect::Strikes {
                strike: Strike::new(5, vec![DamageRoll::new(1, 4, 0, DamageKind::Piercing)]),
                count: 1,
            },
        ));
        healer.bonus_actions.push(Move::new(
            "Healing Word",
            Effect::Heal(HealRoll::new(1, 4, 3)),
        ));
        let mut friend = puncher("friend", 10, 30, 5, 2);
        friend.player_character = true;
        let enemy = puncher("enemy", 10, 30, 5, 2);
        let roster = [(&healer, Side::A), (&friend, Side::A), (&enemy, Side::B)];
        let (mut fight, mut rng) = fight_of(&roster, 13);

        assert_eq!(
            fight.decide(1, 0, 2, &mut rng).bonus,
            Some(0),
            "nobody down: jab"
        );
        fight.fighters[1].hp = 0;
        assert_eq!(
            fight.decide(1, 0, 2, &mut rng).bonus,
            Some(1),
            "friend down: heal"
        );
    }
}
