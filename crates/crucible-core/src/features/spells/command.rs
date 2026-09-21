//! Command (SRD 5.2, 1st level).

use crate::creature::{Effect, Move, MoveKind, SaveEffect};
use crate::features::spells::{charge, parse_spell_cost, save_dc, SpellCost};
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{Ability, Condition, Duration};

/// A one-word command Command can cast. The SRD lists five (Approach, Drop,
/// Flee, Grovel, Halt); only the two this codebase's roadmap asks for are
/// implemented, but the shape leaves room for the rest without touching
/// anything above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandWord {
    Grovel,
    Halt,
}

impl CommandWord {
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "grovel" => Self::Grovel,
            "halt" => Self::Halt,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grovel => "Grovel",
            Self::Halt => "Halt",
        }
    }

    /// What a failed save applies. Every word shares `Condition::Compelled`
    /// with `Duration::ApplierTurn` - the engine's handle on "obeys a
    /// directive on its own very next turn" (see that variant's own doc) and
    /// denies the rest of that turn's action economy via
    /// `sim::duel::Fighter::loses_turn`, without touching legendary actions.
    /// `ApplierTurn` rather than `VictimTurn` is deliberate: it is the
    /// mechanism Stunning Strike already proves bounds a condition to
    /// *exactly* the victim's next turn regardless of relative initiative
    /// (see `Duration::ApplierTurn`'s own doc) - `VictimTurn` clears at the
    /// start of the victim's own turn, before that turn's incapacitation
    /// check runs, so it cannot gate the very turn Command means to deny.
    ///
    /// Grovel's real text is "the target falls prone and then ends its
    /// turn", so it stacks `Condition::Prone` on top of the same duration.
    /// There is no engine mechanic for standing back up (no movement model;
    /// see `README.md`, "Positioning is the gap that matters"), so tying
    /// Prone to Compelled's clock is a documented simplification, not a claim
    /// that a real target could not still be prone once its next turn
    /// passes. Halt's text - "the target doesn't move and takes no actions" -
    /// has nothing left over once "takes no actions" is modelled: there is no
    /// movement to additionally restrict, so `Compelled` alone is the whole
    /// of it.
    fn on_failure(self) -> Vec<(Condition, Duration)> {
        let compelled = (Condition::Compelled, Duration::ApplierTurn);
        match self {
            Self::Grovel => vec![(Condition::Prone, Duration::ApplierTurn), compelled],
            Self::Halt => vec![compelled],
        }
    }
}

/// Command (SRD 5.2, 1st-level enchantment, Action): a Wisdom save or the
/// target obeys a one-word command on its next turn.
#[derive(Debug, Clone)]
pub struct CommandPlugin {
    pub word: CommandWord,
    pub cost: Option<SpellCost>,
}

impl CommandPlugin {
    pub fn new(word: CommandWord, cost: Option<SpellCost>) -> Self {
        Self { word, cost }
    }

    fn label(&self) -> String {
        format!("Command (\"{}\")", self.word.as_str())
    }
}

