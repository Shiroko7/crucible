//! A double its summoner calls up and then spends its own moves commanding.

use crate::creature::{Cost, Creature, Effect, Move, MoveKind, Requirement, Spend, Strike};
use crate::dsl::grammar;
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{DamageKind, Reduction};

/// A double a creature calls up beside it - a clone of running water, a
/// spectral ally, a mirror image with teeth - which stands there with its own
/// hit points until it is destroyed, and acts only when its summoner spends a
/// move telling it to.
///
/// That last part is what makes it this mechanism rather than an extra
/// combatant: it never takes a turn, so calling one up buys a body and a
/// *different* use for the summoner's bonus action, not a second action
/// economy. Everything it can be told to do is a move on the summoner
/// ([`Requirement::Summon`]), legal only while enough doubles are standing.
///
/// What it registers, all declared rather than hardcoded:
///
/// - **Calling one up** - a Bonus Action, paying whatever pool it costs, and
///   legal only while there is room for another ([`Requirement::SummonRoom`]).
/// - **Commanding them** - each declared attack profile, once per number of
///   doubles that could be striking together when `scale_with_count` is set:
///   one die per additional double, which is what "increasing the damage by
///   one damage dice when they attack together" is worth. The biggest legal
///   one is the best, so a policy ranking on damage commands as many as are
///   standing.
/// - **Spending one** - an optional move that destroys a double for a last
///   effect ([`Spend::Summon`]), a burst as it comes apart.
///
/// The double's own attack profiles are also its stat block's actions, which
/// is what "what is a double worth" is read off when deciding whether to call
/// one up at all - see `sim::fight`'s valuation.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandedDoublePlugin {
    pub double: Creature,
    pub summon_name: String,
    pub max_active: u32,
    pub resource: Option<(String, u32)>,
    /// Attack profiles, written in the scenario DSL, the summoner can
    /// command - already parsed onto `double`'s own actions.
    pub scale_with_count: bool,
    /// A move that destroys one double, written in the scenario DSL, and its
    /// name.
    pub spend: Option<(String, String)>,
}

/// The same strike with `k` times the dice: `k` doubles striking as one, each
/// adding a die. Flat bonuses - the summoner's own spellcasting modifier -
/// are added once however many of them there are.
fn together(effect: &Effect, k: u32) -> Effect {
    match effect {
        Effect::Strikes { strike, count } => Effect::Strikes {
            strike: Strike {
                damage: strike
                    .damage
                    .iter()
                    .map(|roll| {
                        let mut scaled = *roll;
                        scaled.count *= k;
                        scaled
                    })
                    .collect(),
                ..strike.clone()
            },
            count: *count,
        },
        Effect::Sequence(parts) => Effect::Sequence(parts.iter().map(|p| together(p, k)).collect()),
        other => other.clone(),
    }
}

impl FeaturePlugin for CommandedDoublePlugin {
    fn id(&self) -> &'static str {
        "commanded_double"
    }

    fn name(&self) -> &str {
        &self.double.name
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        if self.max_active == 0 {
            return Err(FeatureError::InvalidConfiguration(
                "commanded_double needs `max_active` of at least one".to_string(),
            ));
        }
        let which = builder.creature.add_summon(self.double.clone());

        let mut call = Move::new(self.summon_name.clone(), Effect::Summon { which }).requiring(
            Requirement::SummonRoom {
                which,
                max: self.max_active,
            },
        );
        if let Some((pool, amount)) = &self.resource {
            call = call.with_cost(Cost {
                resource: builder.resource_index(pool)?,
                amount: *amount,
            });
        }
        builder.add_bonus_action(call);

        // One command per attack profile, per number of doubles that could
        // strike together.
        let counts = match self.scale_with_count {
            true => 1..=self.max_active,
            false => 1..=1,
        };
        for profile in &self.double.actions {
            for k in counts.clone() {
                let name = match k {
                    1 => profile.name.clone(),
                    k => format!("{} (x{k})", profile.name),
                };
                let mut commanded = profile.clone();
                commanded.name = name;
                commanded.effect = together(&profile.effect, k);
                builder
                    .add_bonus_action(commanded.requiring(Requirement::Summon { which, count: k }));
            }
        }

        if let Some((name, effect)) = &self.spend {
            let burst =
                grammar::parse_move_external(&format!("{name} | {effect}"), &builder.creature)
                    .map_err(FeatureError::InvalidConfiguration)?
                    .requiring(Requirement::Summon { which, count: 1 })
                    .spending(Spend::Summon(which));
            builder.add_bonus_action(burst);
        }
        Ok(())
    }
}

