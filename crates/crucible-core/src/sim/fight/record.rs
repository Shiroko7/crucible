//! A fight written down as frames: what happened, and where everyone stood
//! and how hurt they were once it had. This is what a viewer plays back.
//!
//! A frame is one beat of the fight - an initiative roll, a turn starting, a
//! walk, a move being taken, an attack roll, the damage it did, a saving
//! throw, a heal, a condition landing - with the narration for it, the dice
//! behind it and the state after. The dice are read off the generator
//! ([`Rng::rolled`]) rather than handed back by every rule that rolls one.

use serde::{Deserialize, Serialize};

use crate::creature::Creature;
use crate::prob::Rng;
use crate::sim::fight::{Fight, Side};
use crate::sim::{Budget, Outcome, Policy};

/// What kind of beat a frame is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameKind {
    /// Before anything: everyone in place, nobody has rolled.
    Start,
    /// One creature's initiative roll.
    Initiative,
    /// A creature's turn begins.
    Turn,
    /// A walk across the grid.
    Move,
    /// A move being taken: a weapon swung, a spell cast, a potion drunk.
    Use,
    /// An attack roll against an armor class.
    Attack,
    /// Hit points lost.
    Damage,
    /// A saving throw against a DC.
    Save,
    /// Hit points regained.
    Heal,
    /// A condition landing on somebody.
    Condition,
    /// A creature dropping to 0 hit points.
    Down,
    /// A turn lost to a condition.
    Lost,
    /// A condition shaken off at the end of a turn.
    Recover,
    /// Anything else worth a line - digestion, a regurgitation.
    Other,
    End,
}

/// One die as it fell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Die {
    pub sides: u32,
    pub value: i32,
    /// Not rolled: a die of a pool taken at its maximum, shown on its
    /// highest face.
    #[serde(default)]
    pub maxed: bool,
}

/// The numbers behind a beat, for the kinds of beat that have them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Detail {
    Initiative {
        die: i32,
        modifier: i32,
        total: i32,
    },
    Use {
        name: String,
        /// `weapon`, `spell`, `item` or `ability`.
        school: String,
        /// `action`, `bonus action`, `legendary action`, `reaction` or `aura`.
        slot: String,
        ranged: bool,
    },
    Attack {
        name: String,
        school: String,
        ranged: bool,
        /// The d20s rolled: two under advantage or disadvantage.
        dice: Vec<i32>,
        /// The one that counted.
        kept: i32,
        /// Everything added to it: the attack bonus, and dice like Bless's.
        modifier: i32,
        total: i32,
        ac: i32,
        /// `miss`, `hit` or `crit`.
        outcome: String,
    },
    Damage {
        amount: i32,
        /// The damage types in it.
        kinds: Vec<String>,
        dice: Vec<Die>,
        /// Anything that changed it on the way: `sneak attack`, `deflected 12`.
        note: String,
        /// `weapon`, `spell`, `item` or `ability`: what dealt it.
        school: String,
    },
    Save {
        ability: String,
        dice: Vec<i32>,
        kept: i32,
        modifier: i32,
        total: i32,
        dc: i32,
        passed: bool,
        /// `rolled`, `legendary resistance` or `automatic failure`.
        how: String,
    },
    Heal {
        amount: i32,
        school: String,
    },
    Condition {
        name: String,
    },
}

/// One creature as a frame leaves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitState {
    pub hp: i32,
    pub temp_hp: i32,
    /// Its Armor Class as it stands: what it wears, plus whatever is
    /// holding it higher just now.
    #[serde(default)]
    pub ac: i32,
    pub x: i32,
    pub y: i32,
    pub alive: bool,
    /// Off the grid for now: swallowed.
    pub hidden: bool,
    pub conditions: Vec<String>,
    /// The spell it is concentrating on, if it is holding one.
    #[serde(default)]
    pub concentrating: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub round: u32,
    pub kind: FrameKind,
    /// Roster seats: who did it, and to whom.
    pub actor: Option<usize>,
    pub target: Option<usize>,
    pub text: String,
    /// The squares the actor walked through, for a [`FrameKind::Move`].
    pub path: Vec<(i32, i32)>,
    /// The roll or the amount behind it, when it has one.
    #[serde(default)]
    pub detail: Option<Detail>,
    /// Everyone after this beat, by roster seat. A summon has no entry in
    /// the frames from before it was called up.
    pub units: Vec<UnitState>,
}

/// What does not change about a creature over a fight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub name: String,
    pub team: u8,
    /// Squares along each side of its space.
    pub span: i32,
    pub max_hp: i32,
    /// Its Armor Class with nothing raising it.
    #[serde(default)]
    pub ac: i32,
    pub summon: bool,
}

/// A whole fight, ready to play back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replay {
    /// The grid in squares, or 0 by 0 for a fight without one.
    pub width: i32,
    pub height: i32,
    pub units: Vec<Unit>,
    /// Roster seats in initiative order.
    #[serde(default)]
    pub order: Vec<usize>,
    pub frames: Vec<Frame>,
    /// The same fight as the narrated log prints it.
    pub lines: Vec<String>,
    /// 0 or 1, or `None` for a fight nobody won.
    pub winner: Option<u8>,
    pub rounds: u32,
}

/// The frames of a fight being recorded. A clone records nothing: a search
/// rollout is a fight nobody watches.
#[derive(Default)]
pub(super) struct Tape(Option<Vec<Frame>>);