impl FeaturePlugin for CommandPlugin {
    fn id(&self) -> &'static str {
        "command"
    }

    fn name(&self) -> &str {
        self.word.as_str()
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let dc = save_dc(builder, "Command")?;

        let mv = Move::new(
            self.label(),
            Effect::Save(SaveEffect {
                ability: Ability::Wis,
                dc,
                damage: Vec::new(),
                half_on_success: false,
                on_failure: self.word.on_failure(),
                max_targets: Some(1),
                requires_type: None,
            }),
        )
        .with_kind(MoveKind::Spell);
        let mv = charge(builder, &self.cost, mv)?;
        builder.add_action(mv);
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // Command (SRD 5.2, 1st level)
    registry.register("command", |val| {
        let word_str = val.get("word").and_then(|v| v.as_str()).unwrap_or("grovel");
        let word = CommandWord::parse(word_str).ok_or_else(|| {
            FeatureError::InvalidConfiguration(format!("unknown command word '{word_str}'"))
        })?;
        let cost = parse_spell_cost(val)?;
        Ok(Box::new(CommandPlugin::new(word, cost)))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Creature;
    use crate::creature::Uses;
    use crate::prob::Rng;
    use crate::rules::{DamageKind, Reduction, SpellCastingProfile};

    const SAMPLES: usize = 200_000;

    fn tolerance(p: f64, n: usize) -> f64 {
        5.0 * (p * (1.0 - p) / n as f64).sqrt() + 1e-4
    }

    fn caster(ability: Ability, modifier: i32, proficiency: i32) -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Caster", 12, 20);
        builder.set_spellcasting(SpellCastingProfile::new(ability, modifier, proficiency));
        builder
    }

    fn target(ac: i32, save: i32, reductions: &[(DamageKind, Reduction)]) -> Creature {
        let mut c = Creature::new("target", ac, 1_000);
        c.saves = [save; 6];
        c.reductions = reductions.to_vec();
        c
    }

    fn effect_of(m: &Move) -> &Effect {
        &m.effect
    }

    #[test]
    fn command_needs_a_spellcasting_profile() {
        let mut builder = CreatureBuilder::new("Caster", 12, 20);
        let err = CommandPlugin::new(CommandWord::Grovel, None)
            .apply(&mut builder)
            .expect_err("no spellcasting profile means no save DC to compute");
        assert!(matches!(err, FeatureError::InvalidConfiguration(_)));
    }

    #[test]
    fn grovel_forces_prone_and_denies_the_rest_of_the_turn() {
        let mut builder = caster(Ability::Wis, 3, 2); // DC 8 + 3 + 2 = 13
        CommandPlugin::new(CommandWord::Grovel, None)
            .apply(&mut builder)
            .unwrap();
        let Effect::Save(save) = effect_of(&builder.creature.actions[0]) else {
            panic!("expected a Save effect");
        };
        assert_eq!(save.ability, Ability::Wis);
        assert_eq!(save.dc, 13);
        assert_eq!(
            save.on_failure,
            vec![
                (Condition::Prone, Duration::ApplierTurn),
                (Condition::Compelled, Duration::ApplierTurn),
            ]
        );
    }

    #[test]
    fn halt_only_compels_no_prone() {
        let mut builder = caster(Ability::Wis, 3, 2);
        CommandPlugin::new(CommandWord::Halt, None)
            .apply(&mut builder)
            .unwrap();
        let Effect::Save(save) = effect_of(&builder.creature.actions[0]) else {
            panic!("expected a Save effect");
        };
        assert_eq!(
            save.on_failure,
            vec![(Condition::Compelled, Duration::ApplierTurn)]
        );
    }

    #[test]
    fn command_word_names_round_trip() {
        assert_eq!(CommandWord::parse("grovel"), Some(CommandWord::Grovel));
        assert_eq!(CommandWord::parse("HALT"), Some(CommandWord::Halt));
        assert_eq!(CommandWord::parse("flee"), None);
    }

    /// The same closed-form-vs-sampled agreement as Blindness/Deafness,
    /// exercised against Command's Wisdom save instead of a Constitution one.
    #[test]
    fn command_failure_chance_agrees_with_sampled_saves() {
        let mut builder = caster(Ability::Cha, 2, 3); // DC 8 + 2 + 3 = 13
        CommandPlugin::new(CommandWord::Grovel, None)
            .apply(&mut builder)
            .unwrap();
        let Effect::Save(save) = effect_of(&builder.creature.actions[0]) else {
            panic!("expected a Save effect");
        };

        let victim = target(14, -1, &[]);
        let exact = save.failure_chance(&victim);

        let mut rng = Rng::new(77);
        let mut fails = 0u32;
        for _ in 0..SAMPLES {
            if !save.roll_save(&mut rng, &victim, false) {
                fails += 1;
            }
        }
        let sampled = f64::from(fails) / SAMPLES as f64;
        let tol = tolerance(exact, SAMPLES);
        assert!(
            (sampled - exact).abs() < tol,
            "sampled fail rate {sampled:.5} vs exact {exact:.5}, tolerance {tol:.5}"
        );
    }

    /// An end-to-end fight, not just the `Move` shape: Grovel really does
    /// cost the target its very next turn (via `turns_lost`) and really does
    /// leave it Prone, using the duel engine exactly as any other condition
    /// does - the acceptance test for `sim::duel::Fighter::loses_turn`.
    #[test]
    fn grovel_costs_the_target_its_next_turn_in_a_real_fight() {
        use crate::sim::duel::{run, Policy};

        let mut builder = caster(Ability::Wis, 5, 4); // DC 8 + 5 + 4 = 17
        builder.creature.initiative = 100;
        CommandPlugin::new(CommandWord::Grovel, None)
            .apply(&mut builder)
            .unwrap();
        let mut caster = builder.build().unwrap();
        // One cast only: a caster who keeps recasting Command every round
        // would keep the target compelled continuously, which is correct
        // behaviour but would defeat what this test checks - that a single
        // application clears on schedule rather than lingering.
        caster.actions[0].uses = Uses::Limited(1);

        let mut victim = Creature::new("victim", 10, 100);
        victim.saves[Ability::Wis.index()] = -20; // never saves
        victim.initiative = -100;

        let mut rng = Rng::new(31);
        let mut log = Some(Vec::new());
        let o = run(
            &mut rng,
            [&caster, &victim],
            [Policy::Greedy; 2],
            3,
            &mut log,
        );
        let narration = log.unwrap().join("\n");

        assert!(
            narration.contains("prone") && narration.contains("compelled"),
            "Grovel should land both conditions:\n{narration}"
        );
        assert!(
            o.turns_lost[1] >= 1,
            "the target must lose at least the one turn Grovel denies it"
        );
        // The fight ran three rounds and the victim cannot hurt back (no
        // actions of its own), so it must not have lost *every* turn -
        // Compelled expires after the one turn it was meant for.
        assert!(
            o.turns_lost[1] < o.rounds,
            "Compelled must not persist past the one turn it denies: lost {} of {} rounds",
            o.turns_lost[1],
            o.rounds
        );
    }

    fn spellcaster() -> CreatureBuilder {
        let mut builder = CreatureBuilder::new("Wizard", 12, 30);
        builder.set_spellcasting(SpellCastingProfile::new(Ability::Int, 4, 3));
        builder
    }

    #[test]
    fn command_builds_from_toml_and_rejects_an_unknown_word() {
        let registry = FeatureRegistry::new();
        let mut builder = spellcaster();

        let params: toml::Value = toml::from_str("plugin = \"command\"\nword = \"halt\"").unwrap();
        let plugin = registry
            .build_plugin("command", &params)
            .expect("command builds from toml");
        plugin.apply(&mut builder).unwrap();
        let Effect::Save(save) = &builder.creature.actions[0].effect else {
            panic!("expected a Save effect");
        };
        assert_eq!(
            save.on_failure,
            vec![(Condition::Compelled, Duration::ApplierTurn)]
        );

        let bad_params: toml::Value =
            toml::from_str("plugin = \"command\"\nword = \"flee\"").unwrap();
        assert!(matches!(
            registry.build_plugin("command", &bad_params),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }
}
