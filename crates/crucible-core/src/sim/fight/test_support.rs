//! Fixtures the fight tests share.

use crate::creature::{AttackKind, Creature, Effect, Move, Rider, Strike};
use crate::prob::Rng;
use crate::rules::{DamageKind, DamageRoll, RollMode};
use crate::sim::fight::Fight;
use crate::sim::{Budget, Policy, Side};

pub(super) fn puncher(name: &str, ac: i32, hp: i32, to_hit: i32, bonus: i32) -> Creature {
    Creature::new(name, ac, hp).with_action(Move::new(
        "Punch",
        Effect::Strikes {
            strike: Strike::new(
                to_hit,
                vec![DamageRoll::new(1, 6, bonus, DamageKind::Bludgeoning)],
            ),
            count: 1,
        },
    ))
}

pub(super) fn no_log() -> Option<Vec<String>> {
    None
}

pub(super) fn fight_of<'a>(roster: &[(&'a Creature, Side)], seed: u64) -> (Fight<'a>, Rng) {
    let mut rng = Rng::new(seed);
    let fight = Fight::new(
        &mut rng,
        roster,
        [Policy::Greedy; 2],
        10,
        Budget::default(),
        &mut None,
    );
    (fight, rng)
}

/// Resolve `m` from `me` at `target` once, returning the damage done.
pub(super) fn strike_once(
    fight: &mut Fight<'_>,
    rng: &mut Rng,
    me: usize,
    target: usize,
    m: &Move,
) -> i32 {
    let before = fight.fighters[target].hp;
    fight.resolve(
        &m.effect,
        rng,
        me,
        target,
        &m.riders,
        m.reach,
        false,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut None,
    );
    before - fight.fighters[target].hp
}

pub(super) fn sneak_attacker(name: &str) -> Creature {
    Creature::new(name, 15, 50).with_rider(Rider::ConditionalExtraDamage {
        dice_count: 3,
        dice_sides: 6,
        once_per_turn: true,
    })
}

pub(super) fn bow(to_hit: i32, mode: RollMode) -> Move {
    let mut strike = Strike::new(to_hit, vec![DamageRoll::new(1, 8, 3, DamageKind::Piercing)])
        .with_kind(AttackKind::RANGED_WEAPON);
    strike.mode = mode;
    Move::new("Bow", Effect::Strikes { strike, count: 1 })
}
