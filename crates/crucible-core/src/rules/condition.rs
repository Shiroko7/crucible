//! Conditions, and how long an applied one lasts.

use crate::rules::Ability;

/// A condition, in the 5e sense: a named bundle of effects with a lifetime.
///
/// Conditions are a system rather than a set of flags, which `DESIGN.md` calls
/// out as expensive to retrofit. This is the small version of that system: the
/// questions the duel needs to ask are methods here, so adding a condition
/// means adding a variant and letting the compiler find every site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Condition {
    /// No actions, bonus actions, reactions or legendary actions. Attacks
    /// against it have advantage, and Strength and Dexterity saves fail flat.
    Stunned,
    /// The Dodge action: attacks against it have disadvantage.
    Dodging,
    /// Melee attacks against it have advantage, and its own attacks have
    /// disadvantage.
    Prone,
    /// Disadvantage on its own attack rolls. The SRD also gives it
    /// disadvantage on ability checks, which is not modelled: nothing here
    /// rolls one, the same gap that leaves Blinded's sight-based checks and
    /// Poisoned's own out of scope until an ability-check mechanic exists.
    Poisoned,
    /// Disadvantage on its own attack rolls; attacks against it have
    /// advantage. Auto-failing a sight-based check is real 5e text with
    /// nowhere to attach: see [`Condition::Poisoned`].
    Blinded,
    /// Incapacitated (no actions, bonus actions, reactions or legendary
    /// actions), auto-fails Strength and Dexterity saves, and attacks against
    /// it have advantage - the same three as Stunned. What it adds is
    /// [`Condition::auto_crits`]: a hit landed against it is an automatic
    /// critical.
    Paralyzed,
    /// Can't hear. Carries none of Blinded's combat modifiers - nothing here
    /// rolls a hearing-based check any more than an ability check, so this is
    /// tracked for provenance (Blindness/Deafness names it explicitly as the
    /// caster's alternative choice to Blinded) without changing anything a
    /// duel resolves. The same gap [`Condition::Poisoned`] and
    /// [`Condition::Blinded`] already note.
    Deafened,
    /// Obeys, or resists, a directive on its own very next turn - Command's
    /// mechanism, and Suggestion's and Dominate's if they are ever added. Not
    /// itself a named SRD condition, the same way [`Condition::Dodging`]
    /// names a stance rather than a PHB condition: it is the engine's handle
    /// on "loses this turn to a compulsion" as its own mechanism, distinct
    /// from Incapacitated ([`Condition::incapacitated`]) because it carries
    /// none of that condition's side effects - attacks against a compelled
    /// creature gain no advantage, it does not auto-fail Strength or
    /// Dexterity saves, and (see `sim::fight::Fight::legendary`) it does not
    /// take away legendary actions, since Command's text only ever reaches
    /// the target's own next turn.
    Compelled,
    /// Steady Aim (2024 Rogue 3): advantage on the creature's own next attack
    /// roll this turn, and its speed drops to 0 for the rest of the turn.
    ///
    /// The advantage half is [`Condition::advantage_on_attacks`], the mirror
    /// of [`Condition::disadvantage_on_attacks`], and like [`Condition::Marked`]
    /// it is used up by the attack roll it helps - see `sim::fight`'s attack
    /// resolution. The speed half is [`Condition::zeroes_speed`] - there is
    /// no movement model here to apply it against (see `DESIGN.md`'s
    /// "Positioning is the gap that matters"), so it is exposed generically
    /// rather than acted on, ready for whenever one exists.
    SteadyAim,
    /// Cannot cast a spell or activate a magic item
    /// ([`Condition::blocks_magic`]), has disadvantage on every saving throw
    /// it makes ([`Condition::disadvantage_on_save`]), and any damage it
    /// deals - of any type, to anyone - is halved
    /// ([`Condition::halves_own_damage`]).
    ///
    /// The bundle a limited-use item's forced save applies on a failure (see
    /// [`crate::features::items::LimitedUseDebuffItemPlugin`]), kept as one
    /// condition rather than three separately-tracked effects because all
    /// three share exactly one applier, one victim and one duration. Unlike
    /// Stunned or Paralyzed, this does **not** incapacitate: the creature
    /// still gets its turn, its ordinary attacks, and every move that is not
    /// tagged [`crate::creature::MoveKind::Spell`] or
    /// [`crate::creature::MoveKind::MagicItem`].
    Suppressed,
    /// Outlined by a fading light: no effect on its own actions or saves, but
    /// the next attack roll made against it - by anyone, not only whoever
    /// applied it - has advantage.
    ///
    /// Guiding Bolt's mark. Unlike most conditions here, it is not cleared
    /// only at a turn boundary: `sim::fight`'s attack resolution consumes it
    /// the moment that next roll happens, so it clears whichever comes first -
    /// a roll against its holder, or the end of the applier's next turn (its
    /// [`Duration::ApplierNextTurnEnd`] expiry, for the "never got attacked"
    /// case).
    Marked,
    /// Disadvantage on this creature's own saving throws of one ability, and
    /// nothing else - what an injury poison leaves behind on a failed save
    /// (see [`crate::creature::Rider::InjuryPoison`]). Not a named SRD
    /// condition, the same way [`Condition::Compelled`] is not: it is the
    /// engine's handle on "saves of this one kind are burdened" so that
    /// lifetime, stacking and clearing reuse the condition machinery rather
    /// than a parallel list.
    SaveDisadvantage(Ability),
    /// Cannot cast a spell that has to be spoken - the Silence spell's
    /// sphere, a gag. Spell components are not modelled here, so every spell
    /// counts as needing a voice: this blocks every
    /// [`crate::creature::MoveKind::Spell`] move except one taken with
    /// [`crate::creature::Move::bypasses_casting_restrictions`]
    /// (casting without components - Tricky Spells, Subtle Spell). See
    /// [`Condition::blocks_casting`].
    Silenced,
}

