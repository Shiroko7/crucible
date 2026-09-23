//! A whole divine caster written as data, and every piece of it live in a
//! fight: Channel Divinity spent on a burst, on a heal and on turning the
//! Undead, a storm answering whoever closes with it, a cast taken at its
//! maximum for a charge, spirits raised in a ring, and lightning that shoves.
//!
//! The build is a stand-in with neutral names - a storm priest, not any
//! published subclass - the way `multiclass_rogue_caster.rs` and
//! `dual_wielding_summoner.rs` are. What it proves is that the plugins and
//! trait phrases this needed compose into one creature, and that each of
//! them actually changes what happens in a real fight rather than only
//! parsing.
//!
//! A note on how the mechanism tests are run: several of these features are
//! worth nothing to a policy that ranks on damage (a heal, a turning) or are
//! reactions nobody chooses at all, so where a policy would simply never take
//! the move, the test narrows the creature to it and plays `InOrder` - the
//! same approach `defensive_caster.rs` takes and for the same reason.

use crucible_core::creature::{AttackTrigger, Creature, Effect, ReactionTrigger, Rider};
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::dsl::scenario::parse;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::rules::{Condition, DamageKind, DamageRoll, Duration, Size};
use crucible_core::sim::{run_teams, Budget, Policy};

/// Everything a divine caster of this shape has, as its sheet would write it.
/// Wisdom 20 and a +3 proficiency bonus, so every DC below is 16 - and every
/// plugin reads that off the profile rather than being told.
const PRIEST: &str = r#"
[pc]
name = "Storm Priest"
ac = 18
hp = 60
initiative = 0
traits = [
    "push on lightning up to large",
    "once per turn 1d8 thunder with a weapon",
]

[pc.abilities]
wis = 20

[pc.saves]
wis = 8

[pc.spellcasting]
ability = "wis"
ability_modifier = 5
proficiency_bonus = 3

[pc.resources]
channel_divinity = 3

[pc.resources.slots]
1 = 4
2 = 3
3 = 3

[[pc.features]]
plugin = "divine_spark"
dice = 2

[[pc.features]]
plugin = "turn_undead"
sear_dice = 5

[[pc.features]]
plugin = "retaliation"
name = "Wrath of the Storm"
damage = "2d8 lightning"
save = "dex"
trigger = "melee"
uses = 5

[[pc.features]]
plugin = "maximised_damage"
name = "Shatter (at its maximum)"
effect = "spell | slot 2 | save con dc 16 | 3d8 thunder | half on success"
kinds = ["thunder"]
resource = "channel_divinity"

[[pc.features]]
plugin = "shatter"

[[pc.features]]
plugin = "sacred_flame"
dice = 2

[[pc.features]]
plugin = "spirit_guardians"

[[pc.features]]
plugin = "shield_of_faith"

[[pc.features]]
plugin = "thunderwave"

[[pc.features]]
plugin = "call_lightning"

[[pc.actions]]
name = "Warhammer"
effect = "weapon warhammer | strikes 1 | hit +5 | 1d8+2 bludgeoning"
"#;

fn priest() -> Creature {
    load_creature_from_str(PRIEST, &FeatureRegistry::new()).expect("the priest loads")
}

/// Narrow the priest to one action so `InOrder` has no choice but to take it.
fn priest_with_only(action: &str) -> Creature {
    let mut c = priest();
    c.actions.retain(|m| m.name == action);
    c.bonus_actions.clear();
    c.team = 0;
    c
}

fn foe(text: &str) -> Creature {
    parse(text).expect("the foe parses")[0].clone()
}

/// A skeleton, so that turning the Undead has something to turn - and a
/// bandit that is not Undead, so that it can be shown not to catch one.
fn skeleton() -> Creature {
    foe(
        "creature: Skeleton\ntype: undead\nac: 14\nhp: 13\ninitiative: 3\n\
         saves: wis -1\n\
         action: Shortsword | strikes 1 | hit +4 | 1d6+3 piercing\n",
    )
}

fn bandit() -> Creature {
    foe(
        "creature: Bandit\ntype: humanoid\nac: 14\nhp: 13\ninitiative: 3\n\
         saves: wis -1\n\
         action: Scimitar | strikes 1 | hit +4 | 1d6+3 slashing\n",
    )
}

