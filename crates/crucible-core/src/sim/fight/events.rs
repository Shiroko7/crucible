//! The beats of a recorded fight that have numbers behind them: a move being
//! taken, an attack roll against an armor class, the damage it did, a saving
//! throw against a DC, a heal, a condition landing, a creature going down.
//!
//! Each is emitted from the one place in the resolution it happens, and reads
//! its dice off the generator ([`Rng::rolled`]) - so the rules that roll them
//! stay as they are, and a fight nobody is watching pays for a branch.

use crate::creature::{Effect, Move, MoveKind, Strike};
use crate::prob::Rng;
use crate::rules::{
    resolve_mode, Ability, AttackModifier, Condition, DamageRoll, Landed, RollMode, SaveModifier,
};
use crate::sim::fight::record::{signed, Detail, Die, FrameKind};
use crate::sim::fight::saves::{save_mode, saving_throw};
use crate::sim::fight::Fight;

/// The move being resolved, for the beats it gives rise to.
#[derive(Debug, Clone)]
pub(super) struct Cue {
    name: String,
    /// `weapon`, `spell`, `item` or `ability`: how a viewer should draw it.
    school: &'static str,
    ranged: bool,
    /// What it is being taken as - set by whoever is about to resolve a move.
    pub(super) slot: &'static str,
}

impl Default for Cue {
    fn default() -> Self {
        Self {
            name: String::new(),
            school: "ability",
            ranged: false,
            slot: "action",
        }
    }
}

/// What the next damage dealt is made of.
#[derive(Debug, Clone, Default)]
pub(super) struct Hurt {
    kinds: Vec<String>,
    dice: Vec<Die>,
    notes: Vec<String>,
}

impl Hurt {
    /// Damage rolled from `rolls`, with the dice that fell for it - and, for
    /// a pool taken at its maximum, the dice it stands in for on their
    /// highest faces.
    pub(super) fn of(rolls: &[DamageRoll], dice: &[(u32, i32)]) -> Self {
        let mut kinds: Vec<String> = Vec::new();
        for roll in rolls {
            let name = roll.kind.name().to_string();
            if !kinds.contains(&name) {
                kinds.push(name);
            }
        }
        Self {
            kinds,
            dice: dice
                .iter()
                .map(|&(sides, value)| Die {
                    sides,
                    value,
                    maxed: false,
                })
                .chain(
                    rolls
                        .iter()
                        .filter_map(|r| r.maximised)
                        .flat_map(|(count, sides)| {
                            (0..count).map(move |_| Die {
                                sides,
                                value: sides as i32,
                                maxed: true,
                            })
                        }),
                )
                .collect(),
            notes: Vec::new(),
        }
    }

    pub(super) fn note(&mut self, note: String) {
        self.notes.push(note);
    }
}

/// The first attack roll in `effect`, which is what says whether a move is a
/// weapon's and whether it is made at range.
fn first_strike(effect: &Effect) -> Option<&Strike> {
    match effect {
        Effect::Strikes { strike, .. } => Some(strike),
        Effect::Sequence(parts) => parts.iter().find_map(first_strike),
        Effect::Part { effect, .. } => first_strike(effect),
        _ => None,
    }
}

/// The dice a modifier list takes off the front of `dice`, summed with the
/// modifiers that roll none, and the dice left after them.
fn bonus_from(dice: &[(u32, i32)], each: impl Iterator<Item = (i32, u32)>) -> (i32, &[(u32, i32)]) {
    let (mut total, mut rest) = (0, dice);
    for (sign, count) in each {
        let take = (count as usize).min(rest.len());
        total += sign * rest[..take].iter().map(|&(_, face)| face).sum::<i32>();
        rest = &rest[take..];
    }
    (total, rest)
}

/// The die that counts out of the d20s rolled under `mode`.
fn kept(d20s: &[i32], mode: RollMode) -> i32 {
    match mode {
        RollMode::Advantage => d20s.iter().copied().max(),
        RollMode::Disadvantage => d20s.iter().copied().min(),
        RollMode::Normal => d20s.first().copied(),
    }
    .unwrap_or(0)
}

fn pair(d20s: &[i32]) -> String {
    match d20s {
        [a, b] => format!(" (rolled {a} and {b})"),
        _ => String::new(),
    }
}

/// What a splinter in the mind takes off the next save.
const SAP: SaveModifier = SaveModifier::PenaltyDice { count: 1, sides: 4 };

