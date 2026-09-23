//! Spirit Guardians (SRD 5.2, 3rd level).

use crate::creature::{Effect, Move, MoveKind, SaveEffect};
use crate::features::spells::{save_dc, slot_level};
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{Ability, Condition, DamageKind, DamageRoll, Duration};

/// Spirit Guardians (SRD 5.2, 3rd level, concentration, up to 10 minutes):
/// spirits fill a 15-foot emanation around the caster. Every enemy that
/// starts its turn there makes a Wisdom saving throw, taking `dice`d8
/// Radiant or Necrotic damage - half as much on a success - and its speed is
/// halved while it stays.
///
/// This is the aura machinery with a cast in front of it, which is what
/// [`crate::features::LastingAuraPlugin`] already is; the only thing this
/// adds is reading the save DC off the caster's own profile rather than
/// having it written into the aura by hand. Everything else - that every
/// enemy meets it at the start of its own turn, that it drops when
/// concentration does, that raising one already up is no move at all - is
/// the same aura every monster with one uses.
///
/// Two readings stated rather than hidden:
///
/// - **The slow lands on a failed save.** 5e halves the speed of anything in
///   the area whether it saves or not; an aura here does what it does on a
///   failure. Around a creature with a mouth that costs a failed save its
///   movement, and anywhere else there is no movement to lose, so the
///   difference is small and in the honest direction.
/// - **It catches everyone.** With no positioning, every enemy is treated as
///   standing in the emanation. That is the same rule every area effect here
///   follows, and it is generous when the caster is on the party's side - a
///   cleric who really wants to know what it is worth against a spread-out
///   enemy should read the aura's own damage rather than the fight's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpiritGuardiansPlugin {
    pub dice: u32,
    pub damage_kind: DamageKind,
    pub slot: u32,
    pub rounds: u32,
}

impl Default for SpiritGuardiansPlugin {
    fn default() -> Self {
        Self {
            dice: 3,
            damage_kind: DamageKind::Radiant,
            slot: 3,
            rounds: 100,
        }
    }
}

