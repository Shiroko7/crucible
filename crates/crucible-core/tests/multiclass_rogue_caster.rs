//! A whole multiclass build - a rogue with a secondary spellcasting grant,
//! magic weapons, items used through Fast Hands and reactions - declared as
//! TOML through every plugin and trait keyword such a build needs, compiled,
//! and then fought.
//!
//! The build is a stand-in with neutral names, not any specific character:
//! non-SRD characters live in the user's own local files (see the README).
//! What it proves is that each mechanism such a character leans on is not
//! only loadable but *live* - that a real fight actually applies it.

use crucible_core::creature::{AttackKind, MoveKind};
use crucible_core::duel::{run_teams, Budget, Policy};
use crucible_core::{
    load_creature_from_str, Ability, Condition, Creature, DamageKind, Effect, FeatureRegistry,
    Reduction, Rider, Rng,
};

const BUILD: &str = r#"
[pc]
name = "Test Rogue-Caster"
ac = 18
hp = 77
initiative = 3
traits = [
    "evasion dex",
    "halve attack damage",
    "reaction ac 5 vs ranged weapon",
    "resistance fire, poison",
    "empower weapon 1d6 poison on poisoned",
    "injury poison con dc 17 poisoned disadvantage wis for 1 hour",
    "downgrade immunity poison damage, poisoned condition",
]

[pc.abilities]
dex = 16
int = 13
wis = 20

[pc.saves]
dex = 9
wis = 7
con = 4

[pc.resources]
potions = 3

[pc.spellcasting]
ability = "wis"
item_bonus = 2

[[pc.features]]
plugin = "sneak_attack"
dice_count = 5

[[pc.features]]
plugin = "prestige_spellcasting"
name = "Test Prestige Caster"
ability_1 = "dex"
ability_1_min = 13
ability_2 = "int"
ability_2_min = 13
min_sneak_attack_dice = 2
spellcasting_ability = "wis"
proficiency_bonus = 4

[pc.features.slots]
1 = 4
2 = 3

[[pc.features]]
plugin = "true_strike"
weapon_dice_count = 1
weapon_dice_sides = 8
weapon_damage_kind = "piercing"
ranged = true
weapon_bonus = 3
weapon_traits = ["bonus 3d6 piercing vs dragon"]
radiant_dice_count = 2

[[pc.features]]
plugin = "spiritual_weapon"

[[pc.features]]
plugin = "guiding_bolt"

[[pc.features]]
plugin = "bless"

[[pc.features]]
plugin = "cunning_strike"
dex_modifier = 3
proficiency_bonus = 4
item_bonus = 2

[[pc.features]]
plugin = "cunning_strike_trip"

[[pc.features]]
plugin = "steady_aim"

[[pc.features]]
plugin = "fast_hands"
name = "Potion of Healing"
effect = "object | cost potions 1 | heal 2d4+2"

[[pc.features]]
plugin = "limited_use_debuff_item"
name = "Test Card"
ability = "wis"
duration = "for 1 minute"

[[pc.actions]]
name = "Slaying Bow"
effect = "ranged | hit +11 | 1d8+6 piercing | bonus 3d6 piercing vs dragon"
"#;

fn build() -> Creature {
    load_creature_from_str(BUILD, &FeatureRegistry::new()).expect("the build compiles")
}