impl<'a> Fight<'a> {
    /// `me` takes `m`: it becomes the move in hand, and is announced. Returns
    /// the move that was in hand before, to put back afterwards.
    pub(super) fn announce(&mut self, m: &Move, me: usize, target: usize) -> Cue {
        let strike = first_strike(&m.effect);
        let school = match m.kind {
            MoveKind::Spell => "spell",
            MoveKind::MagicItem | MoveKind::ObjectUse => "item",
            MoveKind::Standard if strike.is_some_and(|s| s.kind.weapon) => "weapon",
            MoveKind::Standard => "ability",
        };
        let cue = Cue {
            name: if self.recording() {
                m.name.clone()
            } else {
                String::new()
            },
            school,
            ranged: strike.is_some_and(|s| s.kind.ranged),
            slot: self.cue.slot,
        };
        let before = std::mem::replace(&mut self.cue, cue);
        if self.recording() {
            let verb = match school {
                "spell" => "casts",
                "weapon" => "attacks with",
                _ => "uses",
            };
            let aside = match self.cue.slot {
                "action" => String::new(),
                slot => format!(" ({slot})"),
            };
            let text = format!(
                "{} {verb} {}{aside}",
                self.fighters[me].creature.name, m.name
            );
            let detail = Detail::Use {
                name: m.name.clone(),
                school: school.to_string(),
                slot: self.cue.slot.to_string(),
                ranged: self.cue.ranged,
            };
            let aimed = Self::aimed(m).then_some(target);
            self.beat(FrameKind::Use, Some(me), aimed, text, Some(detail));
        }
        before
    }

