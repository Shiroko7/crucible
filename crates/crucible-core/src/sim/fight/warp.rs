//! Damage and bodies that do not go where they were sent: a hide only
//! enchanted steel bites, a mind that hands what hurts it to whoever is
//! standing near, a creature that changes places with another as the blow
//! lands, one that walks through its enemies, one that folds space around a
//! victim - and hiding, which is the plainest way of not being where one was.

use crate::creature::{Displace, Rider, Strike};
use crate::prob::Rng;
use crate::rules::{Condition, DamageKind, DamageRoll, Landed, Reduction};
use crate::sim::arena::{self, SQUARE};
use crate::sim::fight::events;
use crate::sim::fight::record::FrameKind;
use crate::sim::fight::threshold::reducer;
use crate::sim::fight::{Expiry, Fight};
use crate::sim::Side;

impl<'a> Fight<'a> {
    /// Is `strike`, made by `me` with a move carrying `riders`, magical?
    /// A spell attack is, and so is anything a spell or a magic item does,
    /// a swing its move or its maker says is, and one a boon is riding.
    fn magical(&self, me: usize, strike: &Strike, riders: &[Rider]) -> bool {
        strike.kind.spell
            || self.cast.get().magical
            || riders.contains(&Rider::MagicWeapons)
            || self.fighters[me]
                .creature
                .riders
                .contains(&Rider::MagicWeapons)
            || !self.boon_damage(me, strike).is_empty()
    }

    /// The damage types `target` resists beyond what its stat block lists,
    /// against `strike` from `me` - or against something that is no attack
    /// roll at all, when `None`: whatever a boon it holds shrugs off, and
    /// what it resists from an attack that is not magical
    /// ([`Rider::ResistNonmagical`]).
    pub(super) fn defending(
        &self,
        me: usize,
        target: usize,
        strike: Option<&Strike>,
        riders: &[Rider],
    ) -> Vec<DamageKind> {
        let mut out = self.boon_resistances(target);
        if let Some(strike) = strike {
            if !self.magical(me, strike, riders) {
                out.extend(self.fighters[target].creature.resists_nonmagical());
            }
        }
        out
    }

    /// Everyone within `feet` of `center` but `center` itself, whichever
    /// side they are on. With nowhere to stand there is no telling who is
    /// near, so it is everyone on `otherwise` - the side whatever is asking
    /// is meant for.
    pub(super) fn around(&self, center: usize, feet: u32, otherwise: Side) -> Vec<usize> {
        let (at, span) = (self.fighters[center].pos, self.fighters[center].span);
        (0..self.fighters.len())
            .filter(|&i| {
                let f = &self.fighters[i];
                i != center
                    && f.alive()
                    && self.reaches(center, i)
                    && match self.on_grid() {
                        true => arena::gap(at, span, f.pos, f.span).max(0) as u32 * SQUARE <= feet,
                        false => f.side == otherwise,
                    }
            })
            .collect()
    }

