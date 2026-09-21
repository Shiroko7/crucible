//! The playstyles a side is played under - `DESIGN.md`'s "Playstyles are the
//! product". How each one actually picks a move mid-fight is
//! `sim::fight`'s business, next to everything else it decides.

/// How a combatant picks its move, its target, and whether it spends anything.
///
/// This is the seam where the playstyle library from `DESIGN.md` plugs in. The
/// point of having nine rather than one is that the *spread between them* is the
/// output: if every playstyle wins, the encounter is filler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Take the first usable move as written. Declaration order in the scenario
    /// file is the policy, which makes it the easiest to author against and the
    /// one to reach for when debugging.
    InOrder,
    /// Take whichever usable move has the highest exact mean damage. The ceiling
    /// of the non-searching policies, and still nowhere near optimal play: it
    /// never defends, never retreats, and never holds a resource for a better
    /// moment.
    Greedy,
    /// Spend nothing - no limited uses, no recharge moves, no resource pools, no
    /// Legendary Resistance. Models the player saving the slot for later and the
    /// DM running a dragon as a set of claws, which turn out to be the same
    /// policy.
    Thrifty,
    /// Front-load: spend the most expensive thing available first, breaking ties
    /// on damage. The table that ends fights in two rounds and has nothing left
    /// for the third.
    Nova,
    /// Spend nothing until bloodied, then everything. The most common real
    /// player - "I might need it later" - and the row that most often surprises
    /// a DM.
    Attrition,
    /// Concentrate on the enemy with the least HP left: finish the wounded one.
    /// A party that communicates, and a monster that knows which character to
    /// kill. Only distinguishable when the enemies differ, so against a single
    /// target it is greedy.
    FocusFire,
    /// Each combatant attacks a different enemy, spreading damage around. A
    /// party that does not communicate, and the mistake a DM makes when they
    /// want a fight to feel fair. Usually the weakest targeting rule there is.
    Scattered,
    /// Greedy until bloodied, then prefer a defensive stance for the bonus
    /// action. The spooked table.
    Defensive,
    /// Search: try every legal turn, play each out to a depth budget many times,
    /// and keep the one with the best estimated value.
    ///
    /// This is flat Monte Carlo - one ply of real choice, then rollouts - not
    /// the UCT tree search `DESIGN.md` asks for, and the difference is worth
    /// being honest about: it picks the best turn but cannot plan a sequence of
    /// them, and it does not search over targets. Putting a tree in its place is
    /// a change to one function.
    Solver,
}

impl Policy {
    /// Every policy, best-play first, so a report reads top to bottom as a
    /// descent from the ceiling.
    pub const ALL: [Policy; 9] = [
        Policy::Solver,
        Policy::Nova,
        Policy::Greedy,
        Policy::FocusFire,
        Policy::Scattered,
        Policy::InOrder,
        Policy::Defensive,
        Policy::Attrition,
        Policy::Thrifty,
    ];

    /// Everything except the search, for sweeps where rollouts inside rollouts
    /// are not worth the time.
    pub const CHEAP: [Policy; 8] = [
        Policy::Nova,
        Policy::Greedy,
        Policy::FocusFire,
        Policy::Scattered,
        Policy::InOrder,
        Policy::Defensive,
        Policy::Attrition,
        Policy::Thrifty,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Policy::InOrder => "in-order",
            Policy::Greedy => "greedy",
            Policy::Thrifty => "thrifty",
            Policy::Nova => "nova",
            Policy::Attrition => "attrition",
            Policy::FocusFire => "focus-fire",
            Policy::Scattered => "scattered",
            Policy::Defensive => "defensive",
            Policy::Solver => "solver",
        }
    }

    /// One line on what the row means, for a report that has nine of them.
    pub fn blurb(self) -> &'static str {
        match self {
            Policy::InOrder => "the first usable move, as written",
            Policy::Greedy => "always the highest mean damage available",
            Policy::Thrifty => "spends nothing, ever",
            Policy::Nova => "spends the most expensive thing first",
            Policy::Attrition => "spends nothing until bloodied",
            Policy::FocusFire => "finishes the most wounded enemy first",
            Policy::Scattered => "spreads attacks across enemies",
            Policy::Defensive => "defends once bloodied",
            Policy::Solver => "searches one turn ahead by rollout",
        }
    }
}
