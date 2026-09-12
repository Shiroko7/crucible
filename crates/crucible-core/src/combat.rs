//! Attack resolution, both exactly and by sampling.
//!
//! Every rule here is deliberately spelled out rather than folded into a
//! single probability, because each one is a place the two paths can disagree:
//! a natural 1 always misses, a natural 20 always hits and crits, a crit rolls
//! the damage dice twice but adds the modifier once, damage floors at zero
//! before resistance halves it, and halving rounds down.
//!
//! The mixture over miss/hit/crit is the shape every future ability plugs
//! into. Bless lands on the d20 distribution, Sneak Attack on the damage pool,
//! resistance on the multiplier - none of which requires changing the
//! resolution order.

use crate::dice::Pmf;
use crate::rng::Rng;

/// How the d20 is rolled. Advantage and disadvantage are distributions over
/// the *final* value, so "natural 20" means the kept die showed 20.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollMode {
    Normal,
    Advantage,
    Disadvantage,
}

impl RollMode {
    /// P(final roll = k) for k in 1..=20, indexed from zero.
    ///
    /// With advantage the maximum of two dice is at most `k` exactly when both
    /// are, so `P(max <= k) = (k/20)^2` and the mass at `k` is the difference
    /// of consecutive squares - `(2k-1)/400`. Disadvantage is the mirror. This
    /// is why advantage is worth about +3.3 at the middle of the range and
    /// almost nothing at the ends.
    pub fn distribution(self) -> [f64; 20] {
        let mut out = [0.0; 20];
        for (i, slot) in out.iter_mut().enumerate() {
            let k = i as f64 + 1.0;
            *slot = match self {
                RollMode::Normal => 1.0 / 20.0,
                RollMode::Advantage => (2.0 * k - 1.0) / 400.0,
                RollMode::Disadvantage => (41.0 - 2.0 * k) / 400.0,
            };
        }
        out
    }

    /// The sampled counterpart of [`RollMode::distribution`].
    pub fn roll(self, rng: &mut Rng) -> i32 {
        match self {
            RollMode::Normal => rng.die(20),
            RollMode::Advantage => rng.die(20).max(rng.die(20)),
            RollMode::Disadvantage => rng.die(20).min(rng.die(20)),
        }
    }
}

/// Resistance halves and rounds down; vulnerability doubles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reduction {
    #[default]
    Normal,
    Resistant,
    Vulnerable,
}

impl Reduction {
    #[inline]
    pub fn apply(self, damage: i32) -> i32 {
        match self {
            Reduction::Normal => damage,
            // Integer division truncates toward zero, which is what "round
            // down" means for the non-negative values this ever sees.
            Reduction::Resistant => damage / 2,
            Reduction::Vulnerable => damage * 2,
        }
    }
}

/// One attack's offensive profile.
#[derive(Debug, Clone, Copy)]
pub struct Attack {
    pub to_hit: i32,
    pub dice_count: u32,
    pub dice_sides: u32,
    /// Added once on a hit and once on a crit - never doubled.
    pub damage_bonus: i32,
    pub mode: RollMode,
}

impl Attack {
    pub fn new(to_hit: i32, dice_count: u32, dice_sides: u32, damage_bonus: i32) -> Self {
        Self {
            to_hit,
            dice_count,
            dice_sides,
            damage_bonus,
            mode: RollMode::Normal,
        }
    }

    pub fn with_mode(mut self, mode: RollMode) -> Self {
        self.mode = mode;
        self
    }
}

/// What the attack is being thrown at.
#[derive(Debug, Clone, Copy)]
pub struct Defense {
    pub ac: i32,
    pub hp: i32,
    pub reduction: Reduction,
}

impl Defense {
    pub fn new(ac: i32, hp: i32) -> Self {
        Self {
            ac,
            hp,
            reduction: Reduction::Normal,
        }
    }

    pub fn with_reduction(mut self, reduction: Reduction) -> Self {
        self.reduction = reduction;
        self
    }
}