/// Everything the sheet asked for is there, and the numbers every feature
/// needed came from the profile rather than being written down twice.
#[test]
fn the_whole_build_loads_with_every_feature_on_it() {
    let c = priest();
    let action = |name: &str| {
        c.actions
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("`{name}` should be an action"))
    };

    // Channel Divinity: a pool, and three ways to spend it.
    assert_eq!(c.resources[0].name, "channel_divinity");
    assert_eq!(c.resources[0].max, 3);
    for name in [
        "Divine Spark",
        "Divine Spark (Heal)",
        "Turn Undead (Sear)",
        "Shatter (at its maximum)",
    ] {
        assert_eq!(
            action(name).cost.map(|cost| cost.amount),
            Some(1),
            "`{name}` spends a use of Channel Divinity"
        );
    }

    // Every save it forces is at the one DC its profile computes.
    for name in [
        "Divine Spark",
        "Turn Undead (Sear)",
        "Shatter",
        "Sacred Flame",
    ] {
        let Effect::Save(save) = &action(name).effect else {
            panic!("`{name}` should force a saving throw")
        };
        assert_eq!(save.dc, 16, "`{name}` reads the priest's own DC");
    }

    // The storm answers a blade, five times, at that same DC.
    let reaction = &c.reactions[0];
    assert_eq!(
        reaction.trigger,
        ReactionTrigger::Hit(AttackTrigger::MeleeAttack)
    );
    assert_eq!(
        reaction.action.uses,
        crucible_core::creature::Uses::Limited(5)
    );

    // The two trait phrases, read back as the riders they stand for.
    assert!(c.riders.contains(&Rider::PushOnDamage {
        kinds: vec![DamageKind::Lightning],
        max_size: Size::Large,
    }));
    assert!(c.riders.contains(&Rider::OncePerTurnDamage {
        dice_count: 1,
        dice_sides: 8,
        bonus: 0,
        damage_kind: DamageKind::Thunder,
        weapon_only: true,
    }));
}

/// The maximised cast is the same spell with its dice already spent: a flat
/// 24 where the ordinary one rolls 3d8, and a charge on top of the slot.
#[test]
fn a_charge_buys_the_cast_at_its_maximum() {
    let c = priest();
    let damage_of = |name: &str| {
        let m = c.actions.iter().find(|m| m.name == name).expect("declared");
        let Effect::Save(save) = &m.effect else {
            panic!("`{name}` should force a saving throw")
        };
        (save.damage.clone(), m.spell_slot_level, m.cost.is_some())
    };
    assert_eq!(
        damage_of("Shatter"),
        (
            vec![DamageRoll::new(3, 8, 0, DamageKind::Thunder)],
            Some(2),
            false
        )
    );
    assert_eq!(
        damage_of("Shatter (at its maximum)"),
        (
            vec![DamageRoll::new(0, 1, 24, DamageKind::Thunder)],
            Some(2),
            true
        ),
        "3d8 thunder, maximised, is a flat 24 - and costs a charge as well as the slot"
    );

    // Live: it lands exactly 24 on a failed save, every time.
    let dummy = foe("creature: Dummy\nac: 10\nhp: 4000\ninitiative: -20\nsaves: con -5\n");
    let mut me = priest_with_only("Shatter (at its maximum)");
    let mut them = dummy;
    them.team = 1;
    let mut rng = Rng::new(5);
    let mut log = Some(Vec::new());
    run_teams(
        &mut rng,
        &[&me, &them],
        [Policy::InOrder; 2],
        2,
        Budget::default(),
        &mut log,
    );
    let narration = log.expect("a log was asked for").join("\n");
    assert!(
        narration.contains("failed for 24") || narration.contains("saved for 12"),
        "a maximised 3d8 is 24, or 12 halved:\n{narration}"
    );
    me.actions.clear();
}

