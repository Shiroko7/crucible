//! A caster whose spell is a ring of harm it stands inside: raised for a
//! cost, held by concentration, and paid for by whoever has to start a turn
//! next to it.
//!
//! The build is a stand-in with neutral names, not any specific character:
//! non-SRD characters live in the user's own local files (see the README).
//! What it proves is that a *raised* aura is live in a real fight - that it
//! is off until cast, that it then collects on every enemy turn rather than
//! once, and that nothing ever spends a second slot raising one already up.
//!
//! It also pins the concentration behaviour from both sides: against a foe
//! that never lands a blow the aura goes up once and stays, and against one
//! that hits hard it drops when concentration breaks and has to be paid for
//! again. The aura is held as an ordinary [`Condition::Aura`], so that second
//! half is `end_concentration` clearing it along with everything else rather
//! than a clock of its own.

use crucible_core::creature::{Creature, Effect};
use crucible_core::dsl::config::load_creature_from_str;
use crucible_core::features::FeatureRegistry;
use crucible_core::prob::Rng;
use crucible_core::rules::Condition;
use crucible_core::sim::{run_teams, Budget, Policy};

const CASTER: &str = r#"
[pc]
name = "Test Aura Caster"
ac = 17
hp = 90
initiative = 2

[pc.abilities]
wis = 18

[pc.saves]
wis = 8

[pc.resources.slots]
3 = 2

[pc.spellcasting]
ability = "wis"
ability_modifier = 4
proficiency_bonus = 5

[[pc.features]]
plugin = "lasting_aura"
name = "Ring of Spirits"
slot = 3
rounds = 100
concentration = true
effect = "save wis dc 17 | 3d8 radiant | half"

[[pc.actions]]
name = "Staff"
effect = "hit +7 | 1d6+4 bludgeoning"
"#;

fn caster() -> Creature {
    load_creature_from_str(CASTER, &FeatureRegistry::new()).expect("the caster loads")
}

/// How many times the aura was paid for, as opposed to collected: the
/// `(aura)` lines are it washing over somebody, every other mention is a turn
/// spent putting it back up.
fn raises(lines: &[String]) -> usize {
    lines
        .iter()
        .filter(|l| !l.contains("(aura)") && l.contains("Ring of Spirits"))
        .count()
}

/// The plugin declares the aura on the creature and an action that raises it,
/// rather than an aura that is simply always on.
#[test]
fn raising_an_aura_is_a_move_and_the_aura_is_not_yet_on() {
    let c = caster();
    assert!(
        c.auras.is_empty(),
        "nothing here is always on - it has to be cast"
    );
    assert_eq!(c.lasting_auras.len(), 1, "one aura is declared");
    assert_eq!(c.lasting_auras[0].name, "Ring of Spirits");

    let raise = c
        .actions
        .iter()
        .find(|m| m.name == "Ring of Spirits")
        .expect("an action raises it");
    assert!(matches!(raise.effect, Effect::Aura { which: 0, .. }));
    assert_eq!(raise.spell_slot_level, Some(3));
    assert!(raise.concentration, "it is held by concentration");

    // The aura itself is the save every enemy will meet, with the caster's
    // own DC read off the profile rather than baked into the plugin.
    let Effect::Save(save) = &c.lasting_auras[0].effect else {
        panic!("the aura is a saving throw");
    };
    assert_eq!(save.dc, 17);
    assert!(save.half_on_success);
}