impl Condition {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "stunned" => Self::Stunned,
            "dodging" | "dodge" => Self::Dodging,
            "prone" => Self::Prone,
            "poisoned" => Self::Poisoned,
            "blinded" => Self::Blinded,
            "paralyzed" | "paralysed" => Self::Paralyzed,
            "deafened" | "deafen" => Self::Deafened,
            "compelled" | "compel" => Self::Compelled,
            "steady_aim" | "steady aim" => Self::SteadyAim,
            "suppressed" => Self::Suppressed,
            "marked" => Self::Marked,
            "silenced" => Self::Silenced,
            "disadvantage_str_saves" => Self::SaveDisadvantage(Ability::Str),
            "disadvantage_dex_saves" => Self::SaveDisadvantage(Ability::Dex),
            "disadvantage_con_saves" => Self::SaveDisadvantage(Ability::Con),
            "disadvantage_int_saves" => Self::SaveDisadvantage(Ability::Int),
            "disadvantage_wis_saves" => Self::SaveDisadvantage(Ability::Wis),
            "disadvantage_cha_saves" => Self::SaveDisadvantage(Ability::Cha),
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Stunned => "stunned",
            Self::Dodging => "dodging",
            Self::Prone => "prone",
            Self::Poisoned => "poisoned",
            Self::Blinded => "blinded",
            Self::Paralyzed => "paralyzed",
            Self::Deafened => "deafened",
            Self::Compelled => "compelled",
            Self::SteadyAim => "steady_aim",
            Self::Suppressed => "suppressed",
            Self::Marked => "marked",
            Self::Silenced => "silenced",
            Self::SaveDisadvantage(ability) => match ability {
                Ability::Str => "disadvantage_str_saves",
                Ability::Dex => "disadvantage_dex_saves",
                Ability::Con => "disadvantage_con_saves",
                Ability::Int => "disadvantage_int_saves",
                Ability::Wis => "disadvantage_wis_saves",
                Ability::Cha => "disadvantage_cha_saves",
            },
        }
    }

    /// Can the creature act at all? Stunned and Paralyzed both include
    /// Incapacitated, which is what takes away legendary actions as well as
    /// the turn.
    pub fn incapacitated(self) -> bool {
        matches!(self, Self::Stunned | Self::Paralyzed)
    }

    /// Does an attacker striking this creature get advantage?
    ///
    /// Prone grants it only to melee attackers; reach and positioning do not
    /// exist here, so every attack is treated as melee.
    pub fn advantage_to_attackers(self) -> bool {
        matches!(
            self,
            Self::Stunned | Self::Prone | Self::Blinded | Self::Paralyzed | Self::Marked
        )
    }

    pub fn disadvantage_to_attackers(self) -> bool {
        matches!(self, Self::Dodging)
    }

    /// Does this creature's own attack roll suffer?
    pub fn disadvantage_on_attacks(self) -> bool {
        matches!(self, Self::Prone | Self::Poisoned | Self::Blinded)
    }

    /// Does this creature's own attack roll benefit? Steady Aim - the mirror
    /// of [`Condition::disadvantage_on_attacks`], and cancelled by it the same
    /// way any other advantage and disadvantage cancel.
    pub fn advantage_on_attacks(self) -> bool {
        matches!(self, Self::SteadyAim)
    }

    /// Does this condition drop the creature's speed to 0? Steady Aim.
    /// Nothing reads this yet - there is no movement model in this engine -
    /// so this exists purely to expose the flag for whenever one shows up,
    /// rather than leaving Steady Aim's speed clause unmodelled entirely.
    pub fn zeroes_speed(self) -> bool {
        matches!(self, Self::SteadyAim)
    }

    pub fn auto_fails(self, ability: Ability) -> bool {
        matches!(self, Self::Stunned | Self::Paralyzed)
            && matches!(ability, Ability::Str | Ability::Dex)
    }

    /// Evasion is explicitly unavailable while Incapacitated.
    pub fn blocks_riders(self) -> bool {
        self.incapacitated()
    }

    /// Does a hit against this creature land as an automatic critical?
    ///
    /// Paralyzed's actual text is "within 5 feet"; there is no positioning
    /// model to test that against, so - the same call Prone already makes by
    /// treating every attacker as melee - every hit is treated as being close
    /// enough.
    pub fn auto_crits(self) -> bool {
        matches!(self, Self::Paralyzed)
    }

    /// Blocks the two RAW action categories a debuff like this one takes
    /// away: casting a spell
    /// ([`crate::creature::MoveKind::Spell`]) and activating a magic
    /// item ([`crate::creature::MoveKind::MagicItem`]). Whoever
    /// selects a move checks this before taking one of either kind - see
    /// `sim::fight`'s move gating - this only answers the question.
    pub fn blocks_magic(self) -> bool {
        matches!(self, Self::Suppressed)
    }

    /// Blocks casting a spell the ordinary way, but not a cast made without
    /// components ([`crate::creature::Move::bypasses_casting_restrictions`]).
    /// Unlike [`Condition::blocks_magic`], which stops casting outright.
    pub fn blocks_casting(self) -> bool {
        matches!(self, Self::Silenced)
    }

    /// Does this creature roll an `ability` saving throw at disadvantage?
    /// [`Condition::Suppressed`] burdens every save,
    /// [`Condition::SaveDisadvantage`] only its own ability's. Composed with
    /// any other source of advantage or disadvantage on a save via the usual
    /// 5e cancellation rule rather than overriding it - see
    /// `sim::fight::save_mode`, the same stacking `attack_mode` already
    /// applies to attack rolls.
    pub fn disadvantage_on_save(self, ability: Ability) -> bool {
        match self {
            Self::Suppressed => true,
            Self::SaveDisadvantage(burdened) => burdened == ability,
            _ => false,
        }
    }

    /// Halves this creature's own outgoing damage, of any type, against
    /// anyone it attacks - the attacker-side counterpart to
    /// [`crate::rules::Reduction`], which only ever halves by the
    /// *target's* damage type. See `sim::fight::halve_if_suppressed`.
    pub fn halves_own_damage(self) -> bool {
        matches!(self, Self::Suppressed)
    }
}

