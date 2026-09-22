//! Saving throws during a fight: the roll mode conditions put on them, and
//! Legendary Resistance buying back a failure.

use crate::creature::Rider;
use crate::prob::Rng;
use crate::rules::{sample_save_modifier_bonus, Ability, RollMode};
use crate::sim::fight::Fighter;

/// The same stacking rule [`crate::sim::fight::attack::attack_mode`] applies
/// to attack rolls, applied here to a creature's own `ability` saving throw:
/// any source of disadvantage (Suppressed, an injury poison's burden) and any
/// source of advantage (a save against a condition it is immune to,
/// downgraded) cancel rather than override one another.
pub(super) fn save_mode(f: &Fighter<'_>, ability: Ability, advantage: bool) -> RollMode {
    let disadvantage = f.has(|c| c.disadvantage_on_save(ability));
    match (advantage, disadvantage) {
        (true, false) => RollMode::Advantage,
        (false, true) => RollMode::Disadvantage,
        _ => RollMode::Normal,
    }
}

/// Roll a saving throw, letting conditions force a failure or change the
/// roll's mode, and [`Rider::AlwaysSucceed`] buy one back. `advantage` is a
/// source of advantage the caller knows about and the fighter's conditions
/// do not - see [`crate::sim::fight::Fight::conditions_against`].
pub(super) fn saving_throw(
    fighters: &mut [Fighter<'_>],
    rng: &mut Rng,
    who: usize,
    ability: crate::rules::Ability,
    dc: i32,
    advantage: bool,
) -> (bool, bool) {
    let f = &fighters[who];
    let auto_fail = f.has(|c| c.auto_fails(ability));
    // `save_modifiers` covers every saving throw `who` makes, this one
    // included - so a blessed creature's own concentration check picks up
    // its `+1d4` the same way any other save does, with no special case
    // needed here for that being "the same creature".
    let mode = save_mode(f, ability, advantage);
    let bonus = sample_save_modifier_bonus(rng, &f.save_modifiers);
    let rolled = !auto_fail && mode.roll(rng) + f.creature.save(ability) + bonus >= dc;
    if rolled {
        return (true, false);
    }

    // Legendary Resistance, and anything else shaped like it: a ring that
    // rescues one kind of save at the price of a reaction is the same rider
    // with `ability` and `reaction` filled in. A hoarding policy never
    // reaches for any of them, which is most of what separates a well-run
    // monster from a badly run one.
    if fighters[who].will_spend() {
        let creature = fighters[who].creature;
        let f = &fighters[who];
        let mut slot = None;
        for (i, rider) in creature.riders.iter().enumerate() {
            let usable = match rider {
                Rider::AlwaysSucceed {
                    ability: only,
                    reaction,
                    ..
                } => {
                    only.is_none_or(|a| a == ability)
                        && (!reaction || (f.reaction && !f.incapacitated()))
                }
                _ => false,
            };
            if usable && f.rider_uses[i] > 0 {
                slot = Some(i);
                break;
            }
        }
        if let Some(i) = slot {
            fighters[who].rider_uses[i] -= 1;
            if creature.riders[i].is_reaction() {
                fighters[who].spend_reaction_budget();
            }
            return (true, true);
        }
    }
    (false, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Creature;
    use crate::prob::Rng;
    use crate::rules::Condition;
    use crate::sim::fight::fighter::refresh;
    use crate::sim::fight::Expiry;
    use crate::sim::{Policy, Side};

    #[test]
    fn suppressed_grants_disadvantage_on_saves_until_it_expires() {
        let creature = Creature::new("x", 10, 10);
        let mut fighter = Fighter::new(&creature, Side::A, Policy::Greedy, 0);
        assert_eq!(save_mode(&fighter, Ability::Dex, false), RollMode::Normal);

        fighter
            .conditions
            .push((Condition::Suppressed, Expiry::TurnStart(0)));
        assert_eq!(
            save_mode(&fighter, Ability::Dex, false),
            RollMode::Disadvantage
        );

        fighter.conditions.clear();
        assert_eq!(
            save_mode(&fighter, Ability::Dex, false),
            RollMode::Normal,
            "the disadvantage must not outlive the condition"
        );
    }

    /// A ring that rescues one kind of save: it answers a Dexterity save and
    /// no other, it costs the reaction, and it runs out - the same rider
    /// Legendary Resistance is, with two of its fields filled in.
    #[test]
    fn a_narrowed_auto_success_answers_one_save_and_costs_a_reaction() {
        let mut creature = Creature::new("wearer", 15, 40);
        creature.saves = [-100; 6]; // nothing is ever made on the roll
        creature.riders.push(Rider::AlwaysSucceed {
            uses: 2,
            ability: Some(Ability::Dex),
            reaction: true,
        });
        let mut fighters = vec![Fighter::new(&creature, Side::A, Policy::Greedy, 0)];
        let mut rng = Rng::new(7);

        // A Wisdom save is not what it answers.
        assert_eq!(
            saving_throw(&mut fighters, &mut rng, 0, Ability::Wis, 20, false),
            (false, false)
        );
        assert!(fighters[0].reaction, "and nothing was spent on it");

        // A Dexterity save is, and it costs the reaction.
        assert_eq!(
            saving_throw(&mut fighters, &mut rng, 0, Ability::Dex, 20, false),
            (true, true)
        );
        assert!(!fighters[0].reaction);
        assert_eq!(fighters[0].rider_uses[0], 1, "one charge gone");

        // With the reaction already spent it cannot fire again this round,
        // however many charges are left.
        assert_eq!(
            saving_throw(&mut fighters, &mut rng, 0, Ability::Dex, 20, false),
            (false, false)
        );
        assert_eq!(fighters[0].rider_uses[0], 1, "and no charge was wasted");

        // The reaction back, the last charge goes, and then it is empty.
        refresh(&mut fighters[0], &mut rng);
        assert!(saving_throw(&mut fighters, &mut rng, 0, Ability::Dex, 20, false).0);
        refresh(&mut fighters[0], &mut rng);
        assert_eq!(
            saving_throw(&mut fighters, &mut rng, 0, Ability::Dex, 20, false),
            (false, false)
        );
    }

    /// Wired into the actual roll, not just the mode computation: a
    /// suppressed creature really does fail more saves than an unsuppressed
    /// one against the same DC.
    #[test]
    fn saving_throw_rolls_worse_while_suppressed() {
        let creature = Creature::new("x", 10, 10);
        let mut plain = vec![Fighter::new(&creature, Side::A, Policy::Greedy, 0)];
        let mut suppressed = vec![Fighter::new(&creature, Side::A, Policy::Greedy, 0)];
        suppressed[0]
            .conditions
            .push((Condition::Suppressed, Expiry::TurnStart(0)));

        let mut rng = Rng::new(11);
        let trials = 20_000;
        let (mut fails_plain, mut fails_suppressed) = (0u32, 0u32);
        for _ in 0..trials {
            if !saving_throw(&mut plain, &mut rng, 0, Ability::Dex, 11, false).0 {
                fails_plain += 1;
            }
            if !saving_throw(&mut suppressed, &mut rng, 0, Ability::Dex, 11, false).0 {
                fails_suppressed += 1;
            }
        }
        assert!(
            fails_suppressed > fails_plain + trials / 20,
            "disadvantage should fail noticeably more often: {fails_suppressed} vs {fails_plain} of {trials}"
        );
    }
}
