//! What a move is worth right now, in expected damage or healing - the
//! number every ranking policy compares.

use crate::creature::{Creature, Effect, Move, Rider, Strike};
use crate::rules::{hit_outcomes_with, AttackModifier, Condition, DamageRoll, HealRoll, RollMode};
use crate::sim::fight::attack::attack_mode;
use crate::sim::fight::{Fight, Fighter};
use crate::sim::{Budget, Policy, Side};

impl<'a> Fight<'a> {
    /// What `m` is worth to `me` against `target` right now, in expected hit
    /// points: the damage it deals - every damage rider the attack would
    /// actually carry included, so Sneak Attack and a slaying weapon's dice
    /// count towards choosing the move that earns them - or, for a heal, the
    /// downed ally it would bring back. `steady` scores an attack as though
    /// the attacker had advantage on it.
    pub(super) fn move_value(&self, me: usize, target: usize, m: &Move, steady: bool) -> f64 {
        self.effect_value(me, target, &m.effect, &m.riders, steady)
    }

    fn effect_value(
        &self,
        me: usize,
        target: usize,
        effect: &Effect,
        riders: &[Rider],
        steady: bool,
    ) -> f64 {
        match effect {
            Effect::Strikes { strike, count } => {
                self.expected_strikes(me, target, strike, *count, riders, steady)
            }
            Effect::Heal(roll) => self.heal_value(me, roll),
            Effect::Sequence(parts) => parts
                .iter()
                .map(|p| self.effect_value(me, target, p, riders, steady))
                .sum(),
            // A save only some creature types are subject to, against a
            // target that is not one: it catches nobody.
            Effect::Save(save)
                if save
                    .requires_type
                    .as_ref()
                    .is_some_and(|t| !self.fighters[target].creature.is_creature_type(t)) =>
            {
                f64::NEG_INFINITY
            }
            other => other.mean_damage(self.fighters[target].creature),
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
            return self.move_value(me, target, m, false);
        }
        lead.map_or(0.0, |a| {
            self.move_value(me, target, a, true) - self.move_value(me, target, a, false)
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
        steady: bool,
    ) -> f64 {
        let attacker = &self.fighters[me];
        let victim = &self.fighters[target];
        let mode = attack_mode(strike.mode, attacker, victim, steady);
        let force_crit = victim.has(Condition::auto_crits);
        let first = self.extra_damage_plan(me, target, strike, riders, mode, true);
        let mut total = expected_hit(
            attacker.creature,
            victim.creature,
            strike,
            mode,
            force_crit,
            &attacker.attack_modifiers,
            &first.rolls,
        );
        if count > 1 {
            let rest = self.extra_damage_plan(me, target, strike, riders, mode, false);
            total += f64::from(count - 1)
                * expected_hit(
                    attacker.creature,
                    victim.creature,
                    strike,
                    mode,
                    force_crit,
                    &attacker.attack_modifiers,
                    &rest.rolls,
                );
        }
        total
    }

    /// A heal is worth a whole ally back in the fight when one is down - its
    /// hit point maximum - and nothing otherwise: topping up someone still
    /// standing is almost never worth a turn in a fight this short.
    fn heal_value(&self, me: usize, _roll: &HealRoll) -> f64 {
        let side = self.fighters[me].side;
        self.fighters
            .iter()
            .filter(|f| f.side == side && f.can_revive())
            .map(|f| f64::from(f.creature.hp))
            .fold(0.0, f64::max)
    }
}

pub(super) fn first_strike(effect: &Effect) -> Option<&Strike> {
    match effect {
        Effect::Strikes { strike, .. } => Some(strike),
        Effect::Sequence(parts) => parts.iter().find_map(first_strike),
        _ => None,
    }
}

/// Expected damage of one attack roll in `mode`, with `extra` riding on a
/// hit: the probability of each outcome times the mean of each damage
/// component, reduced the way `target` reduces it against `attacker`. A
/// `force_crit` target turns every hit into a critical one.
fn expected_hit(
    attacker: &Creature,
    target: &Creature,
    strike: &Strike,
    mode: RollMode,
    force_crit: bool,
    modifiers: &[AttackModifier],
    extra: &[DamageRoll],
) -> f64 {
    let o = hit_outcomes_with(strike.to_hit, mode, target.ac, modifiers);
    let landed = |crit: bool| -> f64 {
        strike
            .damage
            .iter()
            .chain(extra)
            .map(|r| r.pmf(crit, target.reduction_from(r.kind, attacker)).mean())
            .sum()
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
/// (Sneak Attack without advantage or an ally beside the target).
pub fn expected_damage(attacker: &Creature, m: &Move, target: &Creature) -> f64 {
    let fight = Fight {
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
    };
    fight.move_value(0, 1, m, false)
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