/// A monster that has to start its turns beside the aura takes it every
/// round, not once - and the fight really applies it.
#[test]
fn a_raised_aura_lands_every_round_it_is_up() {
    let text = "creature: Dummy\nac: 1\nhp: 400\ninitiative: -20\n\
                action: Poke | hit +0 | 1d4 bludgeoning\n";
    let dummy = crucible_core::dsl::scenario::parse(text).expect("parses")[0].clone();

    let mut with_aura = caster();
    with_aura.team = 0;
    let mut foe = dummy.clone();
    foe.team = 1;

    let budget = Budget::default();
    let mut rng = Rng::new(20_260_923);
    let mut log = Some(Vec::new());
    run_teams(
        &mut rng,
        &[&with_aura, &foe],
        [Policy::Greedy; 2],
        8,
        budget,
        &mut log,
    );
    let lines = log.expect("a log was asked for");

    // The aura writes one line per enemy turn it washes over. The dummy is
    // slow and fat, so there are several of its turns to collect on.
    let rounds_collected = lines
        .iter()
        .filter(|l| l.contains("(aura)") && l.contains("Ring of Spirits"))
        .count();
    assert!(
        rounds_collected >= 3,
        "the aura should collect on every enemy turn it is up, got {rounds_collected}:
{}",
        lines.join(
            "
"
        )
    );
}

/// Putting up an aura that is already up is not a choice, so a ranking
/// playstyle never spends a second slot on it - as long as nothing knocks
/// it down in between.
#[test]
fn an_aura_already_up_is_never_raised_twice() {
    // A foe that cannot land a blow, so the caster is never forced into a
    // concentration check and the aura's only end is the fight's.
    let text = "creature: Harmless
ac: 12
hp: 200
initiative: -20
                action: Wait | stance dodging
";
    let foe = crucible_core::dsl::scenario::parse(text).expect("parses")[0].clone();

    let mut me = caster();
    me.team = 0;
    let mut foe = foe;
    foe.team = 1;

    let mut rng = Rng::new(7);
    let mut log = Some(Vec::new());
    let out = run_teams(
        &mut rng,
        &[&me, &foe],
        [Policy::Greedy; 2],
        8,
        Budget::default(),
        &mut log,
    );
    let lines = log.expect("a log was asked for");
    assert!(
        out.rounds > 1,
        "the fight has to last long enough to offer the choice"
    );
    assert_eq!(
        raises(&lines),
        1,
        "two 3rd-level slots, and it should still only ever pay one:
{}",
        lines.join(
            "
"
        )
    );
}

/// The other half: an aura is only as durable as the concentration holding
/// it, so a caster that gets hit hard enough loses it and has to pay again.
#[test]
fn losing_concentration_drops_the_aura_and_it_must_be_raised_again() {
    let text = "creature: Bruiser
ac: 12
hp: 300
initiative: 20
                action: Smash | hit +20 | 8d8 bludgeoning
";
    let foe = crucible_core::dsl::scenario::parse(text).expect("parses")[0].clone();

    let mut me = caster();
    me.team = 0;
    let mut foe = foe;
    foe.team = 1;

    // Stochastic by nature - a save can be made - so this asks only that it
    // happens at all across a handful of fights, not that it always does.
    let re_raised = (0..40).any(|seed| {
        let mut rng = Rng::new(seed);
        let mut log = Some(Vec::new());
        run_teams(
            &mut rng,
            &[&me, &foe],
            [Policy::Greedy; 2],
            8,
            Budget::default(),
            &mut log,
        );
        raises(&log.expect("a log was asked for")) > 1
    });
    assert!(
        re_raised,
        "a broken concentration should drop the aura and force it to be paid for again"
    );
}

/// The aura is held as a condition on its caster, which is what makes losing
/// concentration drop it without a second clock to keep in step.
#[test]
fn the_aura_is_held_as_a_condition_on_its_caster() {
    let c = caster();
    let Effect::Aura { which, .. } = c
        .actions
        .iter()
        .find(|m| m.name == "Ring of Spirits")
        .expect("an action raises it")
        .effect
    else {
        panic!("raising it is an Aura effect");
    };
    // The index the condition names is the index into the creature's own list.
    assert!(c.lasting_auras.get(which).is_some());
    let held = Condition::Aura(u8::try_from(which).expect("one aura"));
    // Like a boon, it carries nothing on its own - the whole effect is that
    // other creatures have to answer it.
    assert!(!held.incapacitated());
    assert!(!held.advantage_to_attackers());
}
