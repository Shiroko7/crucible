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
//!
//! [`AttackModifier`] and [`DamageRider`] are that plug point made explicit.
//! Neither is a single-slot field: an attack carries a `Vec` of each, so
//! Bless, a magic weapon's flat bonus and Sneak Attack's extra dice can all
//! be active on the same roll at once, from sources that never need to know
//! about one another. Whether a given modifier or rider applies to *this*
//! attack - Sneak Attack's once-per-turn condition, a dragonslaying weapon's
//! preference for dragons - is decided by whoever builds the list; leaving
//! one out is how "the condition did not hold" is expressed. Both are
//! resolved in the exact path (folded into a [`Pmf`] by convolution) and the
//! sampled path (rolled alongside everything else), because the project's
//! whole premise is that the two must keep agreeing.

use crate::prob::dice::Pmf;
use crate::prob::rng::Rng;

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

/// A modifier to an attack roll's resolution.
///
/// Every variant is a mechanism a whole family of abilities reduces to, not a
/// feature in itself: Bless is `BonusDice { count: 1, sides: 4 }`, Bane is
/// the same shape as a penalty, Steady Aim is `ForceAdvantage`, a +1 weapon
/// is `Flat(1)`. A list of these is carried alongside an attack rather than
/// a single optional one, so unrelated sources - Bless from one PC, a magic
/// weapon's bonus, Steady Aim - can all be active on the same roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackModifier {
    /// Extra dice added to the roll's total - Bless's `+1d4`. Rolled
    /// alongside the d20 and added to whatever it takes to clear the AC; it
    /// never touches whether the *natural* roll was a 1 or a 20.
    BonusDice { count: u32, sides: u32 },
    /// Dice subtracted from the roll's total - Bane's `-1d4`. The mirror of
    /// [`AttackModifier::BonusDice`].
    PenaltyDice { count: u32, sides: u32 },
    /// A flat bonus or penalty to the roll's total, for anything that is not
    /// dice - most simply, a magic weapon.
    Flat(i32),
    /// Forces advantage on this roll regardless of the situational mode -
    /// Steady Aim - subject to the same cancellation rule as everything
    /// else: one source of disadvantage anywhere still cancels it.
    ForceAdvantage,
    /// Forces disadvantage on this roll regardless of the situational mode.
    ForceDisadvantage,
}

/// The 5e stacking rule for advantage and disadvantage applied to a modifier
/// list: however many sources of each are present, they collapse to one flag
/// apiece, and one of each cancels to a flat roll. This is the same rule
/// `sim::duel` applies to a set of active conditions, applied here to a set
/// of modifiers instead.
pub fn resolve_mode(base: RollMode, modifiers: &[AttackModifier]) -> RollMode {
    let mut advantage = base == RollMode::Advantage;
    let mut disadvantage = base == RollMode::Disadvantage;
    for m in modifiers {
        match m {
            AttackModifier::ForceAdvantage => advantage = true,
            AttackModifier::ForceDisadvantage => disadvantage = true,
            _ => {}
        }
    }
    match (advantage, disadvantage) {
        (true, false) => RollMode::Advantage,
        (false, true) => RollMode::Disadvantage,
        _ => RollMode::Normal,
    }
}

/// Exact distribution of everything a modifier list adds to a roll's total,
/// besides the d20 itself - every [`AttackModifier::BonusDice`],
/// [`AttackModifier::PenaltyDice`] and [`AttackModifier::Flat`], combined by
/// convolution. Advantage and disadvantage do not appear here; they are
/// resolved separately by [`resolve_mode`].
fn modifier_pmf(modifiers: &[AttackModifier]) -> Pmf {
    modifiers.iter().fold(Pmf::constant(0), |acc, m| match *m {
        AttackModifier::BonusDice { count, sides } => acc.convolve(&Pmf::pool(count, sides)),
        AttackModifier::PenaltyDice { count, sides } => {
            acc.convolve(&Pmf::pool(count, sides).map_values(|v| -v))
        }
        AttackModifier::Flat(n) => acc.offset(n),
        AttackModifier::ForceAdvantage | AttackModifier::ForceDisadvantage => acc,
    })
}

