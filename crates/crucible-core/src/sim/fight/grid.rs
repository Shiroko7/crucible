//! A fight on a grid: where everyone stands, how far a move reaches, and
//! the walk that brings a target into it. See [`crate::sim::arena`].
//!
//! A turn on a grid is move, then act. What a creature can reach is judged
//! as if it had already moved ([`Fight::stride`]), so a policy picks its
//! action the way it always has; the walk is then whatever that action needs,
//! and a creature with nothing in reach Dashes instead.
//!
//! What is not here yet: opportunity attacks, walls and cover, shaped areas
//! (an area is a radius around its target), and forced movement. A creature
//! with a mouth keeps its zones - see `zones` - and nothing around it is
//! measured on the grid.

use std::collections::VecDeque;

use crate::creature::{AttackKind, Effect, Move};
use crate::rules::Condition;
use crate::sim::arena::{self, SQUARE};
use crate::sim::fight::Fight;
use crate::sim::Plan;

/// How far a melee attack reaches when its move does not say.
const MELEE: u32 = 5;
/// How far a ranged attack, a spell or a breath reaches when its move does
/// not say.
const RANGED: u32 = 60;
/// How far an aura reaches when it does not say.
pub(super) const AURA: u32 = 10;

/// A square a walk gets to: where, how many squares in (negative when it
/// can only be passed through), and the square before it on the way.
type Reached = ((i32, i32), i32, (i32, i32));

/// The move being resolved or weighed right now, for the checks far below
/// it that ask how far it reaches - see [`Fight::with_cast`].
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Cast {
    range: Option<u32>,
    area: Option<u32>,
    target: Option<usize>,
    /// A spell, or a magic item at work: what Magic Resistance is against.
    pub(super) magical: bool,
    /// How far an attack in it can be made at disadvantage - see
    /// [`crate::creature::Rider::LongRange`].
    pub(super) long: Option<u32>,
    /// It carries its user its speed again before it is made - see
    /// [`crate::creature::Rider::NeverProvokes`] on a move.
    surges: bool,
}

impl Cast {
    pub(super) fn of(m: &Move, target: usize) -> Self {
        Self {
            range: m.range,
            area: m.area,
            target: Some(target),
            magical: matches!(
                m.kind,
                crate::creature::MoveKind::Spell | crate::creature::MoveKind::MagicItem
            ),
            surges: m.carries(&crate::creature::Rider::NeverProvokes),
            long: m.all_riders().into_iter().find_map(|r| match r {
                crate::creature::Rider::LongRange { feet } => Some(*feet),
                _ => None,
            }),
        }
    }
}

/// A line somebody might ride: how it rates, where it ends, and who is on
/// it.
type Line = ((i32, bool, i32), (i32, i32), Vec<usize>);

/// Does `aura` stay where it was put - see
/// [`crate::creature::Rider::Anchored`]?
pub(super) fn anchored(aura: &Move) -> bool {
    aura.riders
        .iter()
        .any(|r| matches!(r, crate::creature::Rider::Anchored { .. }))
}

impl<'a> Fight<'a> {
    pub(super) fn on_grid(&self) -> bool {
        self.grid.is_some()
    }

    /// Squares between `a` and `b`: 1 when adjacent.
    fn gap(&self, a: usize, b: usize) -> i32 {
        let (a, b) = (&self.fighters[a], &self.fighters[b]);
        arena::gap(a.pos, a.span, b.pos, b.span)
    }

    fn range_of(attack: Option<AttackKind>, declared: Option<u32>) -> u32 {
        declared.unwrap_or(match attack {
            Some(kind) if !kind.ranged => MELEE,
            _ => RANGED,
        })
    }

    /// Is `to` within `feet` of `from` - or, for the creature whose turn it
    /// is and which has not moved yet, could it be once it has?
    pub(super) fn within(&self, from: usize, to: usize, feet: u32) -> bool {
        if !self.on_grid() || from == to {
            return true;
        }
        let mut gap = self.gap(from, to);
        if let Some((who, squares)) = self.stride {
            if who == from {
                // Twice as far, with a move in hand that carries it there.
                let strides = if self.cast.get().surges { 2 } else { 1 };
                gap = (gap - strides * squares).max(1);
            }
        }
        gap.max(0) as u32 * SQUARE <= feet
    }