    /// An attack roll `me` just made at `target`, read back from the `dice`
    /// that fell for it: the d20s, whatever its modifiers rolled, and - on a
    /// hit - the damage, which becomes what the next damage dealt is made of.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn attack_rolled(
        &mut self,
        me: usize,
        target: usize,
        strike: &Strike,
        mode: RollMode,
        modifiers: &[AttackModifier],
        ac: i32,
        landed: Landed,
        dice: &[(u32, i32)],
        extra: &[DamageRoll],
        sneak_attack: bool,
    ) {
        let mode = resolve_mode(mode, modifiers);
        let rolled = if mode == RollMode::Normal { 1 } else { 2 };
        let rolled = rolled.min(dice.len());
        let d20s: Vec<i32> = dice[..rolled].iter().map(|&(_, face)| face).collect();
        let (bonus, damage) = bonus_from(
            &dice[rolled..],
            modifiers.iter().filter_map(|m| match *m {
                AttackModifier::BonusDice { count, .. } => Some((1, count)),
                AttackModifier::PenaltyDice { count, .. } => Some((-1, count)),
                _ => None,
            }),
        );
        let flat: i32 = modifiers
            .iter()
            .map(|m| match *m {
                AttackModifier::Flat(n) => n,
                _ => 0,
            })
            .sum();
        let kept = kept(&d20s, mode);
        let modifier = strike.to_hit + bonus + flat;
        let (outcome, verdict) = match landed {
            Landed::Miss => ("miss", "a miss"),
            Landed::Hit => ("hit", "a hit"),
            Landed::Crit => ("crit", "a critical hit"),
        };
        let text = format!(
            "{} attacks {}: {kept} {} = {} against AC {ac}{} - {verdict}",
            self.fighters[me].creature.name,
            self.fighters[target].creature.name,
            signed(modifier),
            kept + modifier,
            pair(&d20s),
        );
        let detail = Detail::Attack {
            name: self.cue.name.clone(),
            school: self.cue.school.to_string(),
            ranged: strike.kind.ranged,
            dice: d20s,
            kept,
            modifier,
            total: kept + modifier,
            ac,
            outcome: outcome.to_string(),
        };
        self.beat(
            FrameKind::Attack,
            Some(me),
            Some(target),
            text,
            Some(detail),
        );

        self.hurt = (landed != Landed::Miss).then(|| {
            let rolls: Vec<DamageRoll> = strike.damage.iter().chain(extra).cloned().collect();
            let mut hurt = Hurt::of(&rolls, damage);
            if landed == Landed::Crit {
                hurt.note("critical hit".to_string());
            }
            if sneak_attack {
                hurt.note("sneak attack".to_string());
            }
            hurt
        });
    }

    /// `target` just took `amount` damage, and `dropped` to 0 hit points from
    /// it. Damage of nothing in particular that came to nothing is not a beat.
    pub(super) fn damaged(&mut self, target: usize, amount: i32, dropped: bool) {
        if !self.recording() || (amount <= 0 && self.hurt.is_none()) {
            return;
        }
        let hurt = self.hurt.take().unwrap_or_default();
        let name = self.fighters[target].creature.name.clone();
        let kinds = match hurt.kinds.len() {
            0 => String::new(),
            _ => format!(" {}", hurt.kinds.join(" and ")),
        };
        let note = hurt.notes.join(", ");
        let aside = match note.as_str() {
            "" => String::new(),
            note => format!(" ({note})"),
        };
        self.beat(
            FrameKind::Damage,
            None,
            Some(target),
            format!("{name} takes {amount}{kinds} damage{aside}"),
            Some(Detail::Damage {
                amount,
                kinds: hurt.kinds,
                dice: hurt.dice,
                note,
                school: self.cue.school.to_string(),
            }),
        );
        if dropped {
            self.beat(
                FrameKind::Down,
                None,
                Some(target),
                format!("{name} goes down"),
                None,
            );
        }
    }

    /// A saving throw, rolled as [`saving_throw`] rolls it and recorded.
    pub(super) fn saving_throw(
        &mut self,
        rng: &mut Rng,
        who: usize,
        ability: Ability,
        dc: i32,
        advantage: bool,
    ) -> (bool, bool) {
        // A splinter in the mind comes off this save and is gone with it.
        let sapped = self.fighters[who].has(|c| c == Condition::Sapped);
        if sapped {
            let f = &mut self.fighters[who];
            f.conditions.retain(|&(c, _)| c != Condition::Sapped);
            f.save_modifiers.push(SAP);
        }
        let out = self.saving_throw_as_it_stands(rng, who, ability, dc, advantage);
        if sapped {
            let modifiers = &mut self.fighters[who].save_modifiers;
            if let Some(at) = modifiers.iter().rposition(|&m| m == SAP) {
                modifiers.remove(at);
            }
        }
        out
    }

    fn saving_throw_as_it_stands(
        &mut self,
        rng: &mut Rng,
        who: usize,
        ability: Ability,
        dc: i32,
        advantage: bool,
    ) -> (bool, bool) {
        if !self.recording() {
            return saving_throw(&mut self.fighters, rng, who, ability, dc, advantage);
        }
        let (auto_fail, mode, modifiers, base, name) = {
            let f = &self.fighters[who];
            (
                f.has(|c| c.auto_fails(ability)),
                save_mode(f, ability, advantage),
                f.save_modifiers.clone(),
                f.creature.save(ability),
                f.creature.name.clone(),
            )
        };
        rng.rolled();
        let (saved, resisted) = saving_throw(&mut self.fighters, rng, who, ability, dc, advantage);
        let dice = rng.rolled();
        // Whatever the modifiers roll falls first, then the d20s - and no
        // d20 at all for a save that fails whatever it would have been.
        let (bonus, rest) = bonus_from(
            &dice,
            modifiers.iter().map(|m| match *m {
                SaveModifier::BonusDice { count, .. } => (1, count),
                SaveModifier::PenaltyDice { count, .. } => (-1, count),
            }),
        );
        let d20s: Vec<i32> = rest.iter().map(|&(_, face)| face).collect();
        let kept = kept(&d20s, mode);
        let modifier = base + bonus;
        let (how, verdict) = match (auto_fail, saved, resisted) {
            (_, true, true) => (
                "legendary resistance",
                "failed, and chooses to succeed instead",
            ),
            (true, _, _) => ("automatic failure", "fails automatically"),
            (_, true, _) => ("rolled", "saved"),
            _ => ("rolled", "failed"),
        };
        let roll = if d20s.is_empty() {
            String::new()
        } else {
            format!(
                " {kept} {} = {} against DC {dc}{} -",
                signed(modifier),
                kept + modifier,
                pair(&d20s)
            )
        };
        self.beat(
            FrameKind::Save,
            Some(who),
            None,
            format!("{name} {} save:{roll} {verdict}", ability.name()),
            Some(Detail::Save {
                ability: ability.name().to_string(),
                dice: d20s,
                kept,
                modifier,
                total: kept + modifier,
                dc,
                passed: saved,
                how: how.to_string(),
            }),
        );
        (saved, resisted)
    }

    /// `me` just healed `who` for `gained` hit points.
    pub(super) fn healed(&mut self, me: usize, who: usize, gained: i32, revived: bool) {
        if !self.recording() || (gained <= 0 && !revived) {
            return;
        }
        let back = if revived { " and is back up" } else { "" };
        let text = format!(
            "{} regains {gained} hit points{back}",
            self.fighters[who].creature.name
        );
        let detail = Detail::Heal {
            amount: gained,
            school: self.cue.school.to_string(),
        };
        self.beat(FrameKind::Heal, Some(me), Some(who), text, Some(detail));
    }

    /// `applier` just landed `condition` on `victim`.
    pub(super) fn afflicted(&mut self, applier: usize, victim: usize, condition: Condition) {
        if !self.recording() {
            return;
        }
        let text = format!(
            "{} is {}",
            self.fighters[victim].creature.name,
            condition.name()
        );
        let detail = Detail::Condition {
            name: condition.name().to_string(),
        };
        self.beat(
            FrameKind::Condition,
            Some(applier),
            Some(victim),
            text,
            Some(detail),
        );
    }
}