    /// `target` is about to take `raw` damage, made up of `kinds` as it was
    /// rolled. Whatever of it `target` hands on - see
    /// [`Rider::MirrorDamage`] - lands on everyone around it instead, each
    /// by its own resistances, and what is left for `target` comes back.
    ///
    /// Handed on once: another mirror it lands on takes none of it and
    /// passes none of it along, or two of them side by side would never
    /// stop.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn mirror(
        &mut self,
        rng: &mut Rng,
        target: usize,
        raw: i32,
        kinds: &[(DamageKind, i32)],
        record: bool,
        notes: &mut Vec<String>,
    ) -> i32 {
        let holder = self.fighters[target].creature;
        let rolled: i32 = kinds.iter().map(|&(_, n)| n).sum();
        if raw <= 0 || rolled <= 0 {
            return raw;
        }
        // What got through of each type, when something took a share of
        // the whole - and how far it is thrown.
        let shares: Vec<(DamageKind, i32, u32)> = kinds
            .iter()
            .filter(|&&(_, n)| n > 0)
            .filter_map(|&(kind, n)| {
                let feet = holder.mirrors(kind)?;
                Some((
                    kind,
                    (i64::from(n) * i64::from(raw) / i64::from(rolled)) as i32,
                    feet,
                ))
            })
            .collect();
        let handed: i32 = shares.iter().map(|&(_, n, _)| n).sum();
        if handed <= 0 {
            return raw;
        }
        let side = self.fighters[target].side;
        let reach = shares.iter().map(|&(_, _, feet)| feet).max().unwrap_or(0);
        let mut took = Vec::new();
        for i in self.around(target, reach, side.other()) {
            let bystander = self.fighters[i].creature;
            let hurt: i32 = shares
                .iter()
                .filter(|&&(kind, _, _)| bystander.mirrors(kind).is_none())
                .map(|&(kind, n, _)| bystander.reduction(kind).apply(n))
                .sum();
            if hurt <= 0 {
                continue;
            }
            if self.fighters[i].side != side {
                self.fighters[target].dealt += i64::from(hurt);
            }
            self.apply_damage(rng, i, hurt);
            took.push(format!("{} {hurt}", bystander.name));
        }
        let text = match took.is_empty() {
            true => format!("{handed} mirrored onto nobody"),
            false => format!("{handed} mirrored: {}", took.join(", ")),
        };
        if record {
            notes.push(text.clone());
        }
        self.beat(
            FrameKind::Other,
            Some(target),
            None,
            format!("{} hands it on - {text}", holder.name),
            None,
        );
        raw - handed
    }

    /// Damage from `from` that simply lands on `to`: no attack roll and no
    /// save, only `to`'s own resistances - and whatever it does with damage
    /// it will not take. What got through.
    pub(super) fn harm(
        &mut self,
        rng: &mut Rng,
        from: usize,
        to: usize,
        rolls: &[DamageRoll],
        record: bool,
        notes: &mut Vec<String>,
    ) -> i32 {
        if rolls.is_empty() || !self.fighters[to].alive() {
            return 0;
        }
        let attacker = self.fighters[from].creature;
        let against = self.fighters[to].creature;
        let defending = self.boon_resistances(to);
        let reduce = reducer(attacker, against, false, &defending);
        rng.rolled();
        let kinds: Vec<(DamageKind, i32)> = rolls
            .iter()
            .map(|roll| {
                (
                    roll.kind_against(&reduce),
                    roll.sample(rng, false, roll.reduction_against(&reduce)),
                )
            })
            .collect();
        let raw: i32 = kinds.iter().map(|&(_, n)| n).sum();
        if self.recording() {
            let dice = rng.rolled();
            self.hurt = Some(events::Hurt::of(rolls, &dice));
        }
        let raw = self.mirror(rng, to, raw, &kinds, record, notes);
        let (dealt, _) = self.deal(rng, from, to, raw, false);
        self.hurt = None;
        dealt
    }

    /// `me` tries to hide: a Stealth check at `bonus` against the keenest
    /// passive Perception on the other side. Whether it worked.
    pub(super) fn hide(
        &mut self,
        rng: &mut Rng,
        me: usize,
        bonus: i32,
        record: bool,
        notes: &mut Vec<String>,
    ) -> bool {
        let dc = self.keenest(me);
        let total = rng.die(20) + bonus - 2 * i32::from(self.fighters[me].exhaustion);
        let hidden = total >= dc;
        if hidden {
            self.apply_condition(me, Condition::Hidden, Expiry::Rounds { who: me, left: 600 });
            self.afflicted(me, me, Condition::Hidden);
        } else {
            let name = &self.fighters[me].creature.name;
            let text = format!("{name} fails to hide: Stealth {total} against {dc}");
            self.beat(FrameKind::Other, Some(me), None, text, None);
        }
        if record {
            notes.push(match hidden {
                true => format!("hidden ({total} vs {dc})"),
                false => format!("seen ({total} vs {dc})"),
            });
        }
        hidden
    }

    /// The best passive Perception among `me`'s enemies still standing.
    pub(super) fn keenest(&self, me: usize) -> i32 {
        let side = self.fighters[me].side;
        self.fighters
            .iter()
            .filter(|f| f.side != side && f.alive() && !f.incapacitated())
            .map(|f| f.creature.passive_perception())
            .max()
            .unwrap_or(10)
    }

    /// An attack from `me` has just hit `target`. If `target` can change
    /// places with one of its own - see [`Rider::SwapWhenHit`] - it does,
    /// and that one is who the blow lands on: the one with the most hit
    /// points left, of those near enough and of the right kind.
    pub(super) fn bend(&mut self, target: usize) -> Option<usize> {
        let f = &self.fighters[target];
        if !f.reaction || f.incapacitated() || f.swallowed_by().is_some() {
            return None;
        }
        let (feet, kin) = f.creature.riders.iter().find_map(|r| match r {
            Rider::SwapWhenHit { feet, kin } => Some((*feet, kin.to_ascii_lowercase())),
            _ => None,
        })?;
        let other = (0..self.fighters.len())
            .filter(|&i| {
                let o = &self.fighters[i];
                i != target
                    && o.side == f.side
                    && o.alive()
                    && o.swallowed_by().is_none()
                    && self.reaches(target, i)
                    && o.creature.name.to_ascii_lowercase().contains(&kin)
                    && (!self.on_grid()
                        || arena::gap(f.pos, f.span, o.pos, o.span).max(0) as u32 * SQUARE <= feet)
                    && self.can_swap(target, i)
            })
            .max_by_key(|&i| (self.fighters[i].hp, std::cmp::Reverse(i)))?;
        self.fighters[target].spend_reaction_budget();
        let (a, b) = (self.fighters[target].pos, self.fighters[other].pos);
        self.fighters[target].pos = b;
        self.fighters[other].pos = a;
        let name = |i: usize| self.fighters[i].creature.name.clone();
        let text = format!(
            "{} changes places with {}, which is hit instead",
            name(target),
            name(other)
        );
        self.beat(FrameKind::Other, Some(target), Some(other), text, None);
        Some(other)
    }

    /// Would `a` and `b` each fit where the other stands, with nobody else
    /// in the way? Always, with nowhere to stand.
    fn can_swap(&self, a: usize, b: usize) -> bool {
        let Some((width, height)) = self.grid else {
            return true;
        };
        let fits = |who: usize, cell: (i32, i32)| {
            let span = self.fighters[who].span;
            cell.0 + span <= width
                && cell.1 + span <= height
                && !self.fighters.iter().enumerate().any(|(i, f)| {
                    i != a
                        && i != b
                        && f.alive()
                        && f.swallowed_by().is_none()
                        && arena::overlaps(cell, span, f.pos, f.span)
                })
        };
        let (at_a, at_b) = (self.fighters[a].pos, self.fighters[b].pos);
        // Two of different sizes would overlap one another where a corner
        // is all they trade.
        let apart = self.fighters[a].span == self.fighters[b].span
            || !arena::overlaps(at_b, self.fighters[a].span, at_a, self.fighters[b].span);
        apart && fits(a, at_b) && fits(b, at_a)
    }

    /// A hit that was rolled against somebody else lands on `struck`: its
    /// damage, rolled against what `struck` resists.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn landed_instead(
        &mut self,
        rng: &mut Rng,
        me: usize,
        struck: usize,
        strike: &Strike,
        riders: &[Rider],
        extra: &[DamageRoll],
        landed: Landed,
        kinds: &mut Vec<(DamageKind, i32)>,
    ) -> i32 {
        let attacker = self.fighters[me].creature;
        let against = self.fighters[struck].creature;
        let defending = self.defending(me, struck, Some(strike), riders);
        let reduce = reducer(attacker, against, false, &defending);
        let rolls: Vec<DamageRoll> = strike.damage.iter().chain(extra).copied().collect();
        kinds.clear();
        rng.rolled();
        let raw = rolls
            .iter()
            .map(|roll| {
                let dealt =
                    roll.sample(rng, landed == Landed::Crit, roll.reduction_against(&reduce));
                kinds.push((roll.kind_against(&reduce), dealt));
                dealt
            })
            .sum();
        if self.recording() {
            let dice = rng.rolled();
            self.hurt = Some(events::Hurt::of(&rolls, &dice));
        }
        raw
    }

    /// Fold space around `target` - see [`Displace`].
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fold(
        &mut self,
        rng: &mut Rng,
        me: usize,
        target: usize,
        d: &Displace,
        record: bool,
        notes: &mut Vec<String>,
    ) {
        let name = self.fighters[target].creature.name.clone();
        let advantage = self.resists_magic(target);
        let (saved, resisted) = self.saving_throw(rng, target, d.ability, d.dc, advantage);
        if saved {
            let dealt = self.harm(rng, me, target, &d.on_success, record, notes);
            if record {
                let how = match resisted {
                    true => "legendary resistance",
                    false => "saved",
                };
                notes.push(format!("{name} {how} for {dealt}"));
            }
            return;
        }
        let side = self.fighters[me].side;
        let origin = (self.fighters[target].pos, self.fighters[target].span);
        if let Some(to) = self.throw(me, target, d) {
            let text = format!("{name} is somewhere else");
            self.frame(
                self.round,
                FrameKind::Move,
                Some(target),
                None,
                text,
                vec![to],
            );
        }
        // Everything near where it stood - itself too, if that is where it
        // was set down again.
        let caught: Vec<usize> = (0..self.fighters.len())
            .filter(|&i| {
                let f = &self.fighters[i];
                f.alive()
                    && f.swallowed_by().is_none()
                    && !f.has(|c| c == Condition::Banished)
                    && match self.on_grid() {
                        true => {
                            arena::gap(origin.0, origin.1, f.pos, f.span).max(0) as u32 * SQUARE
                                <= d.radius
                        }
                        false => f.side != side && (i != target || !d.away),
                    }
            })
            .collect();
        if record {
            notes.push(format!("{name} failed and is thrown"));
        }
        for i in caught {
            let dealt = self.harm(rng, me, i, &d.blast, record, notes);
            if record {
                notes.push(format!("{} {dealt}", self.fighters[i].creature.name));
            }
        }
    }

    /// Put `target` down somewhere else within `d.feet`: where the blast
    /// still catches it, or - thrown `away` - clear of it, and in either
    /// case as far from `me` as that leaves. Where it went, if anywhere.
    fn throw(&mut self, me: usize, target: usize, d: &Displace) -> Option<(i32, i32)> {
        let (width, height) = self.grid?;
        let (start, span) = (self.fighters[target].pos, self.fighters[target].span);
        let thrower = (self.fighters[me].pos, self.fighters[me].span);
        let reach = (d.feet / SQUARE) as i32;
        let free = |cell: (i32, i32)| {
            !self.fighters.iter().enumerate().any(|(i, f)| {
                i != target
                    && f.alive()
                    && f.swallowed_by().is_none()
                    && arena::overlaps(cell, span, f.pos, f.span)
            })
        };
        let mut best: Option<((i32, i32), (i32, i32))> = None;
        for y in (start.1 - reach).max(0)..=(start.1 + reach).min(height - span) {
            for x in (start.0 - reach).max(0)..=(start.0 + reach).min(width - span) {
                let cell = (x, y);
                let inside = arena::gap(start, span, cell, span).max(0) as u32 * SQUARE <= d.radius;
                if cell == start || inside == d.away || !free(cell) {
                    continue;
                }
                let rated = (
                    arena::gap(cell, span, thrower.0, thrower.1),
                    arena::gap(cell, span, start, span),
                );
                if best.is_none_or(|(most, _)| rated > most) {
                    best = Some((rated, cell));
                }
            }
        }
        let (_, to) = best?;
        self.fighters[target].pos = to;
        Some(to)
    }

    /// What folding space around `target` is worth to `me`: what the blast
    /// does to everyone it would catch, counted against `me` for its own
    /// side, when the save fails - and what a success costs when it does
    /// not.
    pub(super) fn fold_value(&self, me: usize, target: usize, d: &Displace) -> f64 {
        let side = self.fighters[me].side;
        let fail = d.failure_chance(self.fighters[target].creature);
        let mean = |i: usize, rolls: &[DamageRoll]| {
            let c = self.fighters[i].creature;
            Displace::mean_of(rolls, &|k| match c.mirrors(k) {
                Some(_) => Reduction::Immune,
                None => c.reduction(k),
            })
        };
        let mut blast: f64 = self
            .around(target, d.radius, side.other())
            .into_iter()
            .map(|i| match self.fighters[i].side == side {
                true => -mean(i, &d.blast),
                false => mean(i, &d.blast),
            })
            .sum();
        if !d.away {
            blast += mean(target, &d.blast);
        }
        fail * blast + (1.0 - fail) * mean(target, &d.on_success)
    }

    /// `me` has walked along `path`: everyone whose space it passed through
    /// on the way takes `roll` - see [`Rider::PhaseThrough`].
    pub(super) fn pass_through(
        &mut self,
        rng: &mut Rng,
        me: usize,
        path: &[(i32, i32)],
        roll: DamageRoll,
        record: bool,
        notes: &mut Vec<String>,
    ) {
        let span = self.fighters[me].span;
        let Some((_, way)) = path.split_last() else {
            return;
        };
        let crossed: Vec<usize> = (0..self.fighters.len())
            .filter(|&i| {
                let f = &self.fighters[i];
                i != me
                    && f.alive()
                    && f.swallowed_by().is_none()
                    && way
                        .iter()
                        .any(|&cell| arena::overlaps(cell, span, f.pos, f.span))
            })
            .collect();
        for i in crossed {
            let dealt = self.harm(rng, me, i, &[roll], record, notes);
            if record {
                notes.push(format!(
                    "passes through {} for {dealt}",
                    self.fighters[i].creature.name
                ));
            }
        }
    }
}
