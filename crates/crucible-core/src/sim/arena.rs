//! The grid a fight is played on: five-foot squares, and where everyone
//! starts.
//!
//! A fight has a grid when every creature in it has somewhere to stand
//! ([`Creature::at`]). Nothing here is read otherwise, which is how a duel
//! checked against exact arithmetic stays the duel it was.

use crate::creature::Creature;
use crate::rules::Size;

/// Feet to a square.
pub const SQUARE: u32 = 5;

/// The smallest map a fight is given, in squares across and down: 200 feet
/// by 150. Room to back away in, and to come round the side of something.
pub const FIELD: (i32, i32) = (40, 30);

/// Empty squares left around the starting positions.
pub const MARGIN: i32 = 4;

/// The tallest a starting rank gets before a side forms a second one.
const RANK: i32 = 14;

/// How many squares a creature of this size takes up along each side.
pub fn span(size: Size) -> i32 {
    match size {
        Size::Tiny | Size::Small | Size::Medium => 1,
        Size::Large => 2,
        Size::Huge => 3,
        Size::Gargantuan => 4,
    }
}

/// The squares between two spaces, counted the way 5e counts distance on a
/// grid - a diagonal is one square. Adjacent spaces are 1 apart; overlapping
/// ones are 0.
pub fn gap(a: (i32, i32), a_span: i32, b: (i32, i32), b_span: i32) -> i32 {
    let along = |a: i32, a_span: i32, b: i32, b_span: i32| {
        (a - (b + b_span - 1)).max(b - (a + a_span - 1)).max(0)
    };
    along(a.0, a_span, b.0, b_span).max(along(a.1, a_span, b.1, b_span))
}

pub fn overlaps(a: (i32, i32), a_span: i32, b: (i32, i32), b_span: i32) -> bool {
    gap(a, a_span, b, b_span) == 0
}

/// A starting rank: who is in it, how wide it is, and how tall.
type Rank = (Vec<usize>, i32, i32);

/// Give everyone without a starting square one: the first side in ranks on
/// the left, the second on the right, `apart` feet between their front
/// lines. A square a scenario chose is kept, unless somebody already stands
/// there - which is what copies of one creature would otherwise do.
pub fn place(creatures: &mut [Creature], apart: u32) {
    let mut taken: Vec<((i32, i32), i32)> = Vec::new();
    for c in creatures.iter_mut() {
        let s = span(c.size);
        match c.at {
            Some(at) if !taken.iter().any(|&(b, bs)| overlaps(at, s, b, bs)) => {
                taken.push((at, s));
            }
            _ => c.at = None,
        }
    }

    // Each side's ranks, front rank first.
    let ranks = |team: u8| {
        let mut out: Vec<Rank> = vec![(Vec::new(), 0, 0)];
        for (i, c) in creatures.iter().enumerate() {
            if c.team != team || c.at.is_some() {
                continue;
            }
            let s = span(c.size);
            if out.last().is_some_and(|r| r.2 > 0 && r.2 + 1 + s > RANK) {
                out.push((Vec::new(), 0, 0));
            }
            let rank = out.last_mut().expect("never empty");
            rank.2 += s + i32::from(rank.2 > 0);
            rank.1 = rank.1.max(s);
            rank.0.push(i);
        }
        out
    };
    let sides = [ranks(0), ranks(1)];
    let depth = (sides[0].iter().map(|r| r.1 + 1).sum::<i32>() - 1).max(1);
    let tallest = sides.iter().flatten().map(|r| r.2).max().unwrap_or(1);
    let front = [MARGIN + depth - 1, MARGIN + depth + (apart / SQUARE) as i32];

    for (team, side) in sides.iter().enumerate() {
        let mut edge = front[team];
        for (members, wide, tall) in side {
            let mut y = MARGIN + (tallest - tall) / 2;
            for &i in members {
                let s = span(creatures[i].size);
                // The first side backs away from its front line leftwards,
                // the second rightwards.
                let x = if team == 0 { edge - s + 1 } else { edge };
                let mut at = (x, y);
                while taken.iter().any(|&(b, bs)| overlaps(at, s, b, bs)) {
                    at.1 += 1;
                }
                creatures[i].at = Some(at);
                taken.push((at, s));
                y += s + 1;
            }
            edge += if team == 0 { -(wide + 1) } else { wide + 1 };
        }
    }
}

/// The size of the grid a roster starts on: everyone's space, and a margin.
pub fn bounds(creatures: &[&Creature]) -> (i32, i32) {
    let far = |pick: fn((i32, i32)) -> i32| {
        creatures
            .iter()
            .filter_map(|c| c.at.map(|at| pick(at) + span(c.size)))
            .max()
            .unwrap_or(0)
            + MARGIN
    };
    (far(|at| at.0), far(|at| at.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_is_counted_between_spaces_and_a_diagonal_is_one_square() {
        assert_eq!(gap((0, 0), 1, (1, 0), 1), 1, "adjacent");
        assert_eq!(gap((0, 0), 1, (1, 1), 1), 1, "diagonally adjacent");
        assert_eq!(gap((0, 0), 1, (7, 3), 1), 7);
        assert_eq!(gap((0, 0), 2, (2, 0), 1), 1, "beside a large creature");
        assert_eq!(gap((0, 0), 3, (2, 2), 1), 0, "inside its space");
    }

    #[test]
    fn two_sides_start_apart_with_nobody_sharing_a_square() {
        let mut ogre = Creature::new("ogre", 11, 59);
        ogre.size = Size::Large;
        ogre.team = 1;
        let hero = Creature::new("hero", 15, 40);
        let mut roster = vec![hero.clone(), hero, ogre.clone(), ogre.clone(), ogre];
        place(&mut roster, 30);

        for (i, a) in roster.iter().enumerate() {
            for b in &roster[i + 1..] {
                let d = gap(a.at.unwrap(), span(a.size), b.at.unwrap(), span(b.size));
                assert!(d > 0, "{} and {} overlap", a.name, b.name);
                if a.team != b.team {
                    assert!(d >= 7, "front lines start 30 feet apart, got {d}");
                }
            }
        }
        let nearest = gap(roster[0].at.unwrap(), 1, roster[2].at.unwrap(), 2);
        assert_eq!(nearest, 7, "six empty squares between the front lines");
    }

    #[test]
    fn a_chosen_square_is_kept_unless_somebody_is_already_on_it() {
        let mut a = Creature::new("a", 10, 10);
        a.at = Some((20, 20));
        let mut roster = vec![a.clone(), a];
        place(&mut roster, 30);
        assert_eq!(roster[0].at, Some((20, 20)));
        assert_ne!(roster[1].at, Some((20, 20)), "the copy is moved");
        assert!(roster[1].at.is_some());
    }
}
