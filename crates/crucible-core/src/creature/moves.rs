//! Moves - what a creature spends an action, bonus action or legendary
//! action on - and the pools and budgets that pay for them.

use crate::creature::{Creature, Effect, Rider};

/// A pool several moves draw on: focus points, ki, sorcery points, superiority
/// dice. Distinct from [`crate::creature::Uses`], which is one move's private
/// budget.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Resource {
    pub name: String,
    pub max: u32,
}

/// What a move or rider takes out of a pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cost {
    /// Index into the creature's `resources`, resolved when the scenario is
    /// parsed so the hot path never compares strings.
    pub resource: usize,
    pub amount: u32,
}

/// How often a move can be taken, out of its own budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Uses {
    #[default]
    Unlimited,
    /// A fixed budget for the whole fight: a breath weapon's free uses, a
    /// once-per-day ability.
    Limited(u32),
    /// Spent on use, and back on a `d6` of at least this value rolled at the
    /// start of each of the creature's turns. `Recharge(5)` is the printed
    /// "Recharge 5-6", which is why a dragon's breath is certain on round one
    /// and intermittent afterwards.
    Recharge(u32),
}

/// What kind of activity a move represents, beyond its damage/effect shape.
///
/// Almost every move needs nothing here - `Standard` covers plain attacks,
/// stances and saves, and nothing reads this tag at all by default. The other
/// variants exist only so a plugin, or a condition's move gating, can
/// recognise "this move is RAW an Action" - `ObjectUse`/`MagicItem` for
/// `FastHandsPlugin`, which rejects anything tagged `Standard` or `Spell`
/// rather than silently promoting it - see `features::classes::rogue::thief` - and
/// `Spell`/`MagicItem` for a condition like
/// [`crate::rules::Condition::Suppressed`] that blocks casting and
/// item activation, via `sim::fight`'s move gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MoveKind {
    #[default]
    Standard,
    /// The Use an Object action - drinking a potion, retrieving a hidden
    /// blade, activating a non-magical object.
    ObjectUse,
    /// Activating a magic item that would otherwise cost the Magic action -
    /// a wand, a staff, most consumable magic items.
    MagicItem,
    /// Casting a spell. What it costs is separate from the tag -
    /// [`Move::spell_slot_level`] for a slot, [`Move::cost`] for a wand's
    /// charges - so a free cantrip and a slotted spell are both `Spell`.
    Spell,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    pub name: String,
    pub uses: Uses,
    /// Paid from a shared pool, on top of `uses`.
    pub cost: Option<Cost>,
    /// A spell slot level this move spends from the caster's own
    /// [`crate::rules::SpellSlots`], separate from `cost`: a
    /// caster's slots are nine independent counters rather than one named
    /// pool, so they are not representable as a [`Cost`]. `None` for
    /// anything that is not a spell.
    pub spell_slot_level: Option<u32>,
    /// Fire when this move hits.
    pub riders: Vec<Rider>,
    pub effect: Effect,
    /// Requires concentration: taking this move ends whatever the user was
    /// already concentrating on, before anything else happens - even if this
    /// cast goes on to land nothing. Whatever condition it then applies, on
    /// whichever targets, becomes the new thing concentration is
    /// maintaining; see `sim::fight` for how that is tracked and torn
    /// down. `false` for every ordinary move, which is why this defaults with
    /// the rest of [`Move::new`] rather than needing its own builder call.
    pub concentration: bool,
    /// What kind of activity this is; see [`MoveKind`]. Read by Fast Hands
    /// (which list a move may be promoted into) and by a condition that
    /// blocks casting or item use ([`crate::rules::Condition::blocks_magic`]).
    pub kind: MoveKind,
    /// Resolve this move before the same turn's action rather than after it.
    /// Only meaningful for a bonus action: Steady Aim is taken *before* the
    /// attack it is meant to help, while almost everything else a bonus
    /// action does - an off-hand strike, a Spiritual Weapon swing - follows
    /// the action. `false` for every ordinary move.
    pub before_action: bool,
    /// Set on a move-taking that spent a limited-use charge to be exempt from
    /// whatever casting-restriction mechanism might apply to it elsewhere -
    /// being silenced, unable to speak or gesture, and so on. `false` for
    /// every ordinary move.
    ///
    /// This engine has no restriction-checking condition to consult the flag
    /// against yet - that is a separate, independent mechanism's concern - so
    /// it exists here as a standalone primitive: whichever mechanism checks
    /// "can this creature cast right now" can read it once it exists, the same
    /// way a new [`crate::rules::Condition`] variant is added once and every
    /// call site the compiler can find is updated to consult it. See
    /// [`crate::features::spellcasting::BypassCastingRestrictionsPlugin`] for
    /// how a build grants a charge-gated option to set it, reusing
    /// [`Uses::Limited`] for the charge budget rather than inventing a
    /// parallel resource mechanism.
    pub bypasses_casting_restrictions: bool,
}

