//! Where enemies stand around a creature with a mouth, how far each of its
//! moves reaches, and how it moves on its turn.
//!
//! This is not a map. There are no coordinates, only four places an enemy
//! can be relative to one big creature - enough to say who its bite can
//! reach, who can see into its mouth, and how long it takes to close in.
//! See [`crate::creature::Creature::mouth`].

/// Where an enemy stands relative to a creature with a mouth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    /// In front of it and within reach of its mouth: a melee attack here
    /// reaches the mouth, and the mouth reaches back.
    Mouth,
    /// Beside its body, in its reach but away from the mouth: a melee attack
    /// reaches only the shell, and nothing here sees into the mouth.
    Body,
    /// In front of it, out of its reach but one move from closing in. A
    /// ranged attack from here sees into the mouth.
    Range,
    /// In front of it, a move further out than [`Zone::Range`] - where it
    /// leaves its enemies when it withdraws.
    Far,
}

impl Zone {
    pub fn name(self) -> &'static str {
        match self {
            Zone::Mouth => "the mouth",
            Zone::Body => "the body",
            Zone::Range => "range",
            Zone::Far => "far off",
        }
    }

    /// Within its melee reach, where a melee attack can reach it back.
    pub fn in_melee(self) -> bool {
        matches!(self, Zone::Mouth | Zone::Body)
    }

    /// In front of it, with a line of sight into its mouth.
    pub fn in_front(self) -> bool {
        !matches!(self, Zone::Body)
    }

    /// How far from the mouth, in moves: what "safest" and "nearest" rank on.
    pub fn distance(self) -> u32 {
        match self {
            Zone::Mouth => 0,
            Zone::Body => 1,
            Zone::Range => 2,
            Zone::Far => 3,
        }
    }

    /// One move closer to the mouth - where a pull leaves it.
    pub fn pulled(self) -> Zone {
        match self {
            Zone::Far => Zone::Range,
            Zone::Range | Zone::Body | Zone::Mouth => Zone::Mouth,
        }
    }

    /// One move away from it - where a push leaves it.
    pub fn pushed(self) -> Zone {
        match self {
            Zone::Mouth | Zone::Body => Zone::Range,
            Zone::Range | Zone::Far => Zone::Far,
        }
    }

    /// The zones one move from here. Range opens onto the mouth and the
    /// body alike; the body runs along to the mouth.
    fn neighbours(self) -> &'static [Zone] {
        match self {
            Zone::Mouth => &[Zone::Body, Zone::Range],
            Zone::Body => &[Zone::Mouth, Zone::Range],
            Zone::Range => &[Zone::Mouth, Zone::Body, Zone::Far],
            Zone::Far => &[Zone::Range],
        }
    }

    /// Every zone within `moves` of here, this one included, nearest to the
    /// mouth first.
    pub fn within(self, moves: u32) -> Vec<Zone> {
        let mut out = vec![self];
        let mut frontier = vec![self];
        for _ in 0..moves {
            let mut next = Vec::new();
            for z in frontier {
                for &n in z.neighbours() {
                    if !out.contains(&n) {
                        out.push(n);
                        next.push(n);
                    }
                }
            }
            frontier = next;
        }
        out.sort_by_key(|z| z.distance());
        out
    }
}

/// Which zones a move by a creature with a mouth reaches. Every other
/// creature's moves reach everywhere, as they always have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Reach {
    /// Anywhere around it: an aura, a whirlpool's pull, a burst from a crack.
    #[default]
    Any,
    /// Only [`Zone::Mouth`]: a bite.
    Mouth,
    /// Anywhere in its melee reach - [`Zone::Mouth`] and [`Zone::Body`]: a
    /// slam, a tail, a burst around its body.
    Near,
    /// Everything in front of it - every zone but [`Zone::Body`]: a breath
    /// weapon's cone out of its mouth.
    Front,
}