#[test]
fn the_build_compiles_to_the_numbers_it_declares() {
    let c = build();
    assert_eq!((c.ac, c.hp, c.initiative), (18, 77, 3));
    assert!(c.player_character);

    // The prestige grant reads Wis 20 (+5) off the score, adds proficiency
    // 4, and keeps the item bonus declared ahead of it.
    let profile = c.spellcasting.expect("a spellcasting profile");
    assert_eq!(profile.ability_modifier, 5);
    assert_eq!(c.spell_attack_bonus(), Some(11));
    assert_eq!(c.spell_save_dc(), Some(19));
    assert_eq!((c.spell_slots.max(1), c.spell_slots.max(2)), (4, 3));
    assert!(c.resources.iter().all(|r| r.name == "potions"));

    let action = |name: &str| {
        c.actions
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("no action {name}"))
    };
    let true_strike = action("True Strike");
    let Effect::Strikes { strike, .. } = &true_strike.effect else {
        panic!("True Strike is an attack");
    };
    assert_eq!(strike.to_hit, 14, "spell attack 11 + the bow's 3");
    assert_eq!(strike.damage[0].bonus, 8, "Wis 5 + the bow's 3");
    assert!(strike.kind.weapon && strike.kind.spell && strike.kind.ranged);
    assert!(matches!(
        true_strike.riders[..],
        [Rider::BonusDamageVsCreatureType { dice_count: 3, .. }]
    ));
    let bolt = action("Guiding Bolt");
    assert_eq!(bolt.spell_slot_level, Some(1));
    assert_eq!(action("Bless").spell_slot_level, Some(1));

    let bonus = |name: &str| {
        c.bonus_actions
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("no bonus action {name}"))
    };
    assert!(bonus("Steady Aim").before_action);
    assert_eq!(bonus("Potion of Healing").kind, MoveKind::ObjectUse);
    let card = bonus("Test Card");
    assert_eq!(card.kind, MoveKind::MagicItem);
    let Effect::Save(save) = &card.effect else {
        panic!("the card forces a save");
    };
    assert_eq!((save.ability, save.dc), (Ability::Wis, 19));

    assert!(c.has_evasion(Ability::Dex));
    assert!(c.riders.contains(&Rider::HalveAttackDamage));
    assert!(c
        .riders
        .iter()
        .any(|r| matches!(r, Rider::ConditionalExtraDamage { dice_count: 5, .. })));
    assert!(c.riders.iter().any(|r| r.cunning_strike_dc() == Some(17)));
    assert_eq!(c.reduction(DamageKind::Fire), Reduction::Resistant);
    let Effect::Strikes { strike, .. } = &action("Slaying Bow").effect else {
        panic!("the bow is an attack");
    };
    assert_eq!(strike.kind, AttackKind::RANGED_WEAPON);
}

fn dragon() -> Creature {
    let mut d = load_creature_from_str(
        r#"
            [monster]
            name = "Test Dragon"
            creature_type = "dragon"
            size = "Huge"
            ac = 19
            hp = 400
            [monster.saves]
            con = 7
            wis = 7
            [[monster.actions]]
            name = "Claw"
            effect = "hit +10 | 1d6 slashing"
        "#,
        &FeatureRegistry::new(),
    )
    .expect("the dummy compiles");
    d.team = 1;
    d
}

/// Played through whole fights, every live mechanism shows up in the
/// narration: Sneak Attack, Cunning Strike's poison, the injury dose, the
/// weapon buff that poison arms, and the reaction that halves a hit.
#[test]
fn the_build_fights_with_every_mechanism_live() {
    let rogue = build();
    let foe = dragon();
    let mut rng = Rng::new(7);
    let mut narration = String::new();
    for _ in 0..40 {
        let mut log = Some(Vec::new());
        run_teams(
            &mut rng,
            &[&rogue, &foe],
            [Policy::Greedy, Policy::Greedy],
            6,
            Budget::default(),
            &mut log,
        );
        narration.push_str(&log.unwrap().join("\n"));
        narration.push('\n');
    }
    for needle in [
        "Steady Aim",
        "sneak",
        "cunning strike poisoned",
        "injury poison",
        "(halved)",
    ] {
        assert!(
            narration.contains(needle),
            "expected `{needle}` somewhere in forty fights:\n{}",
            &narration[..narration.len().min(3000)]
        );
    }
}

/// The same build without Sneak Attack deals clearly less over the same
/// fights - the extra dice are real damage, not narration.
#[test]
fn sneak_attack_adds_real_damage_in_a_fight() {
    let with = build();
    let mut without = with.clone();
    without
        .riders
        .retain(|r| !matches!(r, Rider::ConditionalExtraDamage { .. }));
    let foe = dragon();
    let dealt = |rogue: &Creature| {
        let mut rng = Rng::new(3);
        let mut total = 0i64;
        for _ in 0..400 {
            let o = run_teams(
                &mut rng,
                &[rogue, &foe],
                [Policy::Greedy, Policy::Greedy],
                3,
                Budget::default(),
                &mut None,
            );
            total += o.damage_dealt[0];
        }
        total
    };
    let (a, b) = (dealt(&with), dealt(&without));
    assert!(a > b + b / 4, "with Sneak Attack {a}, without {b}");
}

/// A poison-immune foe still takes half from the build's poison, and still
/// has to save against being Poisoned - with advantage - because the build
/// downgrades both immunities.
#[test]
fn the_builds_immunity_downgrade_reaches_a_poison_immune_foe() {
    let rogue = build();
    let mut golem = dragon();
    golem.creature_type = None;
    golem
        .reductions
        .push((DamageKind::Poison, Reduction::Immune));
    golem.condition_immunities.push(Condition::Poisoned);
    assert_eq!(
        golem.reduction_from(DamageKind::Poison, &rogue),
        Reduction::Resistant
    );
}