/// The three outcomes of an attack roll. Sums to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outcomes {
    pub miss: f64,
    pub hit: f64,
    pub crit: f64,
}

/// Exact probabilities of miss, ordinary hit and critical hit.
pub fn outcomes(attack: &Attack, defense: &Defense) -> Outcomes {
    let dist = attack.mode.distribution();
    let mut hit = 0.0;
    for (i, &p) in dist.iter().enumerate() {
        let roll = i as i32 + 1;
        // 1 and 20 are handled outside the loop; they ignore the arithmetic.
        if roll > 1 && roll < 20 && roll + attack.to_hit >= defense.ac {
            hit += p;
        }
    }
    let crit = dist[19];
    Outcomes {
        miss: 1.0 - hit - crit,
        hit,
        crit,
    }
}

/// Exact distribution of damage dealt by a single attack, zero included.
pub fn damage_pmf(attack: &Attack, defense: &Defense) -> Pmf {
    let o = outcomes(attack, defense);
    let reduce = defense.reduction;

    let on_hit = Pmf::pool(attack.dice_count, attack.dice_sides)
        .offset(attack.damage_bonus)
        .floor_at(0)
        .map_values(|d| reduce.apply(d));

    // A crit rolls the damage dice twice. The modifier is added once.
    let on_crit = Pmf::pool(attack.dice_count * 2, attack.dice_sides)
        .offset(attack.damage_bonus)
        .floor_at(0)
        .map_values(|d| reduce.apply(d));

    Pmf::mixture(&[
        (o.miss, Pmf::constant(0)),
        (o.hit, on_hit),
        (o.crit, on_crit),
    ])
}

/// One sampled attack. Must be distributed according to [`damage_pmf`].
pub fn sample_damage(rng: &mut Rng, attack: &Attack, defense: &Defense) -> i32 {
    let roll = attack.mode.roll(rng);

    let crit = roll == 20;
    let hit = crit || (roll != 1 && roll + attack.to_hit >= defense.ac);
    if !hit {
        return 0;
    }

    let dice = if crit {
        attack.dice_count * 2
    } else {
        attack.dice_count
    };
    let raw: i32 = (0..dice).map(|_| rng.die(attack.dice_sides)).sum();
    defense.reduction.apply((raw + attack.damage_bonus).max(0))
}

