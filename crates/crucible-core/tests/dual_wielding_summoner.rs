//! A whole two-weapon build - a marked quarry, a swarm that joins one blow a
//! turn, an enchantment on one of its two blades, a form it can put on for a
//! minute, a parry that stays up, a ring that rescues a failed save, and
//! doubles it calls up and commands - declared as TOML through every plugin
//! and trait keyword such a build needs, compiled, and then fought.
//!
//! The build is a stand-in with neutral names, not any specific character:
//! non-SRD characters live in the user's own local files (see the README).
//! What it proves is that each mechanism such a character leans on is not
//! only loadable but *live* - that a real fight actually applies it.

use crucible_core::creature::{Creature, Effect, Requirement, Rider, Spend};
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::rules::{Ability, Condition, DamageKind, Reduction};
use crucible_core::sim::{run_teams, Budget, Policy};

const BUILD: &str = r#"
[pc]
name = "Test Duellist-Summoner"
ac = 19
hp = 114
initiative = 4
resist = ["cold", "fire"]
traits = [
    "once per turn 1d6 piercing",
    "reaction ac 4 vs melee until next turn",
    "always succeed dex 3 reaction",
]

[pc.abilities]
dex = 19
con = 19
wis = 16

[pc.saves]
str = 3
dex = 8
con = 4
wis = 3

[pc.resources]
essence = 6
potions = 1

[pc.resources.slots]
1 = 4
2 = 2

[pc.spellcasting]
ability = "wis"
ability_modifier = 3
proficiency_bonus = 4

[[pc.features]]
plugin = "hunters_mark"
dice_count = 1
free_uses = 3
slot = 1

[[pc.features]]
plugin = "commanded_double"
name = "Test Double"
ac = 15
hp = 57
immune = ["cold"]
resist = ["fire"]
vulnerable = ["lightning"]
max_active = 3
resource = "essence"
cost = 1
summon_name = "Test Double (summon)"
scale_with_count = true
attacks = [
    "Test Double (melee) | spell | hit +7 | 1d8+3 cold",
    "Test Double (ranged) | spell | ranged | hit +7 | 1d6+3 cold",
]
spend_name = "Test Double (detonate)"
spend = "spell | save dex dc 15 | 28 cold | half"

[[pc.features]]
plugin = "lasting_boon"
name = "Blessed Blade"
bonus_action = true
concentration = true
kind = "spell"
rounds = 600
uses = 1
dice_count = 2
dice_sides = 8
damage_kind = "radiant"
weapon = "Coldbrand"
dismiss_name = "Blessed Blade (burst)"
dismiss = "spell | save con dc 15 | 4d8 radiant | half | on fail blinded until save"

[[pc.features]]
plugin = "lasting_boon"
name = "Beast Form"
kind = "item"
rounds = 10
uses = 1
resist = ["bludgeoning", "piercing", "slashing"]
dice_count = 1
dice_sides = 6
damage_kind = "force"
weapon_only = true

[[pc.actions]]
name = "Attack"
effect = "strikes 2 | weapon shortblade | finesse | hit +10 | 1d6+6 slashing | on hit vexed until end && weapon coldbrand | finesse | hit +11 | 1d6+7 slashing or cold"

[[pc.actions]]
name = "Potion of Healing"
effect = "object | cost potions 1 | heal 2d4+2"

[[pc.bonus]]
name = "Coldbrand (off hand)"
effect = "weapon coldbrand | finesse | hit +11 | 1d6+7 slashing or cold"
"#;

fn build() -> Creature {
    load_creature_from_str(BUILD, &FeatureRegistry::new()).expect("the build compiles")
}

fn dummy(name: &str, ac: i32, hp: i32) -> Creature {
    let mut c = load_creature_from_str(
        &format!(
            r#"
            [monster]
            name = "{name}"
            ac = {ac}
            hp = {hp}
            [monster.saves]
            dex = 4
            con = 7
            [[monster.actions]]
            name = "Multiattack"
            effect = "strikes 3 | hit +12 | 2d6+6 slashing"
            "#
        ),
        &FeatureRegistry::new(),
    )
    .expect("the dummy compiles");
    c.team = 1;
    c
}

