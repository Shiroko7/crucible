//! Hunter's Mark (SRD 5.2, 1st level).

use crate::creature::{Effect, Move, MoveKind, Rider, Uses};
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{Condition, DamageKind, Duration};

/// Hunter's Mark (SRD 5.2, 1st level, Bonus Action, Concentration up to 1
/// hour): mark one creature as your quarry, and every attack of yours that
/// hits it deals an extra `1d6` Force damage until your concentration
/// breaks.
///
/// Two halves that meet in the fight: the move marks the target
/// ([`Effect::Afflict`] with [`Condition::Quarry`] - no attack roll, no save,
/// nothing to resist), and the rider ([`Rider::BonusDamageVsQuarry`]) pays
/// out on every hit against the creature this caster's own concentration is
/// holding the mark on. Neither is worth anything without the other, which is
/// why one plugin registers both.
///
/// A ranger's Favored Enemy is the same spell cast without a slot a few times
/// a day, so `free_uses` registers a second, slot-free copy of the move
/// ahead of the slotted one: identical in every other way, so a policy
/// spending nothing still marks its quarry, and one willing to spend a slot
/// reaches for the free casts first purely from move order. `0` leaves it
/// out entirely, for a caster that only ever pays a slot.
///
/// Not modelled: moving the mark to a new creature when the first one drops.
/// That is a Bonus Action with no cost, which this engine has no way to make
/// conditional on the quarry being dead, and against a single enemy there is
/// nowhere to move it to anyway. Against several, the mark simply stays where
/// it was put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HuntersMarkPlugin {
    /// Extra damage dice on a hit against the quarry - `1` for the spell as
    /// printed, more for a build that scales it.
    pub dice_count: u32,
    /// Casts that spend no spell slot: a ranger's Favored Enemy.
    pub free_uses: u32,
    /// The slot level a paid cast spends.
    pub slot: u32,
}

impl Default for HuntersMarkPlugin {
    fn default() -> Self {
        Self {
            dice_count: 1,
            free_uses: 0,
            slot: 1,
        }
    }
}

impl HuntersMarkPlugin {
    /// The mark itself: an hour of it, which is longer than any fight, so
    /// what ends it in practice is the concentration.
    fn mark(name: impl Into<String>) -> Move {
        Move::new(
            name,
            Effect::Afflict {
                condition: Condition::Quarry,
                duration: Duration::Rounds(600),
            },
        )
        .with_concentration()
        .with_kind(MoveKind::Spell)
    }
}

