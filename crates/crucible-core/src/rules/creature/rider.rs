//! Triggered modifiers and reactions (riders).

use super::damage::{DamageKind, DamageRoll};
use super::types::{Ability, Condition, Cost, Duration};
use crate::rules::combat::{Attack, DamageRider, RollMode};

/// What kind of incoming attack a [`Rider::ReactionOnTargeted`] answers.
///
/// One variant today, because nothing in the engine yet distinguishes a
/// melee attack roll from a ranged or spell one the way
/// [`Attack::finesse_or_ranged`] distinguishes weapon properties. The field
/// exists anyway so a future distinction - a reaction that only answers a
/// melee attack, say - is a new variant matched at the same call site, not a
/// new field threaded through every caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackTrigger {
    /// Any attack roll made against this creature.
    AnyAttack,
}

/// A triggered modifier.
///
/// Each variant is a mechanism, not a feature. The comments name the features
/// that map onto it, which is the test of whether the abstraction is pulling
/// its weight: a variant only one ability can use is a branch in disguise.
#[derive(Debug, Clone, PartialEq)]
pub enum Rider {
    /// On a hit, the target saves or takes a condition.
    ///
    /// Stunning Strike. Also every knockdown, every on-hit poison, and the
    /// secondary effect on most breath weapons.
    SaveOrCondition {
        ability: Ability,
        dc: i32,
        condition: Condition,
        duration: Duration,
        cost: Option<Cost>,
        /// Stunning Strike is once per turn however many times you hit.
        once_per_turn: bool,
    },
    /// A successful save against an effect that would deal half takes none
    /// instead, and a failed one takes half.
    ///
    /// Evasion, for the ability the effect names. Danger Sense and a rogue's
    /// Evasion are the same shape.
    NothingOnSuccess { ability: Ability },
    /// Turn a failed save into a success, a fixed number of times per fight.
    ///
    /// Legendary Resistance. A fighter's Indomitable is the same shape with
    /// one use and a reroll instead of a pass.
    AlwaysSucceed { uses: u32 },
    /// A reaction that reduces the damage of an incoming *attack* whose types
    /// include one of `kinds`.
    ///
    /// Deflect Attacks. Uncanny Dodge and Heavy Armor Master are variations.
    ReduceDamage {
        kinds: Vec<DamageKind>,
        roll: DamageRoll,
        /// Reactions refresh at the start of the creature's turn.
        per_round: u32,
    },
    /// A reaction spent on being *targeted* by an attack, before its hit or
    /// miss is finalized, that adds `ac_bonus` to this creature's AC against
    /// that one attack - capable of turning what would have been a hit into
    /// a miss.
    ///
    /// The mirror of [`Rider::ReduceDamage`]: that one reacts to an attack
    /// that already hit, on its damage; this one reacts to being targeted,
    /// before the roll against AC is decided. A reaction that boosts AC
    /// against a targeting attack - the Shield spell, a Ring of Protection's
    /// reactive bonus, and any homebrew item shaped the same way - is this
    /// mechanism; nothing here is specific to any one of them.
    ReactionOnTargeted {
        trigger: AttackTrigger,
        ac_bonus: i32,
        /// Reactions refresh at the start of the creature's turn, the same
        /// as [`Rider::ReduceDamage::per_round`].
        per_round: u32,
    },
    /// Extra damage dice on a hit, gated on the attack roll having advantage
    /// or an ally next to the target - and never at all if the attacker also
    /// has disadvantage, which overrides an ally in place. Spendable once per
    /// turn if `once_per_turn`.
    ///
    /// Sneak Attack. The gate itself is evaluated by whoever resolves the
    /// attack, from flags on the attack rather than derived geometry - see
    /// [`crate::rules::combat::Attack::ally_adjacent`] and
    /// [`crate::rules::combat::Attack::finesse_or_ranged`] for why. This
    /// variant only carries the dice pool and the once-per-turn budget, so
    /// anything else that shares the exact same gate is this variant too,
    /// not a new branch.
    ConditionalExtraDamage {
        dice_count: u32,
        dice_sides: u32,
        once_per_turn: bool,
    },
}

impl Rider {
    /// Riders with a budget need somewhere to count it down.
    pub fn initial_uses(&self) -> u32 {
        match self {
            Rider::AlwaysSucceed { uses } => *uses,
            Rider::ReduceDamage { per_round, .. } => *per_round,
            Rider::ReactionOnTargeted { per_round, .. } => *per_round,
            _ => 0,
        }
    }

    /// The [`DamageRider`] this rider contributes to `attack`, or `None` if
    /// it does not apply - either because this variant is not
    /// [`Rider::ConditionalExtraDamage`], or because its gate does not hold.
    ///
    /// The gate (Sneak Attack's, specifically): a finesse or ranged weapon,
    /// on a roll that is not at disadvantage, with either advantage or an
    /// ally next to the target. `used_this_turn` is the once-per-turn
    /// budget, tracked by the caller - the same shared per-creature flag
    /// `sim::duel` already keeps for Stunning-Strike-style riders (see
    /// `README.md`'s note that a creature gets one once-per-turn rider
    /// trigger per turn in total, not one per rider).
    ///
    /// Returns the *full* qualifying dice pool. A feature that spends part
    /// of it on something other than damage (Cunning Strike) reduces
    /// `dice_count` on the result before handing it to
    /// [`Attack::with_damage_rider`] - [`DamageRider`] is a plain count of
    /// dice, so rolling fewer of them is not a special case.
    pub fn extra_damage_for(&self, attack: &Attack, used_this_turn: bool) -> Option<DamageRider> {
        let Rider::ConditionalExtraDamage {
            dice_count,
            dice_sides,
            once_per_turn,
        } = self
        else {
            return None;
        };
        if *once_per_turn && used_this_turn {
            return None;
        }
        if !attack.finesse_or_ranged || attack.mode == RollMode::Disadvantage {
            return None;
        }
        if attack.mode == RollMode::Advantage || attack.ally_adjacent {
            Some(DamageRider::new(*dice_count, *dice_sides))
        } else {
            None
        }
    }
}