impl Clone for Tape {
    fn clone(&self) -> Self {
        Tape(None)
    }
}

impl<'a> Fight<'a> {
    /// Add a frame, if this fight is being recorded.
    pub(super) fn frame(
        &mut self,
        round: u32,
        kind: FrameKind,
        actor: Option<usize>,
        target: Option<usize>,
        text: String,
        path: Vec<(i32, i32)>,
    ) {
        self.frame_with(round, kind, actor, target, text, path, None);
    }

    /// Is this fight being recorded? Asked before building anything a frame
    /// would carry, so a fight nobody watches pays for none of it.
    pub(super) fn recording(&self) -> bool {
        self.tape.0.is_some()
    }

    /// A beat with numbers behind it, in the round being played.
    pub(super) fn beat(
        &mut self,
        kind: FrameKind,
        actor: Option<usize>,
        target: Option<usize>,
        text: String,
        detail: Option<Detail>,
    ) {
        self.frame_with(self.round, kind, actor, target, text, Vec::new(), detail);
    }

    #[allow(clippy::too_many_arguments)]
    fn frame_with(
        &mut self,
        round: u32,
        kind: FrameKind,
        actor: Option<usize>,
        target: Option<usize>,
        text: String,
        path: Vec<(i32, i32)>,
        detail: Option<Detail>,
    ) {
        if self.tape.0.is_none() {
            return;
        }
        let units = self
            .fighters
            .iter()
            .enumerate()
            .map(|(seat, f)| UnitState {
                hp: f.hp.max(0),
                temp_hp: f.temp_hp,
                ac: f.creature.ac
                    + self.boon_ac(seat)
                    + f.reactive_ac.map_or(0, |(_, bonus)| bonus),
                x: f.pos.0,
                y: f.pos.1,
                alive: f.alive(),
                hidden: f.swallowed_by().is_some(),
                conditions: f
                    .conditions
                    .iter()
                    .map(|&(c, _)| c.name().to_string())
                    .collect(),
                concentrating: f.concentration.as_ref().map(|_| {
                    self.held
                        .iter()
                        .find(|(who, _)| *who == seat)
                        .map_or_else(|| "a spell".to_string(), |(_, spell)| spell.clone())
                }),
            })
            .collect();
        if let Some(frames) = self.tape.0.as_mut() {
            frames.push(Frame {
                round,
                kind,
                actor,
                target,
                text,
                path,
                detail,
                units,
            });
        }
    }
}

/// A modifier as it is written after a roll: `+ 5`, `- 1`.
pub(super) fn signed(n: i32) -> String {
    if n < 0 {
        format!("- {}", -n)
    } else {
        format!("+ {n}")
    }
}

/// [`crate::sim::run_teams`], recorded.
pub fn replay_teams(
    rng: &mut Rng,
    roster: &[&Creature],
    policies: [Policy; 2],
    max_rounds: u32,
    budget: Budget,
) -> (Outcome, Replay) {
    let roster: Vec<(&Creature, Side)> = roster
        .iter()
        .map(|&c| (c, Side::from_team(c.team)))
        .collect();
    let mut log = Some(Vec::new());
    rng.watch();
    rng.rolled();
    let mut fight = Fight::new(rng, &roster, policies, max_rounds, budget, &mut log);
    // One d20 each, in roster order: what `Fight::new` just rolled.
    let initiative = rng.rolled();
    fight.tape = Tape(Some(Vec::new()));
    fight.round = 1;
    fight.beat(
        FrameKind::Start,
        None,
        None,
        "Combat begins. Roll for initiative!".to_string(),
        None,
    );
    let mut initiative = initiative.into_iter().map(|(_, die)| die);
    for seat in 0..roster.len() {
        let (name, modifier, twice) = {
            let c = fight.fighters[seat].creature;
            let twice = c
                .riders
                .contains(&crate::creature::Rider::InitiativeAdvantage);
            (c.name.clone(), c.initiative, twice)
        };
        // Two dice fell for whatever rolls with advantage: the better one.
        let Some(mut die) = initiative.next() else {
            break;
        };
        if twice {
            die = die.max(initiative.next().unwrap_or(die));
        }
        fight.beat(
            FrameKind::Initiative,
            Some(seat),
            None,
            format!(
                "{name} rolls initiative: {die} {} = {}",
                signed(modifier),
                die + modifier
            ),
            Some(Detail::Initiative {
                die,
                modifier,
                total: die + modifier,
            }),
        );
    }
    let outcome = fight.play(rng, 1, 0, &mut log);
    let verdict = match outcome.winner {
        Some(side) => format!("side {side:?} wins in round {}", outcome.rounds),
        None => format!("unresolved after round {}", outcome.rounds),
    };
    fight.frame(
        outcome.rounds,
        FrameKind::End,
        None,
        None,
        verdict,
        Vec::new(),
    );

    let (width, height) = fight.grid.unwrap_or((0, 0));
    let replay = Replay {
        width,
        height,
        units: fight
            .fighters
            .iter()
            .map(|f| Unit {
                name: f.creature.name.clone(),
                team: f.side.index() as u8,
                span: f.span,
                max_hp: f.creature.hp,
                ac: f.creature.ac,
                summon: f.is_summon(),
            })
            .collect(),
        order: fight.order.clone(),
        frames: fight.tape.0.take().unwrap_or_default(),
        lines: log.unwrap_or_default(),
        winner: outcome.winner.map(|s| s.index() as u8),
        rounds: outcome.rounds,
    };
    (outcome, replay)
}