/// The sampled counterpart of [`modifier_pmf`].
fn sample_modifier_bonus(rng: &mut Rng, modifiers: &[AttackModifier]) -> i32 {
    modifiers
        .iter()
        .map(|m| match *m {
            AttackModifier::BonusDice { count, sides } => (0..count).map(|_| rng.die(sides)).sum(),
            AttackModifier::PenaltyDice { count, sides } => {
                -(0..count).map(|_| rng.die(sides)).sum::<i32>()
            }
            AttackModifier::Flat(n) => n,
            AttackModifier::ForceAdvantage | AttackModifier::ForceDisadvantage => 0,
        })
        .sum()
}

/// Extra damage dice appended to a hit's damage, on top of an attack's own
/// pool - Sneak Attack, a dragonslaying weapon's bonus against its favoured
/// prey, a smite.
///
/// Doubled on a crit exactly like the attack's own dice: a critical hit in
/// 5e doubles all of the attack's damage dice, not only the weapon's.
/// `bonus` is a flat addition and, like [`Attack::damage_bonus`], is never
/// doubled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRider {
    pub dice_count: u32,
    pub dice_sides: u32,
    pub bonus: i32,
}

impl DamageRider {
    pub fn new(dice_count: u32, dice_sides: u32) -> Self {
        Self {
            dice_count,
            dice_sides,
            bonus: 0,
        }
    }

    pub fn with_bonus(mut self, bonus: i32) -> Self {
        self.bonus = bonus;
        self
    }
}

/// Exact distribution of every active rider's contribution to one hit's
/// damage, dice doubled on a crit exactly like the base pool.
fn rider_pmf(riders: &[DamageRider], crit: bool) -> Pmf {
    riders.iter().fold(Pmf::constant(0), |acc, r| {
        let dice = if crit { r.dice_count * 2 } else { r.dice_count };
        acc.convolve(&Pmf::pool(dice, r.dice_sides).offset(r.bonus))
    })
}

/// The sampled counterpart of [`rider_pmf`].
fn sample_riders(rng: &mut Rng, riders: &[DamageRider], crit: bool) -> i32 {
    riders
        .iter()
        .map(|r| {
            let dice = if crit { r.dice_count * 2 } else { r.dice_count };
            (0..dice).map(|_| rng.die(r.dice_sides)).sum::<i32>() + r.bonus
        })
        .sum()
}

/// Resistance halves and rounds down; vulnerability doubles; immunity zeroes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reduction {
    #[default]
    Normal,
    Resistant,
    Vulnerable,
    Immune,
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
            Reduction::Immune => 0,
        }
    }
}

/// One attack's offensive profile.
///
/// `attack_modifiers` and `damage_riders` are the generic hook: every
/// ability that touches the roll or appends bonus damage - present or
/// future - is a value pushed onto one of these lists, never a new field or
/// a new branch in [`outcomes`] or [`damage_pmf`].
#[derive(Debug, Clone)]
pub struct Attack {
    pub to_hit: i32,
    pub dice_count: u32,
    pub dice_sides: u32,
    /// Added once on a hit and once on a crit - never doubled.
    pub damage_bonus: i32,
    pub mode: RollMode,
    /// Active modifiers to the attack roll, from as many unrelated sources
    /// as apply at once - see [`AttackModifier`].
    pub attack_modifiers: Vec<AttackModifier>,
    /// Active extra damage dice, from as many unrelated sources as apply at
    /// once - see [`DamageRider`].
    pub damage_riders: Vec<DamageRider>,
    /// Whether the weapon used for this attack has the finesse or ranged
    /// property - the weapon-side gate 2024 Sneak Attack needs alongside
    /// [`Attack::ally_adjacent`]. `false` for anything else, and irrelevant
    /// to a creature that has no feature asking about it.
    pub finesse_or_ranged: bool,
    /// Whether an ally is within 5 feet of this attack's target.
    ///
    /// The engine has no positioning model (see `DESIGN.md`'s "Positioning
    /// is the gap that matters"), so this cannot be derived from geometry.
    /// It is an explicit flag on the attack itself instead - set per-attack,
    /// or held fixed for a whole scenario - the same way [`Attack::mode`]
    /// stands in for whether *this* creature happens to have advantage
    /// rather than deriving it from who is prone or flanking. Sneak Attack
    /// is the first feature that reads it; anything later that also keys off
    /// "an ally is next to the target" sets the same flag rather than
    /// growing a second one.
    pub ally_adjacent: bool,
}