/// Attacks taken to drop the target, or `None` if it survived the cap.
///
/// The cap is not paranoia: a profile that can only ever deal zero - a d4 from
/// a weak attacker against resistance, say - never terminates, and a silent
/// hang is a worse failure than a `None`.
pub fn sample_attacks_to_kill(
    rng: &mut Rng,
    attack: &Attack,
    defense: &Defense,
    cap: u32,
) -> Option<u32> {
    let mut hp = defense.hp;
    for n in 1..=cap {
        hp -= sample_damage(rng, attack, defense);
        if hp <= 0 {
            return Some(n);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn every_roll_mode_is_a_distribution() {
        for mode in [
            RollMode::Normal,
            RollMode::Advantage,
            RollMode::Disadvantage,
        ] {
            let sum: f64 = mode.distribution().iter().sum();
            assert!(close(sum, 1.0), "{mode:?} sums to {sum}");
        }
    }

    /// The classic numbers: 39/400 to crit with advantage, 1/400 to fumble.
    #[test]
    fn advantage_and_disadvantage_mirror_each_other() {
        let adv = RollMode::Advantage.distribution();
        let dis = RollMode::Disadvantage.distribution();
        assert!(close(adv[19], 39.0 / 400.0));
        assert!(close(adv[0], 1.0 / 400.0));
        assert!(close(dis[19], 1.0 / 400.0));
        assert!(close(dis[0], 39.0 / 400.0));
        for k in 0..20 {
            assert!(close(adv[k], dis[19 - k]), "asymmetry at {k}");
        }
    }

    #[test]
    fn outcomes_partition_the_probability() {
        for ac in 5..=30 {
            for mode in [
                RollMode::Normal,
                RollMode::Advantage,
                RollMode::Disadvantage,
            ] {
                let o = outcomes(
                    &Attack::new(5, 1, 8, 3).with_mode(mode),
                    &Defense::new(ac, 20),
                );
                assert!(close(o.miss + o.hit + o.crit, 1.0), "ac {ac} {mode:?}");
                assert!(o.miss >= 0.0 && o.hit >= 0.0 && o.crit >= 0.0);
            }
        }
    }

    /// +5 against AC 15 needs a 10, so 10..19 hit normally and 20 crits.
    #[test]
    fn hit_chance_matches_a_hand_count() {
        let o = outcomes(&Attack::new(5, 1, 8, 3), &Defense::new(15, 20));
        assert!(close(o.hit, 10.0 / 20.0));
        assert!(close(o.crit, 1.0 / 20.0));
        assert!(close(o.miss, 9.0 / 20.0));
    }

    /// Even an impossible AC leaves the natural 20, and even a trivial one
    /// leaves the natural 1.
    #[test]
    fn natural_ones_and_twenties_override_the_arithmetic() {
        let impossible = outcomes(&Attack::new(0, 1, 6, 0), &Defense::new(40, 10));
        assert!(close(impossible.hit, 0.0));
        assert!(close(impossible.crit, 1.0 / 20.0));

        let trivial = outcomes(&Attack::new(20, 1, 6, 0), &Defense::new(1, 10));
        assert!(close(trivial.miss, 1.0 / 20.0), "a natural 1 always misses");
    }

    #[test]
    fn resistance_halves_and_rounds_down() {
        assert_eq!(Reduction::Resistant.apply(7), 3);
        assert_eq!(Reduction::Resistant.apply(8), 4);
        assert_eq!(Reduction::Resistant.apply(1), 0);
        assert_eq!(Reduction::Vulnerable.apply(7), 14);
    }

    /// A crit doubles the dice, not the modifier: 2d6+3 crits to 4d6+3, so the
    /// mean goes 10 -> 17, not 20.
    #[test]
    fn a_crit_doubles_dice_but_not_the_modifier() {
        let hit = Pmf::pool(2, 6).offset(3);
        let crit = Pmf::pool(4, 6).offset(3);
        assert!(close(hit.mean(), 10.0));
        assert!(close(crit.mean(), 17.0));
        assert_eq!(crit.min(), 7);
    }

    #[test]
    fn damage_pmf_is_a_distribution_with_no_negative_outcomes() {
        let attack = Attack::new(7, 2, 6, 4);
        let pmf = damage_pmf(&attack, &Defense::new(16, 30));
        assert!(close(pmf.total(), 1.0));
        assert!(pmf.min() >= 0, "damage floors at zero");
        assert!(pmf.prob(0) > 0.0, "a miss must be possible");
    }

    /// A big modifier against resistance is the case where flooring and
    /// halving interact, and the order matters: floor first, then halve.
    #[test]
    fn a_negative_modifier_floors_before_resistance_halves() {
        let attack = Attack::new(10, 1, 4, -6);
        let pmf = damage_pmf(
            &attack,
            &Defense::new(10, 10).with_reduction(Reduction::Resistant),
        );
        assert_eq!(pmf.min(), 0);
        assert!(close(pmf.total(), 1.0));
    }

    /// 1d4-8 cannot reach 1 even on a crit (2d4-8 tops out at zero), so this
    /// is structurally unkillable rather than merely unlucky - the assertion
    /// does not depend on the seed.
    #[test]
    fn an_attack_that_cannot_hurt_reports_survival_rather_than_hanging() {
        let attack = Attack::new(20, 1, 4, -8);
        let defense = Defense::new(1, 10);
        assert_eq!(
            damage_pmf(&attack, &defense).max(),
            0,
            "no damage is possible"
        );

        let mut rng = Rng::new(1);
        assert_eq!(
            sample_attacks_to_kill(&mut rng, &attack, &defense, 50),
            None
        );
    }
}