impl FeaturePlugin for HuntersMarkPlugin {
    fn id(&self) -> &'static str {
        "hunters_mark"
    }

    fn name(&self) -> &str {
        "Hunter's Mark"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        if self.dice_count == 0 {
            return Err(FeatureError::InvalidConfiguration(
                "hunters_mark needs at least one damage die".to_string(),
            ));
        }
        builder.add_rider(Rider::BonusDamageVsQuarry {
            dice_count: self.dice_count,
            dice_sides: 6,
            bonus: 0,
            damage_kind: DamageKind::Force,
        });
        if self.free_uses > 0 {
            builder.add_bonus_action(
                Self::mark("Hunter's Mark (no slot)").with_uses(Uses::Limited(self.free_uses)),
            );
        }
        builder.add_bonus_action(Self::mark("Hunter's Mark").with_spell_slot(self.slot));
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Hunter's Mark: `dice_count` (default 1), `free_uses` for casts that
    // spend no slot (a ranger's Favored Enemy), and the `slot` a paid cast
    // spends (default 1st).
    registry.register("hunters_mark", |val| {
        let read = |key: &str, default: u32| {
            val.get(key)
                .and_then(|v| v.as_integer())
                .map_or(default, |n| n.max(0) as u32)
        };
        let slot = read("slot", 1);
        if !(1..=crate::rules::SPELL_LEVELS).contains(&slot) {
            return Err(FeatureError::InvalidConfiguration(format!(
                "a spell slot is level 1 to 9, got `slot = {slot}`"
            )));
        }
        Ok(Box::new(HuntersMarkPlugin {
            dice_count: read("dice_count", 1),
            free_uses: read("free_uses", 0),
            slot,
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Creature;
    use crate::prob::Rng;
    use crate::sim::{run_teams, Budget, Policy};

    fn hunter(free_uses: u32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Hunter", 17, 60);
        builder.set_spell_slot_max(1, 4);
        HuntersMarkPlugin {
            free_uses,
            ..Default::default()
        }
        .apply(&mut builder)
        .expect("hunter's mark applies");
        builder
    }

    /// The spell is one move and one rider: the free casts come first, so a
    /// caster reaches for them before a slot, and both mark the same way.
    #[test]
    fn it_registers_the_free_casts_the_slotted_cast_and_the_rider() {
        let c = hunter(2).creature;
        assert_eq!(c.bonus_actions.len(), 2);

        let free = &c.bonus_actions[0];
        assert_eq!(free.uses, Uses::Limited(2));
        assert_eq!(free.spell_slot_level, None);
        let paid = &c.bonus_actions[1];
        assert_eq!(paid.uses, Uses::Unlimited);
        assert_eq!(paid.spell_slot_level, Some(1));

        for m in [free, paid] {
            assert!(m.concentration, "the mark lasts as long as attention does");
            assert_eq!(m.kind, MoveKind::Spell);
            assert_eq!(
                m.effect,
                Effect::Afflict {
                    condition: Condition::Quarry,
                    duration: Duration::Rounds(600),
                }
            );
        }
        assert_eq!(
            c.riders,
            vec![Rider::BonusDamageVsQuarry {
                dice_count: 1,
                dice_sides: 6,
                bonus: 0,
                damage_kind: DamageKind::Force,
            }]
        );

        // Without free casts there is only the slotted move.
        assert_eq!(hunter(0).creature.bonus_actions.len(), 1);
    }

    /// The whole point, in a live fight: a hunter that marks its quarry deals
    /// clearly more damage over the same fights than the identical hunter
    /// without the spell, and the narration says where the extra came from.
    #[test]
    fn marking_a_quarry_adds_real_damage_in_a_fight() {
        let mut marked = hunter(3).creature;
        marked.actions.push(
            crate::dsl::grammar::parse_move_external(
                "Two Blades | strikes 2 | hit +9 | 1d8+5 slashing",
                &Creature::new("x", 10, 10),
            )
            .expect("the attack parses"),
        );
        let mut plain = marked.clone();
        plain.bonus_actions.clear();
        plain.riders.clear();

        let mut dummy = Creature::new("Dummy", 15, 400);
        dummy.team = 1;
        dummy.actions.push(
            crate::dsl::grammar::parse_move_external(
                "Swipe | hit +6 | 1d6 slashing",
                &Creature::new("x", 10, 10),
            )
            .expect("the attack parses"),
        );

        let dealt = |who: &Creature| {
            let mut rng = Rng::new(21);
            let mut total = 0i64;
            for _ in 0..200 {
                let o = run_teams(
                    &mut rng,
                    &[who, &dummy],
                    [Policy::Greedy; 2],
                    6,
                    Budget::default(),
                    &mut None,
                );
                total += o.damage_dealt[0];
            }
            total
        };
        let (with, without) = (dealt(&marked), dealt(&plain));
        assert!(
            with > without,
            "the mark should be worth real damage: {with} with, {without} without"
        );

        let mut log = Some(Vec::new());
        let mut rng = Rng::new(3);
        run_teams(
            &mut rng,
            &[&marked, &dummy],
            [Policy::Greedy; 2],
            4,
            Budget::default(),
            &mut log,
        );
        let narration = log.unwrap().join("\n");
        assert!(
            narration.contains("quarry"),
            "the mark should show up in the narration:\n{narration}"
        );
    }
}