#[test]
fn the_build_compiles_to_the_numbers_it_declares() {
    let c = build();
    assert_eq!((c.ac, c.hp, c.initiative), (19, 114, 4));
    assert!(c.player_character);
    assert_eq!(c.spell_attack_bonus(), Some(7));
    assert_eq!(c.spell_save_dc(), Some(15));
    assert_eq!(c.reduction(DamageKind::Cold), Reduction::Resistant);

    // Three traits, three riders - plus the one the mark's plugin adds.
    assert!(c.riders.contains(&Rider::OncePerTurnDamage {
        weapon_only: false,
        dice_count: 1,
        dice_sides: 6,
        bonus: 0,
        damage_kind: DamageKind::Piercing,
    }));
    assert!(c.riders.contains(&Rider::BonusDamageVsQuarry {
        dice_count: 1,
        dice_sides: 6,
        bonus: 0,
        damage_kind: DamageKind::Force,
    }));
    assert!(c.riders.iter().any(|r| matches!(
        r,
        Rider::AlwaysSucceed {
            uses: 3,
            ability: Some(Ability::Dex),
            reaction: true
        }
    )));
    assert!(c.riders.iter().any(|r| matches!(
        r,
        Rider::ReactionOnTargeted {
            ac_bonus: 4,
            lasting: true,
            ..
        }
    )));

    // The Attack action: two swings that mark, then the other blade, which
    // can land as either of two damage types.
    let attack = c
        .actions
        .iter()
        .find(|m| m.name == "Attack")
        .expect("an Attack action");
    let Effect::Sequence(parts) = &attack.effect else {
        panic!("three swings in two parts");
    };
    let Effect::Part { riders, .. } = &parts[0] else {
        panic!("the first part carries the mastery");
    };
    assert!(riders.iter().any(|r| matches!(
        r,
        Rider::ConditionOnHit {
            condition: Condition::Vexed,
            ..
        }
    )));
    let Effect::Strikes { strike, .. } = &parts[1] else {
        panic!("the off-hand swing");
    };
    assert!(strike.made_with("Coldbrand"));
    assert_eq!(strike.damage[0].alternative, Some(DamageKind::Cold));

    // Both boons, the blade one pinned to one blade.
    let boon = |name: &str| {
        c.boons
            .iter()
            .find(|b| b.name == name)
            .unwrap_or_else(|| panic!("no boon {name}"))
    };
    assert_eq!(boon("Blessed Blade").weapon.as_deref(), Some("Coldbrand"));
    assert!(boon("Beast Form").weapon_only);
    assert_eq!(boon("Beast Form").resist.len(), 3);

    // The double, and every move that needs one standing.
    assert_eq!(c.summons.len(), 1);
    assert_eq!((c.summons[0].ac, c.summons[0].hp), (15, 57));
    let bonus = |name: &str| {
        c.bonus_actions
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("no bonus action {name}"))
    };
    assert_eq!(
        bonus("Test Double (summon)").requires,
        Some(Requirement::SummonRoom { which: 0, max: 3 })
    );
    assert_eq!(
        bonus("Test Double (melee) (x3)").requires,
        Some(Requirement::Summon { which: 0, count: 3 })
    );
    assert_eq!(
        bonus("Test Double (detonate)").spends,
        Some(Spend::Summon(0))
    );
    assert_eq!(
        bonus("Blessed Blade (burst)").requires,
        Some(Requirement::Boon { which: 0 })
    );
}

/// Played through whole fights, every live mechanism shows up in the
/// narration: the mark, the mastery's mark, a form put on, a double called up
/// and commanded, and the parry turning a hit into a miss.
#[test]
fn the_build_fights_with_every_mechanism_live() {
    let hero = build();
    let foe = dummy("Test Brute", 17, 600);
    let mut rng = Rng::new(9);
    let mut narration = String::new();
    for _ in 0..40 {
        let mut log = Some(Vec::new());
        run_teams(
            &mut rng,
            &[&hero, &foe],
            [Policy::Nova, Policy::Greedy],
            8,
            Budget::default(),
            &mut log,
        );
        narration.push_str(&log.unwrap().join("\n"));
        narration.push('\n');
    }
    for needle in [
        "quarry",
        "vexed",
        "Beast Form",
        "Blessed Blade",
        "Test Double stands up",
        "AC boosted",
    ] {
        assert!(
            narration.contains(needle),
            "expected `{needle}` somewhere in forty fights:\n{}",
            &narration[..narration.len().min(3000)]
        );
    }
}