/// Read `resist`/`immune`/`vulnerable` lists onto the double.
fn reductions(val: &toml::Value, double: &mut Creature) -> FeatureResult<()> {
    for (key, reduction) in [
        ("resist", Reduction::Resistant),
        ("immune", Reduction::Immune),
        ("vulnerable", Reduction::Vulnerable),
    ] {
        let Some(list) = val.get(key).and_then(|v| v.as_array()) else {
            continue;
        };
        for entry in list {
            let name = entry.as_str().ok_or_else(|| {
                FeatureError::InvalidConfiguration(format!(
                    "commanded_double `{key}` is a list of damage types"
                ))
            })?;
            let kind = DamageKind::parse(name)
                .ok_or_else(|| FeatureError::UnknownDamageKind(name.to_string()))?;
            double.reductions.push((kind, reduction));
        }
    }
    Ok(())
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // A double and everything its summoner can do with it: its `name`, `ac`
    // and `hp`, what it `resist`s / is `immune` to / is `vulnerable` to, how
    // many can stand at once (`max_active`), what calling one up costs
    // (`resource` + `cost`) and what that move is called (`summon_name`),
    // the `attacks` it can be commanded to make (moves in the scenario DSL),
    // whether they add a die per double striking together
    // (`scale_with_count`), and an optional move that destroys one
    // (`spend_name` + `spend`).
    registry.register("commanded_double", |val| {
        let name = val.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            FeatureError::InvalidConfiguration("commanded_double needs a `name`".to_string())
        })?;
        let number =
            |key: &str, default: i64| val.get(key).and_then(|v| v.as_integer()).unwrap_or(default);
        let hp = number("hp", 0);
        if hp <= 0 {
            return Err(FeatureError::InvalidConfiguration(format!(
                "commanded_double `{name}` needs positive `hp`"
            )));
        }
        let mut double = Creature::new(name, number("ac", 10) as i32, hp as i32);
        reductions(val, &mut double)?;

        // The profiles are parsed against the double itself, so a cost
        // clause in one would name the double's own pools - which it has
        // none of, on purpose: what a command costs is the summoner's
        // business, and the summoner pays it when it calls the double up.
        let attacks = val
            .get("attacks")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                FeatureError::InvalidConfiguration(format!(
                    "commanded_double `{name}` needs `attacks`: what it can be commanded to do"
                ))
            })?;
        for entry in attacks {
            let phrase = entry.as_str().ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "commanded_double `attacks` is a list of moves in the scenario DSL".to_string(),
                )
            })?;
            let mut m = grammar::parse_move_external(phrase, &double)
                .map_err(FeatureError::InvalidConfiguration)?;
            // A double's strike is cast through it; nothing it does is a
            // weapon attack it could be disarmed of.
            if m.kind == MoveKind::Standard {
                m.kind = MoveKind::Spell;
            }
            double.actions.push(m);
        }

        let spend = match (
            val.get("spend").and_then(|v| v.as_str()),
            val.get("spend_name").and_then(|v| v.as_str()),
        ) {
            (None, _) => None,
            (Some(effect), Some(spend_name)) => Some((spend_name.to_string(), effect.to_string())),
            (Some(effect), None) => Some((format!("{name} (spent)"), effect.to_string())),
        };

        Ok(Box::new(CommandedDoublePlugin {
            summon_name: val
                .get("summon_name")
                .and_then(|v| v.as_str())
                .unwrap_or(name)
                .to_string(),
            max_active: number("max_active", 1).max(1) as u32,
            resource: val
                .get("resource")
                .and_then(|v| v.as_str())
                .map(|pool| (pool.to_string(), number("cost", 1).max(0) as u32)),
            scale_with_count: val
                .get("scale_with_count")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            spend,
            double,
        }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::FeatureRegistry;
    use crate::prob::Rng;
    use crate::rules::DamageRoll;
    use crate::sim::{run_teams, Budget, Policy};

    const DECLARED: &str = r#"
        name = "Water Clone"
        ac = 15
        hp = 57
        immune = ["cold"]
        resist = ["fire"]
        vulnerable = ["lightning"]
        max_active = 3
        resource = "essence"
        cost = 1
        summon_name = "Summon a Water Clone"
        scale_with_count = true
        attacks = [
            "Water Clone (melee) | spell | hit +7 | 1d8+3 cold",
            "Water Clone (ranged) | spell | ranged | hit +7 | 1d6+3 cold",
        ]
        spend_name = "Detonate a Water Clone"
        spend = "spell | save dex dc 15 | 28 cold | half"
    "#;

    fn summoner() -> Creature {
        let mut builder = CreatureBuilder::new("Summoner", 19, 114);
        builder.ensure_resource("essence", 6);
        let val: toml::Value = toml::from_str(DECLARED).expect("valid toml");
        FeatureRegistry::new()
            .build_plugin("commanded_double", &val)
            .expect("the plugin builds")
            .apply(&mut builder)
            .expect("the plugin applies");
        builder.creature
    }

    /// One double declared, and every move around it: call one up, command
    /// however many are standing, spend one.
    #[test]
    fn it_registers_calling_commanding_and_spending() {
        let c = summoner();
        assert_eq!(c.summons.len(), 1);
        let double = &c.summons[0];
        assert_eq!((double.ac, double.hp), (15, 57));
        assert_eq!(double.reduction(DamageKind::Cold), Reduction::Immune);
        assert_eq!(double.reduction(DamageKind::Fire), Reduction::Resistant);
        assert_eq!(
            double.reduction(DamageKind::Lightning),
            Reduction::Vulnerable
        );

        let names: Vec<&str> = c.bonus_actions.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Summon a Water Clone",
                "Water Clone (melee)",
                "Water Clone (melee) (x2)",
                "Water Clone (melee) (x3)",
                "Water Clone (ranged)",
                "Water Clone (ranged) (x2)",
                "Water Clone (ranged) (x3)",
                "Detonate a Water Clone",
            ]
        );

        let call = &c.bonus_actions[0];
        assert_eq!(call.effect, Effect::Summon { which: 0 });
        assert_eq!(
            call.requires,
            Some(Requirement::SummonRoom { which: 0, max: 3 })
        );
        assert_eq!(call.cost.map(|c| c.amount), Some(1));

        // Each command needs that many doubles standing, and hits that much
        // harder - one die each, the flat bonus once.
        for (i, (k, dice)) in [(1, 1), (2, 2), (3, 3)].into_iter().enumerate() {
            let m = &c.bonus_actions[1 + i];
            assert_eq!(m.requires, Some(Requirement::Summon { which: 0, count: k }));
            let Effect::Strikes { strike, .. } = &m.effect else {
                panic!("a command is an attack");
            };
            assert_eq!(
                strike.damage,
                vec![DamageRoll::new(dice, 8, 3, DamageKind::Cold)]
            );
            assert_eq!(strike.to_hit, 7, "the summoner's own spell attack bonus");
        }

        let burst = c.bonus_actions.last().expect("a spend move");
        assert_eq!(
            burst.requires,
            Some(Requirement::Summon { which: 0, count: 1 })
        );
        assert_eq!(burst.spends, Some(Spend::Summon(0)));
    }

    /// In a live fight: the double is called up and then commanded, turn
    /// after turn, out of the summoner's own bonus action.
    ///
    /// The burst is taken out of this build first, because with one target
    /// to hit it is simply worth more than another jab - which is the right
    /// answer, and not the one this test is about.
    #[test]
    fn a_double_is_called_up_and_then_commanded() {
        let mut me = summoner();
        me.bonus_actions.retain(|m| m.spends.is_none());
        me.initiative = 10;
        me.actions.push(
            grammar::parse_move_external(
                "Blades | strikes 2 | hit +11 | 1d6+7 slashing",
                &Creature::new("x", 10, 10),
            )
            .expect("the attack parses"),
        );

        let mut foe = Creature::new("Foe", 16, 300);
        foe.team = 1;
        foe.actions.push(
            grammar::parse_move_external(
                "Bite | hit +9 | 2d10+5 piercing",
                &Creature::new("x", 10, 10),
            )
            .expect("the attack parses"),
        );

        let mut rng = Rng::new(17);
        let mut log = Some(Vec::new());
        let outcome = run_teams(
            &mut rng,
            &[&me, &foe],
            [Policy::Greedy; 2],
            8,
            Budget::default(),
            &mut log,
        );
        let narration = log.unwrap().join("\n");
        assert!(
            narration.contains("Water Clone stands up"),
            "the double should be called up:\n{narration}"
        );
        assert!(
            narration.contains("Water Clone (melee)"),
            "and then commanded:\n{narration}"
        );
        assert!(
            outcome.deaths[0] <= 1,
            "a destroyed double is not a death on its side"
        );
    }
}
