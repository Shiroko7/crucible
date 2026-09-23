//! Moves - what a creature spends an action, bonus action or legendary
//! action on - and the pools and budgets that pay for them.

use crate::creature::{Creature, Effect, Reach, Rider};

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

/// Something that has to already be true before a move can be taken at all.
///
/// Distinct from what a move *costs* ([`Cost`], [`Uses`], a spell slot):
/// those are budgets, spent when the move is taken. This is a state of the
/// fight - something standing beside its summoner, a spell already on a
/// blade - without which the move has nothing to act on. Checked by
/// `sim::fight` wherever a move's legality is, so no policy can pick one and
/// no search can plan one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// At least `count` of the user's `which`th summon standing.
    ///
    /// Several moves can name the same summon with different counts - one
    /// double strikes for less than three striking together - and each is
    /// legal only while that many are up, so what the user can command is
    /// exactly what it has called up.
    Summon { which: usize, count: u32 },
    /// Fewer than `max` of the user's `which`th summon standing: the room to
    /// call up another one.
    SummonRoom { which: usize, max: u32 },
    /// The user's `which`th boon active - a blade whose enchantment can be
    /// let go of in a burst only while it is still lit.
    Boon { which: usize },
}

/// What taking a move uses up, beyond its own budget: the other half of
/// [`Requirement`], for a move that consumes the very thing it needed.
///
/// A double let go of in a burst, a blade's enchantment discharged in a
/// flash of light. Not a [`Cost`], which is points out of a pool: this is a
/// thing standing on the field or riding on its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spend {
    /// One of the user's `which`th summons - see
    /// [`crate::creature::Creature::summons`].
    Summon(usize),
    /// The user's `which`th boon, which ends - see
    /// [`crate::creature::Creature::boons`].
    Boon(usize),
}

/// Which of 5e's three spell components a cast needs: spoken words, a
/// gesture, a physical component.
///
/// Declared so that a restriction can name the one it actually stops rather
/// than stopping "spells". Silence takes away speech, not gesture, so it
/// blocks a Verbal cast and lets a purely Somatic one through; manacles or a
/// grapple would be the other way round. Without this the engine could only
/// say "no spells at all", which is wrong for every spell that has no Verbal
/// component.
///
/// A move that never declares any is read by [`Move::needs_verbal`] as
/// needing speech if it is a [`MoveKind::Spell`] - which is true of most
/// spells, and is what this engine did before components existed, so an
/// undeclared stat block keeps behaving exactly as it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Components {
    pub verbal: bool,
    pub somatic: bool,
    pub material: bool,
}

impl Components {
    /// Parse `v`, `s`, `m` in any order and any separator - `vsm`,
    /// `verbal, somatic`, `v s` - or `none` for a cast that needs nothing.
    pub fn parse(text: &str) -> Option<Self> {
        let lower = text.to_ascii_lowercase();
        if lower.split_whitespace().eq(["none"]) {
            return Some(Self::default());
        }
        let mut out = Self::default();
        for word in lower.split([',', ' ']).filter(|w| !w.is_empty()) {
            match word {
                "verbal" => out.verbal = true,
                "somatic" => out.somatic = true,
                "material" => out.material = true,
                // A bare run of letters: `vsm`, `vs`, `v`.
                other if other.chars().all(|c| matches!(c, 'v' | 's' | 'm')) => {
                    for c in other.chars() {
                        match c {
                            'v' => out.verbal = true,
                            's' => out.somatic = true,
                            _ => out.material = true,
                        }
                    }
                }
                _ => return None,
            }
        }
        Some(out)
    }