/// How long an applied condition lasts.
///
/// Several variants because the real wordings differ and the difference
/// matters: Stunning Strike lasts "until the start of *your* next turn", so the
/// stunner's turn ends it; something a victim shakes off - standing up from
/// Prone - ends at the start of the victim's own turn; Guiding Bolt's mark
/// lasts "until the end of your next turn"; a spell that reads "at the end of
/// each of its turns, the target can make a save" - Hold Person, most poisons -
/// does not expire on a schedule at all, but on a repeated die roll that can
/// succeed the very turn it was applied; and "for 1 minute" with no save at all
/// is a plain count of rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Duration {
    /// Until the start of the next turn of whoever applied it.
    ApplierTurn,
    /// Until the start of the victim's next turn.
    VictimTurn,
    /// Until the *end* of the applier's next turn - Guiding Bolt's "before
    /// the end of your next turn". Applied during the applier's own turn, it
    /// survives that turn's end and clears at the end of the one after;
    /// applied at any other moment, it clears at the end of the applier's
    /// very next turn.
    ApplierNextTurnEnd,
    /// A fixed number of rounds, counted down at the start of each of the
    /// applier's turns - "for 1 minute" with no repeated save is `Rounds(10)`.
    Rounds(u32),
    /// Repeats `ability` against `dc` at the end of the victim's own turn,
    /// clearing the condition on a success. Kept as data on the duration
    /// rather than a new engine branch, the same way `SaveOrCondition` keeps
    /// the on-hit save as data: whatever applies the condition - a rider, a
    /// save effect - just names the ability and DC once, and the repeat lives
    /// entirely in the duel's turn-end processing.
    SaveEndTurn { ability: Ability, dc: i32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions_answer_the_questions_the_duel_asks() {
        assert!(Condition::Stunned.incapacitated());
        assert!(Condition::Stunned.advantage_to_attackers());
        assert!(Condition::Stunned.auto_fails(Ability::Dex));
        assert!(!Condition::Stunned.auto_fails(Ability::Wis));
        assert!(Condition::Dodging.disadvantage_to_attackers());
        assert!(!Condition::Dodging.incapacitated());
        assert!(Condition::Prone.advantage_to_attackers());
        assert!(Condition::Prone.disadvantage_on_attacks());
    }

    #[test]
    fn poisoned_only_burdens_its_own_attacks() {
        assert!(Condition::Poisoned.disadvantage_on_attacks());
        assert!(!Condition::Poisoned.advantage_to_attackers());
        assert!(!Condition::Poisoned.incapacitated());
        assert!(!Condition::Poisoned.auto_crits());
    }

    #[test]
    fn blinded_burdens_its_own_attacks_and_helps_attackers() {
        assert!(Condition::Blinded.disadvantage_on_attacks());
        assert!(Condition::Blinded.advantage_to_attackers());
        assert!(!Condition::Blinded.incapacitated());
        assert!(!Condition::Blinded.auto_crits());
    }

    /// Paralyzed is Stunned's three effects plus the auto-crit, not a fresh
    /// set - the compiler should catch it if a future edit to Stunned's
    /// semantics forgets its sibling.
    #[test]
    fn paralyzed_is_stunned_plus_the_close_range_crit() {
        assert!(Condition::Paralyzed.incapacitated());
        assert!(Condition::Paralyzed.advantage_to_attackers());
        assert!(Condition::Paralyzed.auto_fails(Ability::Str));
        assert!(Condition::Paralyzed.auto_fails(Ability::Dex));
        assert!(!Condition::Paralyzed.auto_fails(Ability::Con));
        assert!(Condition::Paralyzed.blocks_riders());
        assert!(Condition::Paralyzed.auto_crits());
        assert!(!Condition::Stunned.auto_crits());
    }

    /// Deafened carries none of Blinded's combat modifiers - see its own doc
    /// comment - so it is tracked for provenance only, the same gap Poisoned
    /// and Blinded already note for ability checks.
    #[test]
    fn deafened_changes_nothing_a_duel_checks() {
        assert!(!Condition::Deafened.incapacitated());
        assert!(!Condition::Deafened.advantage_to_attackers());
        assert!(!Condition::Deafened.disadvantage_to_attackers());
        assert!(!Condition::Deafened.disadvantage_on_attacks());
        assert!(!Condition::Deafened.auto_fails(Ability::Dex));
        assert!(!Condition::Deafened.blocks_riders());
        assert!(!Condition::Deafened.auto_crits());
    }

    /// Compelled steals the turn Command spends it on (checked at the duel
    /// layer, in `sim::fight::Fighter::loses_turn`) without carrying any of
    /// Incapacitated's other side effects - no advantage to attackers, no
    /// auto-failed Strength or Dexterity saves, and it must not block
    /// legendary actions the way real Incapacitated does.
    #[test]
    fn compelled_carries_none_of_incapacitated_side_effects() {
        assert!(!Condition::Compelled.incapacitated());
        assert!(!Condition::Compelled.advantage_to_attackers());
        assert!(!Condition::Compelled.disadvantage_to_attackers());
        assert!(!Condition::Compelled.disadvantage_on_attacks());
        assert!(!Condition::Compelled.auto_fails(Ability::Str));
        assert!(!Condition::Compelled.auto_fails(Ability::Dex));
        assert!(!Condition::Compelled.blocks_riders());
        assert!(!Condition::Compelled.auto_crits());
    }

    /// Steady Aim grants advantage on the creature's own attacks - the mirror
    /// of Poisoned/Blinded's self-inflicted disadvantage - and, unlike any
    /// other condition here, marks the speed-zeroing flag nothing yet reads.
    #[test]
    fn steady_aim_grants_advantage_and_flags_zero_speed() {
        assert!(Condition::SteadyAim.advantage_on_attacks());
        assert!(Condition::SteadyAim.zeroes_speed());
        assert!(!Condition::SteadyAim.disadvantage_on_attacks());
        assert!(!Condition::SteadyAim.incapacitated());
        assert!(!Condition::SteadyAim.advantage_to_attackers());
        assert!(!Condition::SteadyAim.auto_crits());
        // Nothing else grants its own attacks advantage or flags speed.
        for c in [
            Condition::Stunned,
            Condition::Dodging,
            Condition::Prone,
            Condition::Poisoned,
            Condition::Blinded,
            Condition::Paralyzed,
        ] {
            assert!(!c.advantage_on_attacks(), "{c:?} should not");
            assert!(!c.zeroes_speed(), "{c:?} should not");
        }
    }

    #[test]
    fn condition_names_round_trip_through_parse() {
        for c in [
            Condition::Stunned,
            Condition::Dodging,
            Condition::Prone,
            Condition::Poisoned,
            Condition::Blinded,
            Condition::Paralyzed,
            Condition::Deafened,
            Condition::Compelled,
            Condition::SteadyAim,
            Condition::Suppressed,
            Condition::Marked,
            Condition::SaveDisadvantage(Ability::Str),
            Condition::SaveDisadvantage(Ability::Dex),
            Condition::SaveDisadvantage(Ability::Con),
            Condition::SaveDisadvantage(Ability::Int),
            Condition::SaveDisadvantage(Ability::Wis),
            Condition::SaveDisadvantage(Ability::Cha),
            Condition::Silenced,
        ] {
            assert_eq!(Condition::parse(c.name()), Some(c));
        }
        assert_eq!(Condition::parse("paralysed"), Some(Condition::Paralyzed));
        assert_eq!(Condition::parse("steady aim"), Some(Condition::SteadyAim));
        assert_eq!(Condition::parse("nonsense"), None);
    }

    /// Suppressed is deliberately not incapacitating and does not touch
    /// attack rolls at all - only the three questions a limited-use item's
    /// debuff actually asks: can it cast or use an item, does it save worse,
    /// does its own damage suffer.
    #[test]
    fn suppressed_only_blocks_magic_items_saves_and_own_damage() {
        assert!(Condition::Suppressed.blocks_magic());
        assert!(Condition::Suppressed.disadvantage_on_save(Ability::Wis));
        assert!(Condition::Suppressed.halves_own_damage());

        assert!(!Condition::Suppressed.incapacitated());
        assert!(!Condition::Suppressed.advantage_to_attackers());
        assert!(!Condition::Suppressed.disadvantage_to_attackers());
        assert!(!Condition::Suppressed.disadvantage_on_attacks());
        assert!(!Condition::Suppressed.auto_fails(Ability::Str));
        assert!(!Condition::Suppressed.auto_fails(Ability::Dex));
        assert!(!Condition::Suppressed.blocks_riders());
        assert!(!Condition::Suppressed.auto_crits());

        // Nothing else answers "yes" to these by accident.
        for c in [
            Condition::Stunned,
            Condition::Dodging,
            Condition::Prone,
            Condition::Poisoned,
            Condition::Blinded,
            Condition::Paralyzed,
        ] {
            assert!(!c.blocks_magic(), "{c:?} should not block magic");
            assert!(
                !c.disadvantage_on_save(Ability::Con),
                "{c:?} should not disadvantage saves"
            );
            assert!(!c.halves_own_damage(), "{c:?} should not halve own damage");
        }
    }

    /// Marked only ever grants advantage to attackers - it does not
    /// incapacitate, burden its own attacks, or do anything else every other
    /// condition here does, which is the point: it is a narrow, single-shot
    /// primitive, not a bundle.
    #[test]
    fn marked_only_grants_advantage_to_attackers() {
        assert!(Condition::Marked.advantage_to_attackers());
        assert!(!Condition::Marked.incapacitated());
        assert!(!Condition::Marked.disadvantage_to_attackers());
        assert!(!Condition::Marked.disadvantage_on_attacks());
        assert!(!Condition::Marked.auto_fails(Ability::Str));
        assert!(!Condition::Marked.blocks_riders());
        assert!(!Condition::Marked.auto_crits());
    }

    /// An injury poison's burden touches exactly one kind of save and
    /// nothing else.
    #[test]
    fn a_save_disadvantage_burdens_only_its_own_ability() {
        let burden = Condition::SaveDisadvantage(Ability::Wis);
        assert!(burden.disadvantage_on_save(Ability::Wis));
        assert!(!burden.disadvantage_on_save(Ability::Con));
        assert!(!burden.incapacitated());
        assert!(!burden.advantage_to_attackers());
        assert!(!burden.disadvantage_on_attacks());
        assert!(!burden.blocks_magic());
        assert!(!burden.halves_own_damage());
    }
}