impl Attack {
    pub fn new(to_hit: i32, dice_count: u32, dice_sides: u32, damage_bonus: i32) -> Self {
        Self {
            to_hit,
            dice_count,
            dice_sides,
            damage_bonus,
            mode: RollMode::Normal,
            attack_modifiers: Vec::new(),
            damage_riders: Vec::new(),
            finesse_or_ranged: false,
            ally_adjacent: false,
        }
    }

    pub fn with_mode(mut self, mode: RollMode) -> Self {
        self.mode = mode;
        self
    }

    /// Mark this attack as made with a finesse or ranged weapon - see
    /// [`Attack::finesse_or_ranged`].
    pub fn with_finesse_or_ranged(mut self, finesse_or_ranged: bool) -> Self {
        self.finesse_or_ranged = finesse_or_ranged;
        self
    }

    /// Mark that an ally is within 5 feet of this attack's target - see
    /// [`Attack::ally_adjacent`].
    pub fn with_ally_adjacent(mut self, ally_adjacent: bool) -> Self {
        self.ally_adjacent = ally_adjacent;
        self
    }

    /// Add one attack modifier. Composable: call it again for a second,
    /// unrelated source and both apply to the same roll.
    pub fn with_attack_modifier(mut self, modifier: AttackModifier) -> Self {
        self.attack_modifiers.push(modifier);
        self
    }

