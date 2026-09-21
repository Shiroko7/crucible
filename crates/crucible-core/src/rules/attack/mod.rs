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

mod damage_rider;
mod modifier;

use crate::prob::{Pmf, Rng};
use crate::rules::{DamageKind, Reduction, RollMode};
pub use damage_rider::DamageRider;
use damage_rider::{riders_pmf, sample_riders};
use modifier::{modifier_pmf, sample_modifier_bonus};
pub use modifier::{resolve_mode, AttackModifier};

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
    /// Whether this attack is a spell attack roll, as opposed to a weapon
    /// attack - the roll-type gate a sneak-attack-style extra damage rider
    /// can be extended to accept alongside [`Attack::finesse_or_ranged`],
    /// for a build that grants that extension (see
    /// [`crate::creature::Rider::extra_damage_for_with_spell_attack_extension`]).
    /// `false` for a weapon attack and for anything that never asks.
    ///
    /// This is purely "was this attack resolved as a spell attack roll" -
    /// it has no bearing on saving-throw spells, which never build an
    /// [`Attack`] at all and so never reach this flag or the rider gate it
    /// feeds.
    pub is_spell_attack: bool,
    /// This attack's own damage type, when it is a spell attack whose damage
    /// type matters to something else on the attack - set only for spell
    /// attacks, and left `None` for a weapon attack (this model has no field
    /// for a weapon's damage type at all; nothing here has needed one yet).
    ///
    /// The 5e rule this exists for: a sneak-attack-style extra-damage rider
    /// triggered by a spell attack deals the *spell's* damage type, not
    /// whatever type the rider would otherwise default to. See
    /// [`DamageRider::kind`] and
    /// [`crate::creature::Rider::extra_damage_for_with_spell_attack_extension`],
    /// which reads this field and copies it onto the rider it returns.
    pub spell_damage_kind: Option<DamageKind>,
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
            is_spell_attack: false,
            spell_damage_kind: None,
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

    /// Mark this attack as a spell attack roll - see [`Attack::is_spell_attack`].
    pub fn with_is_spell_attack(mut self, is_spell_attack: bool) -> Self {
        self.is_spell_attack = is_spell_attack;
        self
    }

    /// Declare this attack's own damage type - see
    /// [`Attack::spell_damage_kind`].
    pub fn with_spell_damage_kind(mut self, kind: DamageKind) -> Self {
        self.spell_damage_kind = Some(kind);
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
#[derive(Debug, Clone)]
pub struct Defense {
    pub ac: i32,
    pub hp: i32,
    pub reduction: Reduction,
    /// Per-damage-type overrides of `reduction`, for a hit whose components
    /// are not all the same type - the reason this exists is
    /// [`DamageRider::kind`]: a rider pinned to a spell's own damage type
    /// needs a reduction that can differ from the rest of the attack's.
    ///
    /// A kind with no entry here falls back to `reduction`, so a plain
    /// [`Defense::new`] with no overrides reduces every damage source
    /// identically - exactly the behaviour before per-rider damage types
    /// existed. Looked up via [`Defense::reduction_for`], never read
    /// directly.
    pub kind_reductions: Vec<(DamageKind, Reduction)>,
}

impl Defense {
    pub fn new(ac: i32, hp: i32) -> Self {
        Self {
            ac,
            hp,
            reduction: Reduction::Normal,
            kind_reductions: Vec::new(),
        }
    }

    pub fn with_reduction(mut self, reduction: Reduction) -> Self {
        self.reduction = reduction;
        self
    }

    /// Resist, are vulnerable to, or are immune to `kind` specifically,
    /// regardless of what `reduction` says for everything else. Composable:
    /// call it again for another type.
    pub fn with_kind_reduction(mut self, kind: DamageKind, reduction: Reduction) -> Self {
        self.kind_reductions.push((kind, reduction));
        self
    }

    /// The effective [`Reduction`] for one damage component of this kind, or
    /// the attack's own [`Reduction`] when `kind` is `None` or has no entry
    /// in `kind_reductions` - see [`Defense::kind_reductions`] and
    /// [`DamageRider::kind`].
    pub fn reduction_for(&self, kind: Option<DamageKind>) -> Reduction {
        kind.and_then(|k| {
            self.kind_reductions
                .iter()
                .find(|&&(kk, _)| kk == k)
                .map(|&(_, r)| r)
        })
        .unwrap_or(self.reduction)
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

/// As [`hit_outcomes_with`], but the defender may spend a reaction to add
/// `ac_bonus` to its AC against this one attack before hit or miss is
/// finalized - [`crate::creature::Rider::ReactionOnTargeted`], a
/// reaction that boosts AC against a targeting attack (the Shield spell, or a
/// magic item shaped the same way), rather than reducing damage after a hit
/// already landed like [`crate::creature::Rider::ReduceDamage`].
///
/// Spending it is never worse than not: raising the AC needed to clear this
/// roll can only turn a hit into a miss, never a miss into a hit, and it
/// cannot touch a natural 20 - that always crits regardless of AC, same as
/// [`hit_outcomes_with`]. So whenever `available` is true, the whole
/// distribution is exactly what fighting against `ac + ac_bonus` would give;
/// `available` is decided by the caller - `sim::duel`, which tracks the
/// per-round budget - and is `false` once the reaction is already spent this
/// round.
pub fn hit_outcomes_with_reaction(
    to_hit: i32,
    mode: RollMode,
    ac: i32,
    modifiers: &[AttackModifier],
    ac_bonus: i32,
    available: bool,
) -> Outcomes {
    let effective_ac = if available { ac + ac_bonus } else { ac };
    hit_outcomes_with(to_hit, mode, effective_ac, modifiers)
}

/// The sampled counterpart of [`hit_outcomes_with_reaction`].
///
/// The d20 (and any modifier dice) is rolled exactly once - the reaction is
/// decided from that same roll rather than by rolling again, the same way
/// [`sample_hit_with`] never rolls twice for one attack. Returns whether the
/// reaction was actually spent alongside how the attack landed, so a caller
/// tracking a limited budget only debits it when it fired: never against a
/// miss (nothing to gain) or a natural-20 crit (nothing it can do), and
/// always against an attack that would otherwise land as an ordinary hit -
/// even when `ac_bonus` turns out not to be enough to save it, the same way
/// spending a reaction at the table does not refund it just because the
/// attack still connects.
pub fn sample_hit_with_reaction(
    rng: &mut Rng,
    to_hit: i32,
    mode: RollMode,
    ac: i32,
    modifiers: &[AttackModifier],
    ac_bonus: i32,
    available: bool,
) -> (Landed, bool) {
    let mode = resolve_mode(mode, modifiers);
    let roll = mode.roll(rng);
    let bonus = sample_modifier_bonus(rng, modifiers);
    if roll == 20 {
        return (Landed::Crit, false);
    }
    if roll == 1 || roll + to_hit + bonus < ac {
        return (Landed::Miss, false);
    }
    // An ordinary hit against the base AC: the reaction fires whenever it is
    // available, whether or not the boost ends up being enough.
    if !available {
        return (Landed::Hit, false);
    }
    if roll + to_hit + bonus < ac + ac_bonus {
        (Landed::Miss, true)
    } else {
        (Landed::Hit, true)
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

/// Exact distribution of the base attack's own dice and flat bonus alone,
/// floored and reduced by [`Defense::reduction`] - the piece [`damage_pmf`]
/// shares between a hit and a crit, and the reduction every rider without
/// its own [`DamageRider::kind`] also falls back to.
fn base_pmf(attack: &Attack, defense: &Defense, dice_count: u32) -> Pmf {
    Pmf::pool(dice_count, attack.dice_sides)
        .offset(attack.damage_bonus)
        .floor_at(0)
        .map_values(|d| defense.reduction.apply(d))
}

/// Exact distribution of damage dealt by a single attack, zero included.
///
/// The base pool and each rider are floored and reduced independently, then
/// summed - see `damage_rider::rider_component_pmf` for why that has to happen
/// before the convolution rather than after it.
pub fn damage_pmf(attack: &Attack, defense: &Defense) -> Pmf {
    let o = outcomes(attack, defense);

    let on_hit = base_pmf(attack, defense, attack.dice_count).convolve(&riders_pmf(
        &attack.damage_riders,
        false,
        defense,
    ));

    // A crit rolls the damage dice twice. The modifier is added once.
    let on_crit = base_pmf(attack, defense, attack.dice_count * 2).convolve(&riders_pmf(
        &attack.damage_riders,
        true,
        defense,
    ));

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
    let base_raw: i32 = (0..dice).map(|_| rng.die(attack.dice_sides)).sum();
    let base = defense
        .reduction
        .apply((base_raw + attack.damage_bonus).max(0));
    base + sample_riders(rng, &attack.damage_riders, crit, defense)
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

    /// [`Defense::reduction_for`] falls back to the attack's own `reduction`
    /// for any kind with no override - including `None`, a rider that never
    /// declared a kind at all - and only consults `kind_reductions` for a
    /// kind that actually has an entry there.
    #[test]
    fn defense_reduction_for_falls_back_to_the_base_reduction() {
        let defense = Defense::new(10, 20)
            .with_reduction(Reduction::Vulnerable)
            .with_kind_reduction(DamageKind::Radiant, Reduction::Resistant);
        assert_eq!(defense.reduction_for(None), Reduction::Vulnerable);
        assert_eq!(
            defense.reduction_for(Some(DamageKind::Force)),
            Reduction::Vulnerable,
            "a kind with no override still falls back to the base reduction"
        );
        assert_eq!(
            defense.reduction_for(Some(DamageKind::Radiant)),
            Reduction::Resistant
        );
    }

    /// +5 against AC 15 needs a 10 (see `hit_chance_matches_a_hand_count`).
    /// A +5 reactive AC boost raises that to needing a 15, so rolls 10..14
    /// move from the hit band into the miss band while the crit chance -
    /// pinned to the natural 20 - does not move at all.
    #[test]
    fn a_reactive_ac_boost_can_turn_some_hits_into_misses() {
        let plain = hit_outcomes_with_reaction(5, RollMode::Normal, 15, &[], 5, false);
        let boosted = hit_outcomes_with_reaction(5, RollMode::Normal, 15, &[], 5, true);
        assert!(close(plain.miss + plain.hit + plain.crit, 1.0));
        assert!(close(boosted.miss + boosted.hit + boosted.crit, 1.0));
        assert!(
            boosted.hit < plain.hit,
            "an available boost should convert some hits to misses"
        );
        assert!(
            close(boosted.crit, plain.crit),
            "a natural 20 crits regardless of the boost"
        );
        assert!(
            close(boosted.hit, 5.0 / 20.0),
            "needing a 15 instead of a 10 should leave exactly rolls 15..19 as hits"
        );
    }

    /// An unavailable reaction changes nothing: the boosted call has to fall
    /// back to exactly [`hit_outcomes_with`].
    #[test]
    fn an_unavailable_reaction_leaves_outcomes_unchanged() {
        let plain = outcomes(&Attack::new(5, 1, 8, 3), &Defense::new(15, 20));
        let unavailable = hit_outcomes_with_reaction(5, RollMode::Normal, 15, &[], 5, false);
        assert!(close(plain.miss, unavailable.miss));
        assert!(close(plain.hit, unavailable.hit));
        assert!(close(plain.crit, unavailable.crit));
    }

    #[test]
    fn sampled_reactive_ac_boost_agrees_with_the_exact_path() {
        for (seed, available) in [(0u64, false), (1u64, true)] {
            let exact = hit_outcomes_with_reaction(5, RollMode::Normal, 15, &[], 5, available);
            let mut rng = Rng::new(seed + 700);
            let n = 200_000;
            let (mut miss, mut hit, mut crit) = (0usize, 0usize, 0usize);
            for _ in 0..n {
                match sample_hit_with_reaction(&mut rng, 5, RollMode::Normal, 15, &[], 5, available)
                    .0
                {
                    Landed::Miss => miss += 1,
                    Landed::Hit => hit += 1,
                    Landed::Crit => crit += 1,
                }
            }
            let tol = |p: f64| 5.0 * (p * (1.0 - p) / n as f64).sqrt() + 1e-4;
            let check = |name: &str, want: f64, got_count: usize| {
                let got = got_count as f64 / n as f64;
                assert!(
                    (got - want).abs() < tol(want),
                    "available={available} {name}: sampled {got:.5}, exact {want:.5}"
                );
            };
            check("miss", exact.miss, miss);
            check("hit", exact.hit, hit);
            check("crit", exact.crit, crit);
        }
    }

    /// The reaction is only ever spent against a roll that would otherwise
    /// land as an ordinary hit against the *base* AC: never on a roll that
    /// was already going to miss (nothing to gain) and never on a natural 20
    /// (nothing the boost can do about it) - even though a converted roll
    /// still reports [`Landed::Miss`], the same as a roll that missed on its
    /// own. Two independently-seeded but identical RNG streams isolate the
    /// question: one decides the base outcome with no reaction in play at
    /// all, the other decides it with the reaction available, and both draw
    /// from the same die roll because [`sample_hit_with_reaction`] never
    /// rolls more than [`sample_hit_with`] does.
    #[test]
    fn the_reaction_only_fires_on_a_would_be_ordinary_hit() {
        let mut saw_a_consumed_hit = false;
        for seed in 0..2_000u64 {
            let mut base_rng = Rng::new(seed);
            let mut reaction_rng = Rng::new(seed);
            let base_landed = sample_hit_with(&mut base_rng, 5, RollMode::Normal, 15, &[]);
            let (landed, consumed) =
                sample_hit_with_reaction(&mut reaction_rng, 5, RollMode::Normal, 15, &[], 5, true);
            match base_landed {
                Landed::Miss => {
                    assert!(
                        !consumed,
                        "a roll that would already miss should not spend the reaction"
                    );
                    assert_eq!(landed, Landed::Miss);
                }
                Landed::Crit => {
                    assert!(!consumed, "a natural 20 should never spend the reaction");
                    assert_eq!(landed, Landed::Crit);
                }
                Landed::Hit => {
                    assert!(
                        consumed,
                        "an available reaction should always fire on a would-be ordinary hit"
                    );
                    saw_a_consumed_hit = true;
                }
            }
        }
        assert!(
            saw_a_consumed_hit,
            "an ordinary hit should have spent the reaction at least once in 2000 rolls"
        );
    }
}