/// Turning the Undead: a Wisdom save nothing but Undead is even caught by,
/// leaving what fails unable to act - and the searing damage does not undo
/// its own turning, though anyone else's blow does.
#[test]
fn turning_undead_catches_only_undead_and_takes_their_turn_away() {
    let run = |target: Creature| {
        let mut me = priest_with_only("Turn Undead (Sear)");
        // The storm would answer the blows in the meantime and muddy what
        // this is about, so it is left out: this is the turning alone.
        me.reactions.clear();
        let mut them = target;
        them.team = 1;
        them.initiative = -20;
        let mut rng = Rng::new(17);
        let mut log = Some(Vec::new());
        let out = run_teams(
            &mut rng,
            &[&me, &them],
            [Policy::InOrder; 2],
            3,
            Budget::default(),
            &mut log,
        );
        (out, log.expect("a log was asked for").join("\n"))
    };

    // A skeleton with a hopeless Wisdom save: seared, and then unable to act.
    let mut sure = skeleton();
    sure.hp = 400;
    let (out, narration) = run(sure);
    assert!(
        out.damage_dealt[0] > 0,
        "the searing damage should have landed:\n{narration}"
    );
    assert!(
        narration.contains("Skeleton failed"),
        "the skeleton should have been caught:\n{narration}"
    );
    assert!(
        narration.contains("frightened") && narration.contains("incapacitated"),
        "turning lands both conditions off the one save:\n{narration}"
    );
    assert!(
        narration.contains("loses its turn"),
        "an incapacitated creature does not act:\n{narration}"
    );

    // A living bandit is not caught at all - it never rolls, never takes the
    // searing damage, and keeps its turn.
    let (out, narration) = run(bandit());
    assert_eq!(
        out.damage_dealt[0], 0,
        "nothing but Undead is even caught by it:\n{narration}"
    );
    assert!(
        !narration.contains("frightened") && !narration.contains("loses its turn"),
        "and it goes on acting:\n{narration}"
    );
}

/// "Or until it takes any damage": somebody else's blow ends the turning.
#[test]
fn a_blow_from_anyone_snaps_a_turned_creature_out_of_it() {
    let mut me = priest_with_only("Turn Undead (Sear)");
    me.initiative = 20;
    // An ally that hits the skeleton the round after it is turned.
    let mut ally = foe("creature: Ally\nac: 18\nhp: 200\ninitiative: 10\n\
         action: Jab | strikes 1 | hit +20 | 1 bludgeoning\n");
    ally.team = 0;
    let mut them = skeleton();
    them.hp = 400;
    them.team = 1;
    them.initiative = -20;

    let mut rng = Rng::new(23);
    let mut log = Some(Vec::new());
    run_teams(
        &mut rng,
        &[&me, &ally, &them],
        [Policy::InOrder; 2],
        3,
        Budget::default(),
        &mut log,
    );
    let lines = log.expect("a log was asked for");
    let narration = lines.join("\n");
    assert!(
        narration.contains("frightened"),
        "it should have been turned first:\n{narration}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("r2 ") && l.contains("Skeleton: Shortsword")),
        "the ally's jab should have snapped it out of it by the next round:\n{narration}"
    );
}

/// The storm answers whoever closes with the priest: a melee attacker that
/// hits pays for it, and the answer is capped at the one reaction a round.
#[test]
fn whoever_hits_it_in_melee_answers_the_storm() {
    let raider = foe(
        "creature: Raider\nac: 10\nhp: 400\ninitiative: -20\nsaves: dex 0\n\
         action: Axe | strikes 3 | hit +12 | 1d6 slashing\n",
    );
    let taken = |with_storm: bool| {
        let mut me = priest_with_only("Warhammer");
        if !with_storm {
            me.reactions.clear();
        }
        let mut them = raider.clone();
        them.team = 1;
        let mut rng = Rng::new(29);
        let mut total = 0i64;
        for _ in 0..200 {
            let mut log = None;
            let out = run_teams(
                &mut rng,
                &[&me, &them],
                [Policy::InOrder; 2],
                4,
                Budget::default(),
                &mut log,
            );
            total += out.damage_dealt[0];
        }
        total
    };
    let (with, without) = (taken(true), taken(false));
    assert!(
        with > without,
        "swinging at a storm priest should cost the raider: {with} vs {without}"
    );

    // One a round, however many times it is hit.
    let me = priest_with_only("Warhammer");
    let mut them = raider;
    them.team = 1;
    let mut rng = Rng::new(31);
    for _ in 0..40 {
        let mut log = Some(Vec::new());
        run_teams(
            &mut rng,
            &[&me, &them],
            [Policy::InOrder; 2],
            1,
            Budget::default(),
            &mut log,
        );
        let answered = log
            .expect("a log was asked for")
            .join("\n")
            .matches("Wrath of the Storm")
            .count();
        assert!(answered <= 1, "one reaction a round, whatever hits it");
    }
}