    /// [`Fight::within`] the range of the move being resolved or weighed,
    /// for a part of it that is an attack roll of `attack`'s kind or, when
    /// `None`, anything else.
    pub(super) fn in_range(&self, from: usize, to: usize, attack: Option<AttackKind>) -> bool {
        // What an aura that was put down catches was settled by where it
        // was put, not by how far off its owner is.
        if matches!(self.pull_to, Some((owner, _)) if owner == from) {
            return true;
        }
        let cast = self.cast.get();
        let feet = Self::range_of(attack, cast.range);
        // An attack reaches its long range too, for what that is worth.
        let feet = match (attack, cast.long) {
            (Some(_), Some(long)) => feet.max(long),
            _ => feet,
        };
        self.within(from, to, feet)
    }

    /// Feet between `a` and `b` as they stand: 5 when adjacent, and 0
    /// with nowhere to stand.
    pub(super) fn feet_between(&self, a: usize, b: usize) -> u32 {
        match self.on_grid() {
            true => self.gap(a, b).max(0) as u32 * SQUARE,
            false => 0,
        }
    }

    /// The range the attack in hand is made at without penalty.
    pub(super) fn normal_range(&self, attack: AttackKind) -> u32 {
        Self::range_of(Some(attack), self.cast.get().range)
    }

    /// How many enemy auras `me` would be standing in on `cell` - what it
    /// would rather not end its turn inside.
    pub(super) fn hazards_at(&self, me: usize, cell: (i32, i32)) -> i64 {
        let (side, span) = (self.fighters[me].side, self.fighters[me].span);
        let mut count = 0;
        for (owner, holder) in self.fighters.iter().enumerate() {
            if holder.side == side || !holder.alive() {
                continue;
            }
            let always = holder.creature.auras.iter();
            let raised = holder
                .creature
                .lasting_auras
                .iter()
                .enumerate()
                .filter(|(which, _)| self.has_aura(owner, *which))
                .map(|(_, aura)| aura);
            for aura in always.chain(raised) {
                let (middle, width) = match self.anchor_of(owner, aura) {
                    Some(middle) => (middle, 1),
                    None => (holder.pos, holder.span),
                };
                if arena::gap(cell, span, middle, width).max(0) as u32 * SQUARE
                    <= aura.range.unwrap_or(AURA)
                {
                    count += 1;
                }
            }
        }
        count
    }

    /// How many of `me`'s own side stand within ten feet of `cell`: who a
    /// blast aimed at it would catch as well.
    pub(super) fn crowd_at(&self, me: usize, cell: (i32, i32)) -> i64 {
        let (side, span) = (self.fighters[me].side, self.fighters[me].span);
        self.fighters
            .iter()
            .enumerate()
            .filter(|&(i, f)| {
                i != me && f.side == side && f.alive() && arena::gap(cell, span, f.pos, f.span) <= 2
            })
            .count() as i64
    }