/// A playstyle that reaches for the most expensive thing it can find calls
/// up every double it can pay for and then sets them all on the enemy at
/// once - which is worth more than any one of them alone.
///
/// The rest of the bonus actions are taken away here on purpose. Whether
/// calling up a double beats swinging a second blade is a judgement about
/// *this* build's numbers; that they gather while there is room and then
/// strike together is a fact about the mechanism.
#[test]
fn doubles_are_called_up_and_then_set_on_the_enemy_together() {
    let mut hero = build();
    // Only the doubles, and no burst: letting one go in a burst is worth
    // more than another jab from it, so with one available none of them
    // would ever gather.
    hero.bonus_actions
        .retain(|m| m.name.starts_with("Test Double") && m.spends.is_none());
    let foe = dummy("Test Brute", 17, 800);

    let mut rng = Rng::new(23);
    let mut narration = String::new();
    for _ in 0..20 {
        let mut log = Some(Vec::new());
        run_teams(
            &mut rng,
            &[&hero, &foe],
            [Policy::Nova, Policy::Greedy],
            10,
            Budget::default(),
            &mut log,
        );
        narration.push_str(&log.unwrap().join("\n"));
        narration.push('\n');
    }
    for needle in ["Test Double stands up", "Test Double (melee) (x3)"] {
        assert!(
            narration.contains(needle),
            "expected `{needle}` somewhere in twenty fights:\n{}",
            &narration[..narration.len().min(3000)]
        );
    }
}

/// Take the second damage type off every swing in an effect, leaving a
/// single-edged copy to compare against.
fn strip_alternatives(effect: &mut Effect) {
    match effect {
        Effect::Strikes { strike, .. } => {
            for roll in strike.damage.iter_mut() {
                roll.alternative = None;
            }
        }
        Effect::Sequence(parts) => parts.iter_mut().for_each(strip_alternatives),
        Effect::Part { effect, .. } => strip_alternatives(effect),
        _ => {}
    }
}

/// A blade that can land as cold or as steel lands as whichever the creature
/// in front of it is worse against - and the difference is real damage, not a
/// label.
#[test]
fn the_two_edged_blade_lands_as_whichever_type_gets_through() {
    let hero = build();
    let attack = hero
        .actions
        .iter()
        .find(|m| m.name == "Attack")
        .expect("an Attack action");

    let mut steel_proof = dummy("Steel-Proof", 10, 400);
    for kind in [
        DamageKind::Slashing,
        DamageKind::Piercing,
        DamageKind::Bludgeoning,
    ] {
        steel_proof.reductions.push((kind, Reduction::Resistant));
    }
    let mut cold_proof = dummy("Cold-Proof", 10, 400);
    cold_proof
        .reductions
        .push((DamageKind::Cold, Reduction::Immune));

    // The same build with the choice taken away: one blade, one damage type.
    let mut steel_only = build();
    for m in steel_only
        .actions
        .iter_mut()
        .chain(&mut steel_only.bonus_actions)
    {
        strip_alternatives(&mut m.effect);
    }
    let single = steel_only
        .actions
        .iter()
        .find(|m| m.name == "Attack")
        .expect("an Attack action");

    let two_edged = |target: &Creature| crucible_core::sim::expected_damage(&hero, attack, target);
    let steel =
        |target: &Creature| crucible_core::sim::expected_damage(&steel_only, single, target);

    // Against a hide that shrugs off blades, the choice is worth real damage:
    // that swing lands as cold instead, whole.
    assert!(
        two_edged(&steel_proof) > steel(&steel_proof) + 1.0,
        "cold should get round a blade-proof hide: {:.1} against {:.1}",
        two_edged(&steel_proof),
        steel(&steel_proof)
    );
    // Against something that ignores cold, the blade stays a blade, so
    // having the option costs nothing at all.
    assert!(
        (two_edged(&cold_proof) - steel(&cold_proof)).abs() < 1e-9,
        "steel is unaffected by cold immunity: {:.1} against {:.1}",
        two_edged(&cold_proof),
        steel(&cold_proof)
    );
}

/// The doubles are kit, not combatants: they can be called up, commanded
/// while they stand, and destroyed without that being a death on the side
/// that called them.
#[test]
fn the_doubles_are_kit_rather_than_combatants() {
    let hero = build();
    let foe = dummy("Test Brute", 17, 800);
    let mut rng = Rng::new(15);
    let mut called = 0;
    let mut deaths = 0;
    for _ in 0..40 {
        let mut log = Some(Vec::new());
        let outcome = run_teams(
            &mut rng,
            &[&hero, &foe],
            [Policy::Nova, Policy::Greedy],
            10,
            Budget::default(),
            &mut log,
        );
        let narration = log.unwrap().join("\n");
        called += narration.matches("Test Double stands up").count();
        deaths += outcome.deaths[0];
    }
    assert!(called > 0, "the doubles should actually be called up");
    assert!(
        deaths <= 40,
        "at most one death a fight, and never a double's"
    );
}
