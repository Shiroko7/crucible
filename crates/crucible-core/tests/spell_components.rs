//! What a cast actually needs, and what taking one of those away stops.
//!
//! Silence does not stop "spells" - it stops speech. Before components, the
//! engine could only say the coarse thing, which is wrong for every spell
//! with no Verbal component. These pin the distinction: a silenced caster
//! still casts what it need not speak, and still cannot cast what it must.

use crucible_core::creature::{Components, Move, MoveKind};
use crucible_core::dsl::grammar::parse_move_external;
use crucible_core::dsl::scenario::parse;
use crucible_core::prob::Rng;
use crucible_core::rules::Condition;
use crucible_core::sim::{run_teams, Budget, Policy};

fn move_of(text: &str) -> Move {
    let owner = parse("creature: x\nhp: 1\n").expect("parses")[0].clone();
    parse_move_external(text, &owner).expect("the move parses")
}

#[test]
fn components_are_written_the_way_a_stat_block_writes_them() {
    assert_eq!(
        Components::parse("v"),
        Some(Components {
            verbal: true,
            somatic: false,
            material: false
        })
    );
    // Order, separators and spelling are all free.
    assert_eq!(Components::parse("vsm"), Components::parse("m, v, s"));
    assert_eq!(
        Components::parse("verbal, somatic"),
        Components::parse("vs")
    );
    assert_eq!(Components::parse("none"), Some(Components::default()));
    assert_eq!(Components::parse("loud"), None);

    assert_eq!(Components::parse("vsm").expect("parses").name(), "V, S, M");
    assert_eq!(
        Components::parse("none").expect("parses").name(),
        "no components"
    );
}

/// A spell that never says what it needs is read as speaking, which is what
/// the engine did before components existed - so no stat block changes
/// meaning by this feature landing.
#[test]
fn an_undeclared_spell_is_still_read_as_speaking() {
    let undeclared = move_of("Bolt | spell | ranged | hit +7 | 4d6 radiant");
    assert_eq!(undeclared.kind, MoveKind::Spell);
    assert!(undeclared.components.is_none());
    assert!(undeclared.needs_verbal());

    // And an ordinary attack never speaks, declared or not.
    let swing = move_of("Staff | hit +6 | 1d6+4 bludgeoning");
    assert!(!swing.needs_verbal());
}

#[test]
fn a_declared_spell_answers_from_its_own_components() {
    let silent = move_of("Shield | spell | components s | stance dodging");
    assert!(!silent.needs_verbal(), "somatic only: nothing to silence");

    let spoken = move_of("Word | spell | components v, m | save wis dc 15 | 2d6 psychic");
    assert!(spoken.needs_verbal());

    let nothing = move_of("Thought | spell | components none | save wis dc 15 | 2d6 psychic");
    assert!(!nothing.needs_verbal());
}

/// The point of all of it: in a real fight a silenced caster loses the spells
/// it has to speak and keeps the ones it does not.
#[test]
fn silence_stops_only_the_casts_that_speak() {
    assert!(Condition::Silenced.blocks_casting());

    // Two spells, identical but for whether they need speech. The silenced
    // caster should still be landing exactly one of them.
    let caster = parse(
        "creature: Caster\nac: 14\nhp: 200\ninitiative: 10\n\
         action: Spoken Bolt | spell | components v | ranged | hit +20 | 1d4 radiant\n\
         action: Silent Bolt | spell | components s | ranged | hit +20 | 1d4 force\n",
    )
    .expect("parses")[0]
        .clone();

    // An aura that silences whoever starts a turn in it, with no save.
    let silencer = parse(
        "creature: Silencer\nac: 30\nhp: 400\ninitiative: -20\nsaves: wis -20\n\
         aura: Hush | save wis dc 30 | on fail silenced for 10 rounds\n\
         action: Wait | stance dodging\n",
    )
    .expect("parses")[0]
        .clone();

    let mut me = caster;
    me.team = 0;
    let mut foe = silencer;
    foe.team = 1;

    let mut rng = Rng::new(3);
    let mut log = Some(Vec::new());
    run_teams(
        &mut rng,
        &[&me, &foe],
        [Policy::InOrder; 2],
        6,
        Budget::default(),
        &mut log,
    );
    let lines = log.expect("a log was asked for");
    let after_silence = lines
        .iter()
        .position(|l| l.contains("silenced"))
        .expect("the aura silences on the first turn");
    let later = &lines[after_silence + 1..];

    assert!(
        later.iter().any(|l| l.contains("Silent Bolt")),
        "a somatic-only spell should survive Silence:\n{}",
        lines.join("\n")
    );
    assert!(
        !later.iter().any(|l| l.contains("Spoken Bolt")),
        "a verbal spell should not:\n{}",
        lines.join("\n")
    );
}