impl Reach {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "any" | "all" => Reach::Any,
            "mouth" => Reach::Mouth,
            "near" | "body" => Reach::Near,
            "front" | "cone" => Reach::Front,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Reach::Any => "any",
            Reach::Mouth => "mouth",
            Reach::Near => "near",
            Reach::Front => "front",
        }
    }

    pub fn covers(self, zone: Zone) -> bool {
        match self {
            Reach::Any => true,
            Reach::Mouth => zone == Zone::Mouth,
            Reach::Near => zone.in_melee(),
            Reach::Front => zone.in_front(),
        }
    }

    /// The reach of a part of a move whose own reach is `self`, inside a
    /// move reaching `outer`: its own, unless it never said.
    pub fn within(self, outer: Reach) -> Reach {
        match self {
            Reach::Any => outer,
            own => own,
        }
    }
}

/// How a creature with a mouth moves on its turn. It is always the faster
/// swimmer, so where it goes decides who ends up at its mouth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tactic {
    /// Stays where it is and lets its enemies - and whatever drags them - come
    /// to it.
    Hold,
    /// Every turn, swims at the nearest enemy until it is at its mouth.
    #[default]
    Charge,
    /// Charges the nearest enemy, acts, then withdraws until every enemy is
    /// [`Zone::Far`] - spending its bonus action to get clear, so it cannot
    /// take one that turn.
    HitAndRun,
}

impl Tactic {
    pub const ALL: [Tactic; 3] = [Tactic::Hold, Tactic::Charge, Tactic::HitAndRun];

    pub fn parse(word: &str) -> Option<Self> {
        Some(
            match word
                .trim()
                .to_ascii_lowercase()
                .replace(['-', '_'], " ")
                .as_str()
            {
                "hold" => Tactic::Hold,
                "charge" => Tactic::Charge,
                "hit and run" => Tactic::HitAndRun,
                _ => return None,
            },
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            Tactic::Hold => "hold",
            Tactic::Charge => "charge",
            Tactic::HitAndRun => "hit-and-run",
        }
    }

    /// One line on what the row means.
    pub fn blurb(self) -> &'static str {
        match self {
            Tactic::Hold => "stays put and lets them come",
            Tactic::Charge => "swims at the nearest enemy every turn",
            Tactic::HitAndRun => "charges, acts, then withdraws out of reach",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_closes_one_move_and_a_push_opens_one() {
        assert_eq!(Zone::Far.pulled(), Zone::Range);
        assert_eq!(Zone::Range.pulled(), Zone::Mouth);
        assert_eq!(Zone::Body.pulled(), Zone::Mouth);
        assert_eq!(Zone::Mouth.pushed(), Zone::Range);
        assert_eq!(Zone::Body.pushed(), Zone::Range);
        assert_eq!(Zone::Range.pushed(), Zone::Far);
    }

    #[test]
    fn one_move_from_range_reaches_everything_but_two_moves_from_far_are_needed() {
        assert_eq!(
            Zone::Range.within(1),
            vec![Zone::Mouth, Zone::Body, Zone::Range, Zone::Far]
        );
        assert_eq!(Zone::Far.within(1), vec![Zone::Range, Zone::Far]);
        assert_eq!(Zone::Far.within(0), vec![Zone::Far]);
        assert_eq!(Zone::Far.within(2).len(), 4);
    }

    #[test]
    fn each_reach_covers_the_zones_its_shape_does() {
        let covered = |r: Reach| {
            [Zone::Mouth, Zone::Body, Zone::Range, Zone::Far]
                .into_iter()
                .filter(|&z| r.covers(z))
                .collect::<Vec<_>>()
        };
        assert_eq!(covered(Reach::Mouth), vec![Zone::Mouth]);
        assert_eq!(covered(Reach::Near), vec![Zone::Mouth, Zone::Body]);
        assert_eq!(
            covered(Reach::Front),
            vec![Zone::Mouth, Zone::Range, Zone::Far]
        );
        assert_eq!(covered(Reach::Any).len(), 4);
        assert_eq!(Reach::Any.within(Reach::Near), Reach::Near);
        assert_eq!(Reach::Mouth.within(Reach::Near), Reach::Mouth);
    }

    #[test]
    fn tactics_parse_however_they_are_written() {
        for t in Tactic::ALL {
            assert_eq!(Tactic::parse(t.name()), Some(t));
        }
        assert_eq!(Tactic::parse("Hit and run"), Some(Tactic::HitAndRun));
        assert_eq!(Tactic::parse("hit_and_run"), Some(Tactic::HitAndRun));
        assert_eq!(Tactic::parse("dance"), None);
    }
}
