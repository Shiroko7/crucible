//! Landing damage, and the shell some creatures put in its way: a damage
//! threshold shrugs off any single instance below it whole, a breach lands
//! in full - and may crack the shell and set off a reaction - and a weak spot
//! skips the threshold for a resistance of its own.

use crate::creature::{Creature, ReactionTrigger};
use crate::prob::{Pmf, Rng};
use crate::rules::{Condition, DamageKind, Reduction};
use crate::sim::fight::conditions::halve_if_suppressed;
use crate::sim::fight::{Expiry, Fight};

/// What a damage threshold made of one instance of damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shell {
    /// No threshold in the way: a weak spot, or no shell at all.
    Open,
    /// Below the threshold, so none of it landed.
    Absorbed,
    /// At or over the threshold, so all of it landed.
    Breached,
}

impl<'a> Fight<'a> {
    /// Does damage from `from` reach `to`'s weak spot, skipping its damage
    /// threshold?
    ///
    /// From inside - `from` is swallowed by `to` - always. From outside only
    /// an attack roll does, and only while the weak spot is open:
    /// [`Condition::Exposed`] and not [`Condition::Sealed`], or
    /// [`Condition::Cracked`], which sealing does not close. An area or a
    /// dart is aimed at the creature, not at a spot on it. A creature
    /// without a damage threshold has no weak spot to reach.
    pub(super) fn through_weak_spot(&self, from: usize, to: usize, attack_roll: bool) -> bool {
        let target = &self.fighters[to];
        if target.creature.damage_threshold().is_none() {
            return false;
        }
        if self.fighters[from].swallowed_by() == Some(to) {
            return true;
        }
        attack_roll
            && (target.has(|c| c == Condition::Cracked)
                || (target.has(|c| c == Condition::Exposed)
                    && !target.has(|c| c == Condition::Sealed)))
    }

    /// Land `raw` damage from `me` on `target`: halved if `me` is
    /// suppressed, then held to `target`'s damage threshold unless the
    /// damage came through its weak spot (`weak`), then taken off its hit
    /// points. A breach leaves a crack if the shell cracks. Returns what
    /// actually landed; see [`Fight::answer_breach`] for what a breach sets
    /// off, which the caller runs once it has recorded the hit.
    pub(super) fn deal(
        &mut self,
        rng: &mut Rng,
        me: usize,
        target: usize,
        raw: i32,
        weak: bool,
    ) -> (i32, Shell) {
        let mut dealt = halve_if_suppressed(&self.fighters, me, raw);
        let mut shell = Shell::Open;
        if let (false, Some((threshold, cracks))) =
            (weak, self.fighters[target].creature.damage_threshold())
        {
            if dealt > 0 && dealt < threshold {
                dealt = 0;
                shell = Shell::Absorbed;
            } else if dealt > 0 {
                shell = Shell::Breached;
                if cracks {
                    self.fighters[target].add_condition(Condition::Cracked, Expiry::RoundEnd);
                }
            }
        }
        if self.fighters[me].swallowed_by() == Some(target) {
            self.fighters[target].inside_damage += dealt;
        }
        self.fighters[me].dealt += i64::from(dealt);
        self.apply_damage(rng, target, dealt);
        (dealt, shell)
    }

    /// A breach of `target`'s threshold by `me` sets off `target`'s reaction
    /// waiting for one, aimed back at `me` - if `target` survived it.
    pub(super) fn answer_breach(
        &mut self,
        rng: &mut Rng,
        me: usize,
        target: usize,
        shell: Shell,
        record: bool,
        notes: &mut Vec<String>,
    ) {
        if shell == Shell::Breached {
            self.react(rng, target, ReactionTrigger::Breached, me, record, notes);
        }
    }

    /// `pmf` - the damage of one instance, from `me` to `target` - as
    /// `target`'s threshold lets it land: every outcome below it becomes
    /// zero, unless the damage came through the weak spot.
    pub(super) fn past_threshold(&self, target: usize, weak: bool, pmf: Pmf) -> Pmf {
        match (weak, self.fighters[target].creature.damage_threshold()) {
            (false, Some((threshold, _))) => {
                pmf.map_values(move |d| if d >= threshold { d } else { 0 })
            }
            _ => pmf,
        }
    }
}