/// Lightning shoves: the priest's own lightning leaves its victim Pushed,
/// and its thunder does not.
#[test]
fn its_lightning_shoves_what_it_hits_and_its_thunder_does_not() {
    let pushed_after = |action: &str| {
        let me = priest_with_only(action);
        let mut them =
            foe("creature: Target\nac: 10\nhp: 4000\ninitiative: -20\nsaves: dex -5, con -5\n");
        them.team = 1;
        let mut rng = Rng::new(37);
        let mut log = Some(Vec::new());
        run_teams(
            &mut rng,
            &[&me, &them],
            [Policy::InOrder; 2],
            2,
            Budget::default(),
            &mut log,
        );
        log.expect("a log was asked for")
            .join("\n")
            .contains("pushed")
    };
    assert!(
        pushed_after("Call Lightning"),
        "lightning throws a Large or smaller creature back"
    );
    assert!(
        !pushed_after("Shatter"),
        "thunder is not what the rider names"
    );
}

/// Divine Spark, both ways: a burst at an enemy, and the same charge spent
/// to bring an ally back up.
#[test]
fn a_charge_can_be_spent_on_a_burst_or_on_a_dying_ally() {
    // The burst: 2d8 + Wisdom against a Constitution save.
    let c = priest();
    let spark = c
        .actions
        .iter()
        .find(|m| m.name == "Divine Spark")
        .expect("declared");
    let Effect::Save(save) = &spark.effect else {
        panic!("the burst is a saving throw")
    };
    assert_eq!(
        save.damage,
        vec![DamageRoll::new(2, 8, 5, DamageKind::Radiant)]
    );

    // The heal: the ally that got hit is the one the charge goes to, not the
    // untouched priest. `InOrder` aims at the first enemy on the roster, so
    // the ally standing first is the one the raider swings at.
    let mut me = priest_with_only("Divine Spark (Heal)");
    me.initiative = -20;
    let mut ally = foe("creature: Ally\nac: 10\nhp: 200\ninitiative: -30\n");
    ally.team = 0;
    let mut raider = foe("creature: Raider\nac: 20\nhp: 400\ninitiative: 20\n\
         action: Axe | strikes 1 | hit +20 | 30 slashing\n");
    raider.team = 1;

    let mut rng = Rng::new(41);
    let mut log = Some(Vec::new());
    run_teams(
        &mut rng,
        &[&ally, &me, &raider],
        [Policy::InOrder; 2],
        3,
        Budget::default(),
        &mut log,
    );
    let narration = log.expect("a log was asked for").join("\n");
    assert!(
        narration.contains("heals Ally"),
        "the charge should have gone to whoever needed it:\n{narration}"
    );
}

/// The spirits, raised and held: an enemy that starts its turn beside the
/// priest answers them, and the slow lands on a failure.
#[test]
fn the_raised_spirits_catch_whoever_starts_its_turn_in_them() {
    let mut me = priest_with_only("Spirit Guardians");
    me.initiative = 20;
    let mut them = foe(
        "creature: Raider\nac: 20\nhp: 4000\ninitiative: -20\nsaves: wis -5\n\
         action: Axe | strikes 1 | hit +0 | 1d6 slashing\n",
    );
    them.team = 1;

    let mut rng = Rng::new(43);
    let mut log = Some(Vec::new());
    let out = run_teams(
        &mut rng,
        &[&me, &them],
        [Policy::InOrder; 2],
        4,
        Budget::default(),
        &mut log,
    );
    let narration = log.expect("a log was asked for").join("\n");
    assert!(
        narration.contains("(aura)"),
        "the spirits should be met at the start of a turn:\n{narration}"
    );
    assert!(
        narration.contains("slowed"),
        "and slow what fails against them:\n{narration}"
    );
    assert!(
        out.damage_dealt[0] > 0,
        "the spirits are where the damage comes from"
    );
}