impl Move {
    pub fn new(name: impl Into<String>, effect: Effect) -> Self {
        Self {
            name: name.into(),
            uses: Uses::Unlimited,
            cost: None,
            spell_slot_level: None,
            riders: Vec::new(),
            effect,
            concentration: false,
            kind: MoveKind::Standard,
            before_action: false,
            bypasses_casting_restrictions: false,
        }
    }

    pub fn with_uses(mut self, uses: Uses) -> Self {
        self.uses = uses;
        self
    }

    pub fn with_cost(mut self, cost: Cost) -> Self {
        self.cost = Some(cost);
        self
    }

    /// Mark this move as spending one spell slot of `level` when cast. See
    /// [`Move::pay_spell_cost`].
    pub fn with_spell_slot(mut self, level: u32) -> Self {
        self.spell_slot_level = Some(level);
        self
    }

    pub fn with_rider(mut self, rider: Rider) -> Self {
        self.riders.push(rider);
        self
    }

    pub fn with_concentration(mut self) -> Self {
        self.concentration = true;
        self
    }

    pub fn with_kind(mut self, kind: MoveKind) -> Self {
        self.kind = kind;
        self
    }

    /// Resolve this move before the turn's action - see
    /// [`Move::before_action`].
    pub fn with_before_action(mut self) -> Self {
        self.before_action = true;
        self
    }

    /// Marks this specific move-taking as exempt from whatever
    /// casting-restriction mechanism might apply to it - see
    /// [`Move::bypasses_casting_restrictions`].
    pub fn with_bypasses_casting_restrictions(mut self) -> Self {
        self.bypasses_casting_restrictions = true;
        self
    }

    /// A move that spends nothing is one a hoarding policy will still take.
    pub fn is_free(&self) -> bool {
        matches!(self.uses, Uses::Unlimited)
            && self.cost.is_none()
            && self.spell_slot_level.is_none()
    }

    /// Spend this move's spell slot, if it has one, from `caster`'s own
    /// pool. `true` and no change for a move that costs no slot; `false` and
    /// no change if the slot it needs is not available - the same shape as
    /// [`crate::rules::SpellSlots::cast`], which this calls.
    pub fn pay_spell_cost(&self, caster: &mut Creature) -> bool {
        match self.spell_slot_level {
            None => true,
            Some(level) => caster.cast_spell(level),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A move with no `spell_slot_level` (every other move in the game so
    /// far) must not be affected by this: `pay_spell_cost` is a no-op that
    /// always succeeds.
    #[test]
    fn a_move_without_a_spell_slot_pays_nothing() {
        let mut caster = Creature::new("Fighter", 16, 40);
        let punch = Move::new(
            "Punch",
            Effect::Strikes {
                strike: crate::creature::Strike::new(
                    5,
                    vec![crate::rules::DamageRoll::new(
                        1,
                        4,
                        2,
                        crate::rules::DamageKind::Bludgeoning,
                    )],
                ),
                count: 1,
            },
        );
        assert!(punch.pay_spell_cost(&mut caster));
    }
}