    /// Add one damage rider. Composable, like [`Attack::with_attack_modifier`].
    pub fn with_damage_rider(mut self, rider: DamageRider) -> Self {
        self.damage_riders.push(rider);
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

/// How an attack roll landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landed {
    Miss,
    Hit,
    Crit,
}

/// Exact probabilities of miss, ordinary hit and critical hit.
///
/// Taken apart from [`Attack`] because a strike with several damage
/// components - a dragon's claw dealing slashing *and* fire - resolves against
/// exactly these rules and must not own a second copy of them.
pub fn hit_outcomes(to_hit: i32, mode: RollMode, ac: i32) -> Outcomes {
    hit_outcomes_with(to_hit, mode, ac, &[])
}

/// As [`hit_outcomes`], with [`AttackModifier`]s applied.
///
/// A natural 1 always misses and a natural 20 always crits regardless of
/// what is stacked on top of it - that is a property of the raw d20, decided
/// before any modifier is even consulted. Everything else a roll of 2..=19
/// needs to clear the AC is folded into one distribution via [`modifier_pmf`]
/// and compared against what is still needed.
pub fn hit_outcomes_with(
    to_hit: i32,
    mode: RollMode,
    ac: i32,
    modifiers: &[AttackModifier],
) -> Outcomes {
    let mode = resolve_mode(mode, modifiers);
    let dist = mode.distribution();
    let bonus = modifier_pmf(modifiers);
    let mut hit = 0.0;
    for (i, &p) in dist.iter().enumerate() {
        let roll = i as i32 + 1;
        // 1 and 20 are handled outside the loop; they ignore the arithmetic.
        if roll > 1 && roll < 20 {
            hit += p * bonus.at_least(ac - roll - to_hit);
        }
    }
    let crit = dist[19];
    Outcomes {
        miss: 1.0 - hit - crit,
        hit,
        crit,
    }
}

/// The sampled counterpart of [`hit_outcomes`].
pub fn sample_hit(rng: &mut Rng, to_hit: i32, mode: RollMode, ac: i32) -> Landed {
    sample_hit_with(rng, to_hit, mode, ac, &[])
}

/// The sampled counterpart of [`hit_outcomes_with`].
///
/// The modifier bonus is rolled unconditionally, same as at the table -
/// Bless's d4 is rolled alongside the d20 whether or not the natural roll
/// turns out to make it irrelevant - so which branch is taken never changes
/// how many draws this consumes.
pub fn sample_hit_with(
    rng: &mut Rng,
    to_hit: i32,
    mode: RollMode,
    ac: i32,
    modifiers: &[AttackModifier],
) -> Landed {
    let mode = resolve_mode(mode, modifiers);
    let roll = mode.roll(rng);
    let bonus = sample_modifier_bonus(rng, modifiers);
    if roll == 20 {
        Landed::Crit
    } else if roll != 1 && roll + to_hit + bonus >= ac {
        Landed::Hit
    } else {
        Landed::Miss
    }
}

/// Exact probabilities of miss, ordinary hit and critical hit.
pub fn outcomes(attack: &Attack, defense: &Defense) -> Outcomes {
    hit_outcomes_with(
        attack.to_hit,
        attack.mode,
        defense.ac,
        &attack.attack_modifiers,
    )
}

/// Exact distribution of damage dealt by a single attack, zero included.
pub fn damage_pmf(attack: &Attack, defense: &Defense) -> Pmf {
    let o = outcomes(attack, defense);
    let reduce = defense.reduction;

    let on_hit = Pmf::pool(attack.dice_count, attack.dice_sides)
        .convolve(&rider_pmf(&attack.damage_riders, false))
        .offset(attack.damage_bonus)
        .floor_at(0)
        .map_values(|d| reduce.apply(d));

    // A crit rolls the damage dice twice. The modifier is added once.
    let on_crit = Pmf::pool(attack.dice_count * 2, attack.dice_sides)
        .convolve(&rider_pmf(&attack.damage_riders, true))
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
    let landed = sample_hit_with(
        rng,
        attack.to_hit,
        attack.mode,
        defense.ac,
        &attack.attack_modifiers,
    );
    let (dice, crit) = match landed {
        Landed::Miss => return 0,
        Landed::Hit => (attack.dice_count, false),
        Landed::Crit => (attack.dice_count * 2, true),
    };
    let raw: i32 = (0..dice).map(|_| rng.die(attack.dice_sides)).sum::<i32>()
        + sample_riders(rng, &attack.damage_riders, crit);
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
        assert_eq!(Reduction::Immune.apply(7), 0);
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

    /// Bless adds `+1d4` to the roll, so hitting AC 15 with `+5` now succeeds
    /// on some rolls as low as 6 that used to miss - but a natural 1 still
    /// always misses and a natural 20 still always crits, bonus die or not.
    #[test]
    fn bless_widens_the_natural_rolls_that_hit() {
        let plain = outcomes(&Attack::new(5, 1, 8, 3), &Defense::new(15, 20));
        let blessed = outcomes(
            &Attack::new(5, 1, 8, 3)
                .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 }),
            &Defense::new(15, 20),
        );
        assert!(close(blessed.miss + blessed.hit + blessed.crit, 1.0));
        assert!(
            blessed.hit + blessed.crit > plain.hit + plain.crit,
            "a +1d4 should only ever raise the chance to hit"
        );
        assert!(
            blessed.miss < plain.miss,
            "some rolls that used to miss should now clear the AC"
        );
        assert!(
            blessed.miss >= 1.0 / 20.0 - 1e-12,
            "a natural 1 still always misses under Bless"
        );
        assert!(
            close(blessed.crit, 1.0 / 20.0),
            "Bless does not change the crit chance, only whether a lower roll hits"
        );
    }

    /// Bane is the mirror: `-1d4` on the roll can only ever cost hits, never
    /// gain them, and a natural 20 still always crits.
    #[test]
    fn bane_narrows_the_natural_rolls_that_hit() {
        let plain = outcomes(&Attack::new(5, 1, 8, 3), &Defense::new(15, 20));
        let baned = outcomes(
            &Attack::new(5, 1, 8, 3)
                .with_attack_modifier(AttackModifier::PenaltyDice { count: 1, sides: 4 }),
            &Defense::new(15, 20),
        );
        assert!(close(baned.miss + baned.hit + baned.crit, 1.0));
        assert!(
            baned.hit + baned.crit < plain.hit + plain.crit,
            "a -1d4 should only ever lower the chance to hit"
        );
        assert!(
            close(baned.crit, 1.0 / 20.0),
            "Bane does not change the crit chance, a natural 20 always lands"
        );
    }

    /// Several unrelated modifiers on one roll compose rather than replace
    /// each other: Bless and a +1 weapon both apply, and Bless plus Bane
    /// still leaves the flat bonus in effect.
    #[test]
    fn several_attack_modifiers_stack() {
        let one = outcomes(
            &Attack::new(5, 1, 8, 3)
                .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 }),
            &Defense::new(15, 20),
        );
        let both = outcomes(
            &Attack::new(5, 1, 8, 3)
                .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 })
                .with_attack_modifier(AttackModifier::Flat(1)),
            &Defense::new(15, 20),
        );
        assert!(
            both.hit + both.crit > one.hit + one.crit,
            "stacking a flat +1 on top of Bless should hit more, not the same"
        );

        // Bless and Bane together: the dice do not cancel algebraically (a
        // +1d4 and a -1d4 are not the same distribution as +0), but the flat
        // bonus underneath is still exactly what it was.
        let cancelled = outcomes(
            &Attack::new(5, 1, 8, 3)
                .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 })
                .with_attack_modifier(AttackModifier::PenaltyDice { count: 1, sides: 4 }),
            &Defense::new(15, 20),
        );
        assert!(close(cancelled.miss + cancelled.hit + cancelled.crit, 1.0));
    }

    /// Forced advantage and forced disadvantage from unrelated sources cancel
    /// to a flat roll, the same rule the duel layer applies to conditions.
    #[test]
    fn forced_advantage_and_disadvantage_cancel() {
        assert_eq!(
            resolve_mode(
                RollMode::Normal,
                &[
                    AttackModifier::ForceAdvantage,
                    AttackModifier::ForceDisadvantage,
                ],
            ),
            RollMode::Normal
        );
        assert_eq!(
            resolve_mode(RollMode::Disadvantage, &[AttackModifier::ForceAdvantage]),
            RollMode::Normal
        );
        assert_eq!(
            resolve_mode(RollMode::Normal, &[AttackModifier::ForceAdvantage]),
            RollMode::Advantage
        );
    }

    /// A damage rider appends dice on a hit, doubles on a crit like the
    /// attack's own dice, and vanishes entirely from a miss - and whether it
    /// is present at all is exactly how "the condition held" is expressed,
    /// with no predicate machinery needed inside `damage_pmf` itself.
    #[test]
    fn a_damage_rider_only_adds_dice_on_a_hit_and_doubles_on_a_crit() {
        let defense = Defense::new(1, 30); // AC 1: every non-fumble roll hits
        let plain = damage_pmf(&Attack::new(5, 1, 6, 0), &defense);
        let sneak_attack = damage_pmf(
            &Attack::new(5, 1, 6, 0).with_damage_rider(DamageRider::new(2, 6)),
            &defense,
        );
        assert!(close(sneak_attack.total(), 1.0));
        // Miss (a natural 1) is unaffected: the rider never fires.
        assert!(close(sneak_attack.prob(0), plain.prob(0)));
        // A rider absent altogether cannot be conditionally active on the
        // predicate not holding; leaving it out of the list is that "off".
        assert_eq!(
            damage_pmf(&Attack::new(5, 1, 6, 0), &defense).prob(0),
            plain.prob(0)
        );
        assert!(
            sneak_attack.mean() > plain.mean(),
            "extra dice on a hit should raise the mean damage"
        );
        // The base die is 1d6 (max 6); with a crit doubling both pools, the
        // maximum possible is 2*6 (base) + 2*2*6 (rider) = 36.
        assert_eq!(sneak_attack.max(), 2 * 6 + 2 * 2 * 6);
    }

    #[test]
    fn sampled_bless_bane_and_a_rider_agree_with_the_exact_path() {
        let defense = Defense::new(15, 40);
        let cases = [
            (
                "bless",
                Attack::new(5, 1, 8, 3)
                    .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 }),
            ),
            (
                "bane",
                Attack::new(5, 1, 8, 3)
                    .with_attack_modifier(AttackModifier::PenaltyDice { count: 1, sides: 4 }),
            ),
            (
                "sneak attack rider",
                Attack::new(5, 1, 8, 3).with_damage_rider(DamageRider::new(2, 6)),
            ),
            (
                "bless and a rider together",
                Attack::new(5, 1, 8, 3)
                    .with_attack_modifier(AttackModifier::BonusDice { count: 1, sides: 4 })
                    .with_damage_rider(DamageRider::new(2, 6)),
            ),
        ];
        for (seed, (name, attack)) in cases.into_iter().enumerate() {
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 500);
            let n = 100_000;
            let mut counts = vec![0usize; (hi - lo + 1) as usize];
            for _ in 0..n {
                let d = sample_damage(&mut rng, &attack, &defense);
                assert!(
                    d >= lo && d <= hi,
                    "{name}: sampled {d} outside {lo}..={hi}"
                );
                counts[(d - lo) as usize] += 1;
            }
            for (i, &c) in counts.iter().enumerate() {
                let value = lo + i as i32;
                let want = exact.prob(value);
                let got = c as f64 / n as f64;
                let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
                assert!(
                    (got - want).abs() < tol,
                    "{name}: P(damage = {value}) sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
                );
            }
        }
    }
}