/// The ward: Armor Class for as long as concentration holds it, and nothing
/// else - so the same attacker lands less once it is up.
#[test]
fn the_ward_is_armor_class_and_only_that() {
    let c = priest();
    let boon = c
        .boons
        .iter()
        .find(|b| b.name == "Shield of Faith")
        .expect("the ward is declared");
    assert_eq!(boon.ac, 2);
    assert!(boon.damage.is_none());
    assert!(boon.resist.is_empty());

    let taken = |warded: bool| {
        let mut me = priest();
        me.actions.clear();
        me.bonus_actions
            .retain(|m| warded && m.name == "Shield of Faith");
        me.team = 0;
        let mut them = foe("creature: Raider\nac: 20\nhp: 400\ninitiative: -20\n\
             action: Jab | strikes 2 | hit +6 | 1d4 bludgeoning\n");
        them.team = 1;
        let mut rng = Rng::new(47);
        let mut total = 0i64;
        for _ in 0..300 {
            let mut log = None;
            let out = run_teams(
                &mut rng,
                &[&me, &them],
                [Policy::InOrder; 2],
                5,
                Budget::default(),
                &mut log,
            );
            total += out.damage_dealt[1];
        }
        total
    };
    let (with, without) = (taken(true), taken(false));
    assert!(
        with < without,
        "the ward should turn some hits into misses: {with} vs {without}"
    );
}

/// A weapon hit carries thunder with it once a turn - and a spell does not,
/// which is the whole point of writing it `with a weapon`.
#[test]
fn the_strike_rides_a_weapon_hit_and_not_a_spell() {
    let dealt = |action: &str, seed: u64| {
        let me = priest_with_only(action);
        let mut them = foe("creature: Dummy\nac: 1\nhp: 4000\ninitiative: -20\nsaves: dex -5\n");
        them.team = 1;
        let mut rng = Rng::new(seed);
        let mut total = 0i64;
        for _ in 0..200 {
            let mut log = None;
            let out = run_teams(
                &mut rng,
                &[&me, &them],
                [Policy::InOrder; 2],
                1,
                Budget::default(),
                &mut log,
            );
            total += out.damage_dealt[0];
        }
        f64::from(u32::try_from(total).expect("positive")) / 200.0
    };

    // A warhammer is 1d8+2 (6.5) plus 1d8 thunder (4.5) once a turn, against
    // Armor Class 1 - so well above the weapon alone.
    let hammer = dealt("Warhammer", 53);
    assert!(
        hammer > 9.0,
        "a weapon hit should carry the strike: {hammer} a round"
    );

    // Sacred Flame is 2d8 (9) on a failed Dexterity save and nothing on a
    // made one; with the strike wrongly riding it, it would be half again as
    // much. Its target fails almost always, so the ceiling is tight.
    let flame = dealt("Sacred Flame", 59);
    assert!(
        flame < 9.5,
        "a cantrip is not a weapon attack, so the strike must not ride it: {flame} a round"
    );
}

/// A reaction can also be written straight into a sheet, with the trigger
/// phrase the grammar reads - the path a monster's stat block uses.
#[test]
fn a_sheet_can_declare_a_reaction_by_hand() {
    let text = r#"
[pc]
name = "Thornback"
ac = 15
hp = 40

[[pc.reactions]]
name = "Thorns"
effect = "when hit in melee | save dex dc 13 | 1d6 piercing"
"#;
    let c = load_creature_from_str(text, &FeatureRegistry::new()).expect("loads");
    assert_eq!(
        c.reactions[0].trigger,
        ReactionTrigger::Hit(AttackTrigger::MeleeAttack)
    );
    let Effect::Save(save) = &c.reactions[0].action.effect else {
        panic!("expected a saving throw")
    };
    assert_eq!(save.dc, 13);
}

/// The conditions turning lands are the ones it says: Frightened for its own
/// clock, and Incapacitated, both ending on damage.
#[test]
fn turning_lands_both_conditions_for_a_minute_or_until_damaged() {
    let c = priest();
    let turn = c
        .actions
        .iter()
        .find(|m| m.name == "Turn Undead (Sear)")
        .expect("declared");
    let Effect::Save(save) = &turn.effect else {
        panic!("expected a saving throw")
    };
    assert_eq!(save.requires_type.as_deref(), Some("Undead"));
    assert_eq!(
        save.on_failure,
        vec![
            (Condition::Frightened, Duration::RoundsOrDamaged(10)),
            (Condition::Incapacitated, Duration::RoundsOrDamaged(10)),
        ]
    );
    assert_eq!(
        save.damage,
        vec![DamageRoll::new(5, 8, 0, DamageKind::Radiant)],
        "seared at the priest's Wisdom modifier"
    );
}