/// How `target` reduces each damage type from `attacker`: its own
/// reductions, with its weak spot's resistances on top when the damage came
/// through it.
pub(super) fn reducer<'c>(
    attacker: &'c Creature,
    target: &'c Creature,
    weak: bool,
) -> impl Fn(DamageKind) -> Reduction + 'c {
    move |kind| {
        let base = target.reduction_from(kind, attacker);
        if weak && target.weak_spot_resists().contains(&kind) {
            base.with_resistance()
        } else {
            base
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Effect, Move, Reaction, Rider, SaveEffect, Strike};
    use crate::rules::{Ability, DamageRoll, RollMode, Size};
    use crate::sim::fight::attack::attack_mode;
    use crate::sim::fight::test_support::{fight_of, strike_once};
    use crate::sim::Side;

    fn shell(threshold: i32, cracks: bool) -> Creature {
        Creature::new("shell", 15, 10_000_000).with_rider(Rider::DamageThreshold {
            threshold,
            cracks,
            weak_spot_resists: vec![DamageKind::Slashing],
        })
    }

    fn slash(to_hit: i32, roll: DamageRoll) -> Move {
        Move::new(
            "Slash",
            Effect::Strikes {
                strike: Strike::new(to_hit, vec![roll]),
                count: 1,
            },
        )
    }

    fn strike_of(m: &Move) -> &Strike {
        match &m.effect {
            Effect::Strikes { strike, .. } => strike,
            other => panic!("expected strikes, got {other:?}"),
        }
    }

    #[test]
    fn damage_below_the_threshold_does_nothing_and_a_breach_lands_whole() {
        let attacker = Creature::new("attacker", 10, 100);
        let target = shell(20, false);
        let (mut fight, mut rng) = fight_of(&[(&attacker, Side::A), (&target, Side::B)], 1);
        let full = fight.fighters[1].hp;

        assert_eq!(fight.deal(&mut rng, 0, 1, 19, false), (0, Shell::Absorbed));
        assert_eq!(fight.fighters[1].hp, full);
        assert_eq!(fight.deal(&mut rng, 0, 1, 20, false), (20, Shell::Breached));
        assert_eq!(fight.fighters[1].hp, full - 20);
        assert_eq!(
            fight.deal(&mut rng, 0, 1, 5, true),
            (5, Shell::Open),
            "a weak spot skips the threshold"
        );
        assert!(
            !fight.fighters[1].has(|c| c == Condition::Cracked),
            "this shell does not crack"
        );
        assert_eq!(fight.fighters[0].dealt, 25, "only what landed counts");
    }

    #[test]
    fn a_breach_cracks_the_shell_open_to_attack_rolls_until_the_round_ends() {
        let attacker = Creature::new("attacker", 10, 100);
        let target = shell(20, true);
        let (mut fight, mut rng) = fight_of(&[(&attacker, Side::A), (&target, Side::B)], 1);

        assert!(!fight.through_weak_spot(0, 1, true));
        fight.deal(&mut rng, 0, 1, 25, false);
        assert!(
            fight.through_weak_spot(0, 1, true),
            "an attack roll finds the crack"
        );
        assert!(!fight.through_weak_spot(0, 1, false), "an area does not");
        fight.end_of_round();
        assert!(
            !fight.through_weak_spot(0, 1, true),
            "it closes with the round"
        );
    }

    #[test]
    fn sealing_shuts_an_exposed_weak_spot_but_not_a_crack() {
        let attacker = Creature::new("attacker", 10, 100);
        let target = shell(20, true);
        let (mut fight, _) = fight_of(&[(&attacker, Side::A), (&target, Side::B)], 1);

        fight.apply_condition(1, Condition::Exposed, Expiry::TurnStart(1));
        assert!(fight.through_weak_spot(0, 1, true));
        fight.apply_condition(1, Condition::Sealed, Expiry::TurnStart(1));
        assert!(!fight.through_weak_spot(0, 1, true), "sealed shut");
        fight.apply_condition(1, Condition::Cracked, Expiry::RoundEnd);
        assert!(
            fight.through_weak_spot(0, 1, true),
            "a crack is not a mouth"
        );
    }

    /// Anything a swallowed creature does reaches the inside - seal or no
    /// seal, attack roll or not.
    #[test]
    fn from_inside_the_weak_spot_is_always_reached() {
        let prey = Creature::new("prey", 10, 100);
        let target = shell(20, false);
        let (mut fight, _) = fight_of(&[(&prey, Side::A), (&target, Side::B)], 1);
        assert!(fight.swallow(1, 0, Size::Huge));
        fight.apply_condition(1, Condition::Sealed, Expiry::TurnStart(1));
        assert!(fight.through_weak_spot(0, 1, true));
        assert!(fight.through_weak_spot(0, 1, false));
    }

    /// The exact value of a hit against a shell - its whole distribution cut
    /// at the threshold, or its weak spot's resistance from inside - is what
    /// live hits average.
    #[test]
    fn live_hits_against_a_shell_agree_with_the_exact_value() {
        // 3d10+10 against a threshold of 30: roughly half of all hits land.
        let m = slash(5, DamageRoll::new(3, 10, 10, DamageKind::Slashing));
        let strike = strike_of(&m);
        let attacker = Creature::new("attacker", 10, 100).with_action(m.clone());
        let target = shell(30, false);

        for inside in [false, true] {
            let (mut fight, mut rng) = fight_of(&[(&attacker, Side::A), (&target, Side::B)], 7);
            if inside {
                fight.swallow(1, 0, Size::Huge);
            }
            let mode = attack_mode(strike.mode, &fight.fighters[0], &fight.fighters[1], false);
            let (exact, weak) = fight.aimed_hit(0, 1, strike, mode, false, &[]);
            assert_eq!(weak, inside);

            let n = 40_000;
            let (mut sum, mut sq) = (0.0, 0.0);
            for _ in 0..n {
                let d = f64::from(strike_once(&mut fight, &mut rng, 0, 1, &m));
                sum += d;
                sq += d * d;
            }
            let mean = sum / f64::from(n);
            let var = sq / f64::from(n) - mean * mean;
            let tol = 5.0 * (var / f64::from(n)).sqrt() + 1e-4;
            assert!(
                (mean - exact).abs() < tol,
                "inside {inside}: sampled {mean} vs exact {exact} (tol {tol})"
            );
        }
    }

    /// Through a resisting weak spot a big hit is halved, so against a crack
    /// an attacker that can breach aims at the shell instead.
    #[test]
    fn a_hit_big_enough_to_breach_is_aimed_at_the_shell_not_the_crack() {
        let attacker = Creature::new("attacker", 10, 100);
        let mut target = shell(20, true);
        target.ac = 1;
        let (mut fight, _) = fight_of(&[(&attacker, Side::A), (&target, Side::B)], 1);
        fight.apply_condition(1, Condition::Cracked, Expiry::RoundEnd);

        let small = slash(10, DamageRoll::new(1, 4, 1, DamageKind::Slashing));
        let big = slash(10, DamageRoll::new(0, 1, 60, DamageKind::Slashing));
        let aim = |m: &Move| {
            fight
                .aimed_hit(0, 1, strike_of(m), RollMode::Normal, false, &[])
                .1
        };
        assert!(aim(&small), "a small hit goes through the crack");
        assert!(
            !aim(&big),
            "60 lands whole on the shell, 30 through the crack"
        );
    }

    #[test]
    fn a_breach_sets_off_the_reaction_waiting_for_one_at_whoever_did_it() {
        let attacker = Creature::new("attacker", 10, 100);
        let mut target = shell(10, false);
        target.reactions.push(Reaction {
            trigger: ReactionTrigger::Breached,
            action: Move::new(
                "Spray",
                Effect::Save(SaveEffect {
                    ability: Ability::Dex,
                    dc: 99,
                    damage: vec![DamageRoll::new(0, 1, 7, DamageKind::Piercing)],
                    half_on_success: false,
                    on_failure: vec![],
                    max_targets: None,
                    requires_type: None,
                }),
            ),
        });
        let (mut fight, mut rng) = fight_of(&[(&attacker, Side::A), (&target, Side::B)], 1);

        let (_, shell) = fight.deal(&mut rng, 0, 1, 5, false);
        fight.answer_breach(&mut rng, 0, 1, shell, false, &mut Vec::new());
        assert_eq!(fight.fighters[0].hp, 100, "absorbed, so nothing to answer");

        let (_, shell) = fight.deal(&mut rng, 0, 1, 15, false);
        fight.answer_breach(&mut rng, 0, 1, shell, false, &mut Vec::new());
        assert_eq!(fight.fighters[0].hp, 93);
        assert!(!fight.fighters[1].reaction, "the reaction is spent");

        let (_, shell) = fight.deal(&mut rng, 0, 1, 15, false);
        fight.answer_breach(&mut rng, 0, 1, shell, false, &mut Vec::new());
        assert_eq!(fight.fighters[0].hp, 93, "one reaction a round");
    }
}