impl FeaturePlugin for SpiritGuardiansPlugin {
    fn id(&self) -> &'static str {
        "spirit_guardians"
    }

    fn name(&self) -> &str {
        "Spirit Guardians"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = save_dc(builder, "spirit_guardians")?;
        let spirits = Move::new(
            "Spirit Guardians",
            Effect::Save(SaveEffect {
                ability: Ability::Wis,
                dc,
                damage: vec![DamageRoll::new(self.dice.max(1), 8, 0, self.damage_kind)],
                half_on_success: true,
                on_failure: vec![(Condition::Slowed, Duration::ApplierTurn)],
                max_targets: None,
                requires_type: None,
            }),
        );
        let which = builder.creature.add_lasting_aura(spirits);
        // The condition holding it names it by a `u8`, so a creature that
        // has declared that many already has nowhere to put this one.
        if u8::try_from(which).is_err() {
            return Err(FeatureError::InvalidConfiguration(
                "a creature can hold at most 256 declared auras".to_string(),
            ));
        }

        builder.add_action(
            Move::new(
                "Spirit Guardians",
                Effect::Aura {
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
    // Spirit Guardians: `dice` d8s (3 at its base level, one more per level
    // it is upcast) of `damage_kind` (radiant for a cleric of a kindly god,
    // necrotic otherwise), the `slot` it spends and how many `rounds` it is
    // held up for.
    registry.register("spirit_guardians", |val| {
        let damage_kind = match val.get("damage_kind").and_then(|v| v.as_str()) {
            None => DamageKind::Radiant,
            Some(word) => DamageKind::parse(word)
                .ok_or_else(|| FeatureError::UnknownDamageKind(word.to_string()))?,
        };
        Ok(Box::new(SpiritGuardiansPlugin {
            dice: val.get("dice").and_then(|v| v.as_integer()).unwrap_or(3) as u32,
            damage_kind,
            slot: slot_level(val, 3)?,
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
    use crate::rules::SpellCastingProfile;

    fn build(params: &str) -> FeatureResult<CreatureBuilder> {
        let mut builder = CreatureBuilder::new("cleric", 18, 60);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Wis, 4, 3));
        builder.set_spell_slot_max(3, 3);
        let value: toml::Value = toml::from_str(params).expect("valid TOML");
        FeatureRegistry::new()
            .build_plugin("spirit_guardians", &value)?
            .apply(&mut builder)?;
        Ok(builder)
    }

    /// A concentration cast that raises an aura, and the aura itself - the
    /// save every enemy meets at the start of its turn.
    #[test]
    fn it_raises_an_aura_every_enemy_answers_on_its_own_turn() {
        let builder = build("plugin = \"spirit_guardians\"").expect("applies");
        let cast = &builder.creature.actions[0];
        assert_eq!(cast.spell_slot_level, Some(3));
        assert!(cast.concentration);
        assert_eq!(cast.kind, MoveKind::Spell);
        assert_eq!(
            cast.effect,
            Effect::Aura {
                which: 0,
                duration: Duration::Rounds(100)
            }
        );

        let Effect::Save(save) = &builder.creature.lasting_auras[0].effect else {
            panic!("expected the aura to be a saving throw")
        };
        assert_eq!(save.ability, Ability::Wis);
        assert_eq!(save.dc, 15);
        assert!(save.half_on_success);
        assert_eq!(
            save.damage,
            vec![DamageRoll::new(3, 8, 0, DamageKind::Radiant)]
        );
        assert_eq!(
            save.on_failure,
            vec![(Condition::Slowed, Duration::ApplierTurn)]
        );
    }

    /// Necrotic, upcast, and a shorter leash - all declared.
    #[test]
    fn its_dice_type_slot_and_duration_are_all_declared() {
        let builder = build(
            r#"
            plugin = "spirit_guardians"
            dice = 5
            damage_kind = "necrotic"
            slot = 5
            rounds = 10
            "#,
        )
        .expect("applies");
        assert_eq!(
            builder.creature.actions[0].effect,
            Effect::Aura {
                which: 0,
                duration: Duration::Rounds(10)
            }
        );
        assert_eq!(builder.creature.actions[0].spell_slot_level, Some(5));
        let Effect::Save(save) = &builder.creature.lasting_auras[0].effect else {
            unreachable!()
        };
        assert_eq!(
            save.damage,
            vec![DamageRoll::new(5, 8, 0, DamageKind::Necrotic)]
        );
    }

    /// Live: raised in a real fight, the spirits cost an enemy hit points on
    /// every turn it starts standing in them.
    #[test]
    fn the_raised_aura_hurts_whoever_starts_its_turn_in_it() {
        let raider = &crate::dsl::scenario::parse(
            "creature: raider\nac: 20\nhp: 400\ninitiative: -10\n\
             action: Axe | strikes 1 | hit +5 | 1d12+4 slashing\n",
        )
        .expect("parses")[0];
        let taken = |cleric: &crate::creature::Creature| {
            let mut rng = Rng::new(3);
            let mut total = 0i64;
            for _ in 0..100 {
                let mut log = None;
                let outcome = crate::sim::run(
                    &mut rng,
                    [cleric, raider],
                    [crate::sim::Policy::Greedy; 2],
                    6,
                    &mut log,
                );
                total += outcome.damage_dealt[0];
            }
            total
        };
        let mut caster = build("plugin = \"spirit_guardians\"").expect("applies");
        caster.creature.initiative = 10;
        let mut plain = CreatureBuilder::new("cleric", 18, 200);
        plain.creature.initiative = 10;
        let (with, without) = (taken(&caster.creature), taken(&plain.creature));
        assert!(
            with > without,
            "the spirits should be doing the damage: {with} vs {without}"
        );
    }
}