    /// `V, S, M` in the stat block's own words.
    pub fn name(self) -> String {
        let mut bits = Vec::new();
        if self.verbal {
            bits.push("V");
        }
        if self.somatic {
            bits.push("S");
        }
        if self.material {
            bits.push("M");
        }
        match bits.is_empty() {
            true => "no components".to_string(),
            false => bits.join(", "),
        }
    }
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
    /// Which components this cast needs, when the stat block says. `None`
    /// means it never said, and [`Move::needs_verbal`] falls back to "a spell
    /// speaks" - see [`Components`].
    pub components: Option<Components>,
    /// How many of the creature's legendary actions this takes, as a
    /// legendary action - "Costs 2 Actions", "(2 Points)". Read only there;
    /// 1 for every ordinary move.
    pub legendary_cost: u32,
    /// Which zones around a creature with a mouth this reaches - see
    /// [`crate::creature::Creature::mouth`]. [`Reach::Any`] for every
    /// ordinary move, and ignored on any creature without a mouth.
    pub reach: Reach,
    /// What has to already be true for this move to be taken - see
    /// [`Requirement`]. `None` for every ordinary move.
    pub requires: Option<Requirement>,
    /// What taking this move uses up beyond its own budget - see [`Spend`].
    /// `None` for every ordinary move.
    pub spends: Option<Spend>,
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
            components: None,
            legendary_cost: 1,
            reach: Reach::Any,
            requires: None,
            spends: None,
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
    /// Declare which components this cast needs - see [`Components`].
    pub fn with_components(mut self, components: Components) -> Self {
        self.components = Some(components);
        self
    }

    /// Does taking this move require speaking?
    ///
    /// The question Silence actually asks. A move that declared its
    /// components answers from them; one that never did is read as speaking
    /// if it is a spell, which is both true of most spells and exactly how
    /// this engine behaved before components existed.
    pub fn needs_verbal(&self) -> bool {
        match self.components {
            Some(c) => c.verbal,
            None => self.kind == MoveKind::Spell,
        }
    }

    pub fn with_bypasses_casting_restrictions(mut self) -> Self {
        self.bypasses_casting_restrictions = true;
        self
    }

    /// Take `n` legendary actions when used as one - see
    /// [`Move::legendary_cost`].
    pub fn with_legendary_cost(mut self, n: u32) -> Self {
        self.legendary_cost = n;
        self
    }

    /// Reach only these zones - see [`Move::reach`].
    pub fn with_reach(mut self, reach: Reach) -> Self {
        self.reach = reach;
        self
    }

    /// Takeable only while this holds - see [`Requirement`].
    pub fn requiring(mut self, requirement: Requirement) -> Self {
        self.requires = Some(requirement);
        self
    }

    /// Taking it uses something up - see [`Spend`].
    pub fn spending(mut self, spend: Spend) -> Self {
        self.spends = Some(spend);
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

/// What sets off a [`Reaction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactionTrigger {
    /// This creature has just given an enemy this condition, by any means -
    /// a whirlpool snapping at whoever it drags in
    /// ([`crate::rules::Condition::Pulled`]), a pounce on whoever it knocks
    /// down. The reaction answers that enemy.
    EnemyGains(crate::rules::Condition),
    /// Damage has just broken through this creature's damage threshold - see
    /// [`crate::creature::Rider::DamageThreshold`]. The reaction answers
    /// whoever dealt it.
    Breached,
    /// An attack of this kind has just hit this creature. The reaction
    /// answers whoever landed it - a storm answering whoever closes with its
    /// priest, a shell of spikes, a cloak of thorns.
    ///
    /// Carries a [`crate::creature::AttackTrigger`] rather than firing on
    /// anything at all, because every printed version of this says which
    /// blows it answers: "hits you with a melee attack" is the common one,
    /// and it is the same question a raised shield already asks of an
    /// incoming attack ([`crate::creature::Rider::ReactionOnTargeted`]), so
    /// it asks it with the same type.
    ///
    /// The mirror of that rider, and the reason both exist: a reactive Armor
    /// Class boost answers an attack *before* it is resolved and changes
    /// whether it lands, while this answers one that already has and does
    /// something back.
    Hit(crate::creature::AttackTrigger),
}

/// A reaction that is a move of its own - an attack, a burst forcing a save
/// - taken when its trigger happens, against the creature that set it off.
///
/// The reactions that only change numbers on an attack already in flight - a
/// raised AC, a cut to a hit's damage - are [`crate::creature::Rider`]s;
/// this is the kind that does something. All of them share the creature's
/// one reaction a round, and none can be taken while it is Incapacitated.
#[derive(Debug, Clone, PartialEq)]
pub struct Reaction {
    pub trigger: ReactionTrigger,
    pub action: Move,
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
