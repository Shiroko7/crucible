//! Shield of Faith (SRD 5.2, 1st level).

use crate::creature::{Boon, Effect, Move, MoveKind};
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::Duration;

/// Shield of Faith (SRD 5.2, 1st level, Bonus Action, concentration, up to 10
/// minutes): a shimmering field gives its target +2 Armor Class for the
/// duration.
///
/// A [`Boon`] with nothing in it but Armor Class, which is the whole point of
/// boons carrying one: it is up exactly as long as the ward is, it goes when
/// concentration does, and it is neither a permanent bonus nor a reaction
/// spent against one attack.
///
/// Cast on the caster itself, which is the one thing here that is a
/// simplification rather than the spell: a boon rides its holder, and 5e
/// lets this one be cast on a creature within 60 feet. A cleric warding the
/// front line instead of itself is not expressible until boons can be handed
/// out, and the version that is expressible is the common one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShieldOfFaithPlugin {
    pub ac: i32,
    pub slot: u32,
    /// How long it is held up, in rounds. Ten minutes is a hundred rounds -
    /// longer than any fight here - so the default is simply "the whole
    /// fight", ended by concentration breaking rather than by the clock.
    pub rounds: u32,
}

impl Default for ShieldOfFaithPlugin {
    fn default() -> Self {
        Self {
            ac: 2,
            slot: 1,
            rounds: 100,
        }
    }
}

impl FeaturePlugin for ShieldOfFaithPlugin {
    fn id(&self) -> &'static str {
        "shield_of_faith"
    }

    fn name(&self) -> &str {
        "Shield of Faith"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let which = builder
            .creature
            .add_boon(Boon::new("Shield of Faith").with_ac(self.ac));
        // The condition holding it names it by a `u8`, so a creature that
        // has declared that many already has nowhere to put this one.
        if u8::try_from(which).is_err() {
            return Err(FeatureError::InvalidConfiguration(
                "a creature can hold at most 256 declared boons".to_string(),
            ));
        }
        builder.add_bonus_action(
            Move::new(
                "Shield of Faith",
                Effect::Boon {
                    which,
                    duration: Duration::Rounds(self.rounds.max(1)),
                },
            )
            .with_spell_slot(self.slot)
            .with_kind(MoveKind::Spell)
            .with_concentration(),
        );
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Shield of Faith: the `ac` it grants, the `slot` it spends and how many
    // `rounds` it is held up for.
    registry.register("shield_of_faith", |val| {
        Ok(Box::new(ShieldOfFaithPlugin {
            ac: val.get("ac").and_then(|v| v.as_integer()).unwrap_or(2) as i32,
            slot: crate::features::spells::slot_level(val, 1)?,
            rounds: val
                .get("rounds")
                .and_then(|v| v.as_integer())
                .unwrap_or(100) as u32,
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::FeatureRegistry;
    use crate::prob::Rng;
    use crate::rules::{Ability, SpellCastingProfile};

    fn build(params: &str) -> FeatureResult<CreatureBuilder> {
        let mut builder = CreatureBuilder::new("cleric", 16, 60);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 4, 3));
        builder.set_spell_slot_max(1, 4);
        let value: toml::Value = toml::from_str(params).expect("valid TOML");
        FeatureRegistry::new()
            .build_plugin("shield_of_faith", &value)?
            .apply(&mut builder)?;
        Ok(builder)
    }

    /// A concentration bonus action that puts up an Armor Class boon and
    /// nothing else.
    #[test]
    fn it_registers_a_concentration_bonus_action_holding_an_ac_boon() {
        let builder = build("plugin = \"shield_of_faith\"").expect("applies");
        let m = &builder.creature.bonus_actions[0];
        assert_eq!(m.spell_slot_level, Some(1));
        assert_eq!(m.kind, MoveKind::Spell);
        assert!(m.concentration, "a ward has to be held up");
        assert_eq!(
            m.effect,
            Effect::Boon {
                which: 0,
                duration: Duration::Rounds(100)
            }
        );
        let boon = &builder.creature.boons[0];
        assert_eq!(boon.ac, 2);
        assert!(boon.damage.is_none(), "a ward adds nothing to a blow");
        assert!(boon.resist.is_empty());
    }

    /// Live: the ward really is harder to hit while it is up.
    #[test]
    fn a_warded_cleric_takes_less_than_an_unwarded_one() {
        let raider = &crate::dsl::scenario::parse(
            "creature: raider\nac: 10\nhp: 400\ninitiative: -10\n\
             action: Axe | strikes 2 | hit +5 | 1d12+4 slashing\n",
        )
        .expect("parses")[0];
        let taken = |defender: &crate::creature::Creature| {
            let mut rng = Rng::new(11);
            let mut total = 0i64;
            for _ in 0..200 {
                let mut log = None;
                let outcome = crate::sim::run(
                    &mut rng,
                    [defender, raider],
                    [crate::sim::Policy::Greedy; 2],
                    6,
                    &mut log,
                );
                total += outcome.damage_dealt[1];
            }
            total
        };
        let mut warded = build("plugin = \"shield_of_faith\"").expect("applies");
        warded.creature.initiative = 10;
        let mut plain = CreatureBuilder::new("cleric", 16, 60);
        plain.creature.initiative = 10;
        let (with, without) = (taken(&warded.creature), taken(&plain.creature));
        assert!(
            with < without,
            "a raised ward should turn some hits into misses: {with} vs {without}"
        );
    }
}