    /// Does another creature stand between `from` and `to` - half cover
    /// against a ranged attack?
    pub(super) fn screened(&self, from: usize, to: usize) -> bool {
        if !self.on_grid() {
            return false;
        }
        let middle = |i: usize| {
            let f = &self.fighters[i];
            (2 * f.pos.0 + f.span, 2 * f.pos.1 + f.span)
        };
        let (a, b) = (middle(from), middle(to));
        let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) / 2).max(1);
        (1..steps).any(|step| {
            // The square the line is over, a square's length at a time.
            let at = |from: i32, to: i32| (from + (to - from) * step / steps).div_euclid(2);
            let cell = (at(a.0, b.0), at(a.1, b.1));
            self.fighters.iter().enumerate().any(|(i, f)| {
                i != from
                    && i != to
                    && f.alive()
                    && f.swallowed_by().is_none()
                    && arena::overlaps(cell, 1, f.pos, f.span)
            })
        })
    }

    /// Does the area of the move being resolved catch `who`? Everything in
    /// range, unless it says how close to its target that has to be.
    pub(super) fn in_area(&self, who: usize) -> bool {
        match self.cast.get() {
            Cast {
                area: Some(feet),
                target: Some(target),
                ..
            } if self.on_grid() && who != target => {
                self.gap(who, target).max(0) as u32 * SQUARE <= feet
            }
            _ => true,
        }
    }

    /// Everyone a swing made at everything in `me`'s reach would land on,
    /// whichever side they are on - see
    /// [`crate::creature::Rider::EachInReach`]. Still choosing what to do,
    /// `me` has not walked up to `target` yet, so it is `target` and
    /// whoever is standing that close to it. With nowhere to stand it is
    /// the other side alone.
    pub(super) fn swept(
        &self,
        me: usize,
        target: usize,
        reach: crate::creature::Reach,
        kind: AttackKind,
    ) -> Vec<usize> {
        let side = self.fighters[me].side;
        let feet = Self::range_of(Some(kind), self.cast.get().range);
        let planning = self.stride.is_some_and(|(who, _)| who == me);
        (0..self.fighters.len())
            .filter(|&i| {
                i != me && self.fighters[i].alive() && self.in_reach(me, i, reach, Some(kind))
            })
            .filter(|&i| match (self.on_grid(), planning) {
                (false, _) => self.fighters[i].side != side,
                (true, true) => i == target || self.gap(i, target).max(0) as u32 * SQUARE <= feet,
                (true, false) => true,
            })
            .collect()
    }

    /// How far the heal in hand reaches, in feet: what it says, or a touch.
    pub(super) fn heal_reach(&self) -> u32 {
        self.cast.get().range.unwrap_or(MELEE)
    }

    /// Run `f` with `m` aimed at `target` as the move in hand.
    pub(super) fn with_cast<T>(&self, m: &Move, target: usize, f: impl FnOnce() -> T) -> T {
        let previous = self.cast.replace(Cast::of(m, target));
        let out = f();
        self.cast.set(previous);
        out
    }

    /// Does an aura of `owner`'s reach `who`?
    pub(super) fn aura_reaches(&self, owner: usize, who: usize, aura: &Move) -> bool {
        if let Some(middle) = self.anchor_of(owner, aura) {
            let f = &self.fighters[who];
            return arena::gap(middle, 1, f.pos, f.span).max(0) as u32 * SQUARE
                <= aura.range.unwrap_or(AURA);
        }
        self.has_mouth(owner) || self.within(owner, who, aura.range.unwrap_or(AURA))
    }

    /// Where `aura`, one of `owner`'s raised ones, was put - if it was put
    /// somewhere rather than carried, and there is a grid to put it on.
    pub(super) fn anchor_of(&self, owner: usize, aura: &Move) -> Option<(i32, i32)> {
        if !self.on_grid() || !anchored(aura) {
            return None;
        }
        let holder = &self.fighters[owner];
        let which = holder
            .creature
            .lasting_auras
            .iter()
            .position(|m| std::ptr::eq(m, aura))?;
        holder.anchors.get(which).copied().flatten()
    }

    /// Is `me` standing in difficult terrain somebody put down - see
    /// [`crate::creature::Rider::Hinders`]?
    fn hindered(&self, me: usize) -> bool {
        (0..self.fighters.len()).any(|owner| {
            let holder = &self.fighters[owner];
            holder.alive()
                && holder
                    .creature
                    .lasting_auras
                    .iter()
                    .enumerate()
                    .any(|(which, aura)| {
                        aura.riders.contains(&crate::creature::Rider::Hinders)
                            && self.has_aura(owner, which)
                            && self.anchor_of(owner, aura).is_some()
                            && self.aura_reaches(owner, me, aura)
                    })
        })
    }

    /// The nearest enemy of `me` within `feet` of `from` that is not among
    /// `struck`: where a chain goes next - see
    /// [`crate::creature::Rider::Chains`].
    pub(super) fn arc_from(
        &self,
        me: usize,
        from: usize,
        feet: u32,
        struck: &[usize],
    ) -> Option<usize> {
        let side = self.fighters[me].side;
        (0..self.fighters.len())
            .filter(|&i| {
                let f = &self.fighters[i];
                f.side != side && f.alive() && !struck.contains(&i) && self.reaches(me, i)
            })
            .map(|i| (self.gap(from, i).max(0), i))
            .filter(|&(gap, _)| !self.on_grid() || gap as u32 * SQUARE <= feet)
            .min()
            .map(|(_, i)| i)
    }

    /// The line `me` would ride - see [`crate::creature::Rider::RideLine`]:
    /// where it ends, and everyone on the way. Of every straight line of
    /// `feet` it could take, the one that crosses the most enemies and the
    /// fewest of its own side, through `target` when that costs nothing.
    /// With nowhere to stand, it is every enemy and it goes nowhere.
    pub(super) fn line_from(
        &self,
        me: usize,
        target: usize,
        feet: u32,
    ) -> (Option<(i32, i32)>, Vec<usize>) {
        let side = self.fighters[me].side;
        let Some((width, height)) = self.grid else {
            let all = (0..self.fighters.len())
                .filter(|&i| {
                    let f = &self.fighters[i];
                    f.side != side && f.alive() && self.reaches(me, i)
                })
                .collect();
            return (None, all);
        };
        let (start, span) = (self.fighters[me].pos, self.fighters[me].span);
        let reach = (feet / SQUARE) as i32;
        let others: Vec<(usize, (i32, i32), i32)> = self
            .fighters
            .iter()
            .enumerate()
            .filter(|&(i, f)| i != me && f.alive() && self.reaches(me, i))
            .map(|(i, f)| (i, f.pos, f.span))
            .collect();
        let free = |cell: (i32, i32)| {
            cell.0 >= 0
                && cell.1 >= 0
                && cell.0 + span <= width
                && cell.1 + span <= height
                && !others
                    .iter()
                    .any(|&(_, at, s)| arena::overlaps(cell, span, at, s))
        };
        let mut best: Option<Line> = None;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                if dx.abs().max(dy.abs()) != reach {
                    continue;
                }
                // The squares along the way, a step at a time.
                let cells: Vec<(i32, i32)> = (1..=reach)
                    .map(|step| {
                        let along = |d: i32| (2 * d * step + reach * d.signum()) / (2 * reach);
                        (start.0 + along(dx), start.1 + along(dy))
                    })
                    .collect();
                // It ends on the furthest of them it can stand on.
                let Some(last) = cells.iter().rposition(|&cell| free(cell)) else {
                    continue;
                };
                let crossed: Vec<usize> = others
                    .iter()
                    .filter(|&&(_, at, s)| {
                        cells[..last]
                            .iter()
                            .any(|&cell| arena::overlaps(cell, span, at, s))
                    })
                    .map(|&(i, _, _)| i)
                    .collect();
                let foes = crossed
                    .iter()
                    .filter(|&&i| self.fighters[i].side != side)
                    .count() as i32;
                let rated = (
                    2 * foes - crossed.len() as i32,
                    crossed.contains(&target),
                    last as i32,
                );
                if best.as_ref().is_none_or(|(most, _, _)| rated > *most) {
                    best = Some((rated, cells[last], crossed));
                }
            }
        }
        match best {
            Some((rated, to, crossed)) if rated.0 > 0 => (Some(to), crossed),
            _ => (None, Vec::new()),
        }
    }

    /// Squares `me` can cover this turn: its speed, halved while slowed and
    /// again by getting up, and none at all while something holds it.
    pub(super) fn squares(&self, me: usize) -> i32 {
        let f = &self.fighters[me];
        if f.has(Condition::zeroes_speed) {
            return 0;
        }
        let mut squares = (f.creature.speed / SQUARE) as i32;
        if f.has(|c| c == Condition::Slowed) {
            squares /= 2;
        }
        if f.stood_up {
            squares /= 2;
        }
        if self.hindered(me) {
            squares /= 2;
        }
        // A level of exhaustion is five feet.
        (squares - i32::from(f.exhaustion)).max(0)
    }

    /// The shortest range among the parts of `effect` aimed at an enemy, or
    /// `None` when it has none - a heal, a stance, a summon.
    fn effect_range(effect: &Effect, declared: Option<u32>) -> Option<u32> {
        match effect {
            Effect::Strikes { strike, .. } => Some(Self::range_of(Some(strike.kind), declared)),
            Effect::Save(_) | Effect::AutoHit { .. } | Effect::Afflict { .. } => {
                Some(Self::range_of(None, declared))
            }
            // An aura put down at a distance has to be within that distance
            // of whoever it is put around.
            Effect::Aura { .. } => declared,
            Effect::Sequence(parts) => parts
                .iter()
                .filter_map(|p| Self::effect_range(p, declared))
                .min(),
            Effect::Part { effect, .. } => Self::effect_range(effect, declared),
            _ => None,
        }
    }

    /// Does `m` do anything to an enemy?
    pub(super) fn aimed(m: &Move) -> bool {
        Self::effect_range(&m.effect, m.range).is_some()
    }

    /// How close `me` wants to be to its target to carry out `plan`: the
    /// range of the action it chose, or - with no action that reaches - of
    /// whatever it fights with, which is where a Dash is headed.
    pub(super) fn wanted_range(&self, me: usize, plan: &Plan) -> Option<u32> {
        let creature = self.fighters[me].creature;
        let of = |m: &Move| Self::effect_range(&m.effect, m.range);
        match plan.action.and_then(|i| creature.actions.get(i)) {
            // An emanation is raised from where its target will be in it.
            Some(Move {
                effect: Effect::Aura { which, .. },
                range: None,
                ..
            }) => creature
                .lasting_auras
                .get(*which)
                .map(|aura| aura.range.unwrap_or(AURA)),
            Some(m) => of(m),
            None => creature.actions.iter().filter_map(of).min(),
        }
        .or_else(|| {
            plan.bonus
                .and_then(|i| creature.bonus_actions.get(i))
                .and_then(of)
        })
    }

    /// Walk `me` towards `target` until it is within `feet`, or as near as
    /// its speed gets it. It passes through its own side but not the other,
    /// and stops only on free squares. Returns the squares it walked through.
    pub(super) fn advance(&mut self, me: usize, target: usize, feet: u32) -> Vec<(i32, i32)> {
        let Some((width, height)) = self.grid else {
            return Vec::new();
        };
        let need = ((feet / SQUARE) as i32).max(1);
        let budget = self.squares(me);
        let (start, span, side) = {
            let f = &self.fighters[me];
            (f.pos, f.span, f.side)
        };
        let goal = (self.fighters[target].pos, self.fighters[target].span);
        let to_goal = |cell| arena::gap(cell, span, goal.0, goal.1).max(need);
        if budget == 0 || to_goal(start) == need {
            return Vec::new();
        }

        // Everyone else standing somewhere: where, how big, and whether it
        // blocks the way or only the stopping.
        let others: Vec<((i32, i32), i32, bool)> = self
            .fighters
            .iter()
            .enumerate()
            .filter(|&(i, f)| i != me && f.alive() && f.swallowed_by().is_none())
            .map(|(_, f)| (f.pos, f.span, f.side != side))
            .collect();
        // Something that walks through bodies is stopped by none of them,
        // though it can no more stand on one than anything else can.
        let phasing = self.fighters[me].creature.phases_through().is_some();
        let blocked = |cell, only_enemies: bool| {
            !(phasing && only_enemies)
                && others.iter().any(|&(at, s, enemy)| {
                    (enemy || !only_enemies) && arena::overlaps(cell, span, at, s)
                })
        };

        // Breadth-first over the squares its speed reaches.
        let dim = 2 * budget + 1;
        let index =
            |c: (i32, i32)| ((c.1 - start.1 + budget) * dim + (c.0 - start.0 + budget)) as usize;
        let mut steps = vec![-1i32; (dim * dim) as usize];
        let mut came_from = vec![start; (dim * dim) as usize];
        let mut queue = VecDeque::from([start]);
        steps[index(start)] = 0;
        let mut best = (to_goal(start), 0, start);
        while let Some(cell) = queue.pop_front() {
            let taken = steps[index(cell)];
            if cell == start || !blocked(cell, false) {
                let reached = (to_goal(cell), taken);
                if reached < (best.0, best.1) {
                    best = (reached.0, reached.1, cell);
                }
                if reached.0 == need {
                    break;
                }
            }
            if taken == budget {
                continue;
            }
            for (dx, dy) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let next = (cell.0 + dx, cell.1 + dy);
                if next.0 < 0
                    || next.1 < 0
                    || next.0 + span > width
                    || next.1 + span > height
                    || steps[index(next)] >= 0
                    || blocked(next, true)
                {
                    continue;
                }
                steps[index(next)] = taken + 1;
                came_from[index(next)] = cell;
                queue.push_back(next);
            }
        }

        let mut path = Vec::new();
        let mut cell = best.2;
        while cell != start {
            path.push(cell);
            cell = came_from[index(cell)];
        }
        path.reverse();
        self.fighters[me].pos = best.2;
        path
    }

    /// A way for something that walks through bodies to walk through as
    /// many of its enemies as its speed allows and still end within `feet`
    /// of `target` - see [`crate::creature::Rider::PhaseThrough`]. A square
    /// somebody stands on costs two, as difficult terrain does. `None` when
    /// there is nobody it can pass through and still do that.
    pub(super) fn phase_walk(
        &mut self,
        me: usize,
        target: usize,
        feet: u32,
    ) -> Option<Vec<(i32, i32)>> {
        let (width, height) = self.grid?;
        let roll = self.fighters[me].creature.phases_through()?;
        let (start, span, side) = {
            let f = &self.fighters[me];
            (f.pos, f.span, f.side)
        };
        let need = ((feet / SQUARE) as i32).max(1);
        let goal = (self.fighters[target].pos, self.fighters[target].span);
        let others: Vec<(usize, (i32, i32), i32)> = self
            .fighters
            .iter()
            .enumerate()
            .filter(|&(i, f)| i != me && f.alive() && f.swallowed_by().is_none())
            .map(|(i, f)| (i, f.pos, f.span))
            .collect();
        let occupied = |cell: (i32, i32)| {
            others
                .iter()
                .any(|&(_, at, s)| arena::overlaps(cell, span, at, s))
        };
        let inside = |cell: (i32, i32)| {
            cell.0 >= 0 && cell.1 >= 0 && cell.0 + span <= width && cell.1 + span <= height
        };
        // Straight from one square to another, and what that costs.
        let walk = |from: (i32, i32), to: (i32, i32)| {
            let (mut cells, mut cost, mut cell) = (Vec::new(), 0, from);
            while cell != to {
                cell = (
                    cell.0 + (to.0 - cell.0).signum(),
                    cell.1 + (to.1 - cell.1).signum(),
                );
                cost += if occupied(cell) { 2 } else { 1 };
                cells.push(cell);
            }
            (cells, cost)
        };
        // Out the other side: the cheapest free square to stop on that is
        // near enough to `target`.
        let finish = |from: (i32, i32), left: i32| {
            let reach = left.min(4);
            let mut best: Option<(Vec<(i32, i32)>, i32)> = None;
            for dy in -reach..=reach {
                for dx in -reach..=reach {
                    let cell = (from.0 + dx, from.1 + dy);
                    if !inside(cell)
                        || occupied(cell)
                        || arena::gap(cell, span, goal.0, goal.1) > need
                    {
                        continue;
                    }
                    let (cells, cost) = walk(from, cell);
                    if cost <= left && best.as_ref().is_none_or(|(_, least)| cost < *least) {
                        best = Some((cells, cost));
                    }
                }
            }
            best
        };
        // Whoever it would hurt to walk through: its target first, then
        // the nearest.
        let mut prey: Vec<(usize, (i32, i32), i32)> = others
            .iter()
            .copied()
            .filter(|&(i, _, _)| {
                let f = &self.fighters[i];
                f.side != side
                    && f.creature.reduction(roll.kind) != crate::rules::Reduction::Immune
                    && f.creature.mirrors(roll.kind).is_none()
            })
            .collect();
        let (mut at, mut left) = (start, self.squares(me));
        let mut path: Vec<(i32, i32)> = Vec::new();
        let mut out: Option<Vec<(i32, i32)>> = None;
        loop {
            prey.sort_by_key(|&(i, pos, s)| (i != target, arena::gap(at, span, pos, s), i));
            let next = prey.iter().enumerate().find_map(|(k, &(_, pos, s))| {
                let cell = (
                    at.0.clamp(pos.0 - span + 1, pos.0 + s - 1),
                    at.1.clamp(pos.1 - span + 1, pos.1 + s - 1),
                );
                if !inside(cell) {
                    return None;
                }
                let (cells, cost) = walk(at, cell);
                if cost > left {
                    return None;
                }
                let (leaving, _) = finish(cell, left - cost)?;
                Some((k, cell, cells, cost, leaving))
            });
            let Some((k, cell, cells, cost, leaving)) = next else {
                break;
            };
            path.extend(cells);
            at = cell;
            left -= cost;
            out = Some(leaving);
            prey.remove(k);
        }
        path.extend(out?);
        self.fighters[me].pos = *path.last()?;
        Some(path)
    }

    /// Everywhere `me` could walk to this turn and stop: each square its
    /// speed reaches by a way no enemy stands in, that nobody else is
    /// standing on, with how many squares away it is and the square before
    /// it on the way. Where it is already is the first of them.
    fn reachable(&self, me: usize) -> Vec<Reached> {
        let Some((width, height)) = self.grid else {
            return Vec::new();
        };
        let budget = self.squares(me);
        let (start, span, side) = {
            let f = &self.fighters[me];
            (f.pos, f.span, f.side)
        };
        let others: Vec<((i32, i32), i32, bool)> = self
            .fighters
            .iter()
            .enumerate()
            .filter(|&(i, f)| i != me && f.alive() && f.swallowed_by().is_none())
            .map(|(_, f)| (f.pos, f.span, f.side != side))
            .collect();
        // Something that walks through bodies is stopped by none of them,
        // though it can no more stand on one than anything else can.
        let phasing = self.fighters[me].creature.phases_through().is_some();
        let blocked = |cell, only_enemies: bool| {
            !(phasing && only_enemies)
                && others.iter().any(|&(at, s, enemy)| {
                    (enemy || !only_enemies) && arena::overlaps(cell, span, at, s)
                })
        };
        let dim = 2 * budget + 1;
        let index =
            |c: (i32, i32)| ((c.1 - start.1 + budget) * dim + (c.0 - start.0 + budget)) as usize;
        let mut steps = vec![-1i32; (dim * dim) as usize];
        let mut out = vec![(start, 0, start)];
        let mut queue = VecDeque::from([start]);
        steps[index(start)] = 0;
        while let Some(cell) = queue.pop_front() {
            let taken = steps[index(cell)];
            if taken == budget {
                continue;
            }
            for (dx, dy) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let next = (cell.0 + dx, cell.1 + dy);
                if next.0 < 0
                    || next.1 < 0
                    || next.0 + span > width
                    || next.1 + span > height
                    || steps[index(next)] >= 0
                    || blocked(next, true)
                {
                    continue;
                }
                steps[index(next)] = taken + 1;
                queue.push_back(next);
                // Through a friend's square, but not to stay.
                if !blocked(next, false) {
                    out.push((next, taken + 1, cell));
                } else {
                    out.push((next, -(taken + 1), cell));
                }
            }
        }
        out
    }

    /// Walk `me` to whichever square it can reach that `score` rates
    /// highest - the nearest of them, when several rate the same - and say
    /// the way it went. `None` when `score` will have none of them, where
    /// it stands included; an empty way when where it stands is the best.
    pub(super) fn roam(
        &mut self,
        me: usize,
        score: impl Fn(&Self, (i32, i32)) -> Option<i64>,
    ) -> Option<Vec<(i32, i32)>> {
        let reached = self.reachable(me);
        let best = reached
            .iter()
            // A square somebody is on can be crossed, not stopped on.
            .filter(|&&(_, steps, _)| steps >= 0)
            .filter_map(|&(cell, steps, _)| Some((score(self, cell)?, -steps, cell)))
            .max_by_key(|&(rated, nearer, cell)| (rated, nearer, std::cmp::Reverse(cell)))?;
        let start = self.fighters[me].pos;
        let mut path = Vec::new();
        let mut cell = best.2;
        while cell != start {
            path.push(cell);
            cell = reached
                .iter()
                .find(|&&(at, _, _)| at == cell)
                .expect("every square reached was reached from one")
                .2;
        }
        path.reverse();
        self.fighters[me].pos = best.2;
        Some(path)
    }

    /// Feet between `me`, were it standing on `cell`, and `other`.
    pub(super) fn feet_from(&self, me: usize, cell: (i32, i32), other: usize) -> u32 {
        let (span, o) = (self.fighters[me].span, &self.fighters[other]);
        arena::gap(cell, span, o.pos, o.span).max(0) as u32 * SQUARE
    }

    /// Put `me` on the free square within `feet` of where it stands that is
    /// furthest from its nearest enemy - if any is further than where it is.
    /// Where it went.
    pub(super) fn teleport(&mut self, me: usize, feet: u32) -> Option<(i32, i32)> {
        let (width, height) = self.grid?;
        let reach = (feet / SQUARE) as i32;
        let (start, span, side) = {
            let f = &self.fighters[me];
            (f.pos, f.span, f.side)
        };
        let others: Vec<((i32, i32), i32, bool)> = self
            .fighters
            .iter()
            .enumerate()
            .filter(|&(i, f)| i != me && f.alive() && f.swallowed_by().is_none())
            .map(|(_, f)| (f.pos, f.span, f.side != side))
            .collect();
        // How clear a square is: the gap to the nearest enemy, and past
        // that, the gaps to all of them.
        let clear = |cell: (i32, i32)| {
            let gaps = others
                .iter()
                .filter(|o| o.2)
                .map(|&(at, s, _)| arena::gap(cell, span, at, s));
            (gaps.clone().min().unwrap_or(i32::MAX), gaps.sum::<i32>())
        };
        let mut best = (clear(start), start);
        for y in (start.1 - reach).max(0)..=(start.1 + reach).min(height - span) {
            for x in (start.0 - reach).max(0)..=(start.0 + reach).min(width - span) {
                let cell = (x, y);
                let taken = others
                    .iter()
                    .any(|&(at, s, _)| arena::overlaps(cell, span, at, s));
                if !taken && clear(cell) > best.0 {
                    best = (clear(cell), cell);
                }
            }
        }
        (best.1 != start).then(|| {
            self.fighters[me].pos = best.1;
            best.1
        })
    }

    /// Drive `who` up to `squares` squares straight away from `from`, or
    /// toward it, a square at a time and only onto free ones: a shove, a
    /// drag, a rout. The squares it went through.
    pub(super) fn displace(
        &mut self,
        who: usize,
        from: usize,
        squares: i32,
        away: bool,
    ) -> Vec<(i32, i32)> {
        let Some((width, height)) = self.grid else {
            return Vec::new();
        };
        if who == from || self.fighters[who].swallowed_by().is_some() {
            return Vec::new();
        }
        let span = self.fighters[who].span;
        let anchor = match self.pull_to {
            Some((owner, middle)) if owner == from => (middle, 1),
            _ => (self.fighters[from].pos, self.fighters[from].span),
        };
        let others: Vec<((i32, i32), i32)> = self
            .fighters
            .iter()
            .enumerate()
            .filter(|&(i, f)| i != who && f.alive() && f.swallowed_by().is_none())
            .map(|(_, f)| (f.pos, f.span))
            .collect();
        let free = |cell: (i32, i32)| {
            cell.0 >= 0
                && cell.1 >= 0
                && cell.0 + span <= width
                && cell.1 + span <= height
                && !others
                    .iter()
                    .any(|&(at, s)| arena::overlaps(cell, span, at, s))
        };
        // How far a square is, and which way is straight away: both read
        // off the line between the middles of the two.
        let line = |cell: (i32, i32)| {
            (
                (2 * cell.0 + span) - (2 * anchor.0 .0 + anchor.1),
                (2 * cell.1 + span) - (2 * anchor.0 .1 + anchor.1),
            )
        };
        let far = |cell: (i32, i32)| {
            let (dx, dy) = line(cell);
            (
                arena::gap(cell, span, anchor.0, anchor.1),
                dx * dx + dy * dy,
            )
        };
        let mut cell = self.fighters[who].pos;
        let mut path = Vec::new();
        for _ in 0..squares {
            let here = far(cell);
            if !away && here.0 <= 1 {
                break;
            }
            let (dx, dy) = line(cell);
            let toward = if away { 1 } else { -1 };
            let steps = [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ];
            // The free square most nearly along that line, among those that
            // do put it further off (or nearer): straight back if it can,
            // glancing off whatever is in the way if it cannot.
            let next = steps
                .iter()
                .map(|&(sx, sy)| ((sx, sy), (cell.0 + sx, cell.1 + sy)))
                .filter(|&(_, c)| free(c))
                .filter(|&(_, c)| if away { far(c) > here } else { far(c) < here })
                .max_by_key(|&((sx, sy), _)| {
                    // The cosine of the angle off the line, squared and
                    // signed, kept in whole numbers.
                    let dot = toward * (sx * dx + sy * dy);
                    (dot * dot.abs() / (sx * sx + sy * sy), dot)
                });
            let Some((_, to)) = next else { break };
            cell = to;
            path.push(to);
        }
        self.fighters[who].pos = cell;
        path
    }

    /// Stand `who` on the free square nearest to `beside` - where a summon
    /// appears.
    pub(super) fn place_beside(&mut self, who: usize, beside: usize) {
        let Some((width, height)) = self.grid else {
            return;
        };
        let span = self.fighters[who].span;
        let origin = self.fighters[beside].pos;
        let free = |cell: (i32, i32)| {
            cell.0 >= 0
                && cell.1 >= 0
                && cell.0 + span <= width
                && cell.1 + span <= height
                && !self.fighters.iter().enumerate().any(|(i, f)| {
                    i != who && f.alive() && arena::overlaps(cell, span, f.pos, f.span)
                })
        };
        for ring in 1..width.max(height) {
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    let cell = (origin.0 + dx, origin.1 + dy);
                    if dx.abs().max(dy.abs()) == ring && free(cell) {
                        self.fighters[who].pos = cell;
                        return;
                    }
                }
            }
        }
    }
}
