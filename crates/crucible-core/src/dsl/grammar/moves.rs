//! Move phrases: `|`-separated clauses describing what a move does and what
//! it costs, joined with `&&` for a move made of several effects.

use crate::creature::{
    AttackKind, Cost, Creature, Effect, Move, MoveKind, Rider, SaveEffect, Strike, Uses,
};
use crate::dsl::grammar::lex::{arg, parse_damage, parse_dice};
use crate::dsl::grammar::traits::parse_bonus_vs;
use crate::dsl::grammar::{count, number, parse_duration, DurationSpec};
use crate::rules::{Ability, Condition, DamageRoll, Duration, HealRoll, RollMode};

/// Parse a move string against a creature owner.
pub fn parse_move_external(value: &str, owner: &Creature) -> Result<Move, String> {
    parse_move(value, owner)
}

/// `Name | clause | ... && clause | ...`
///
/// Clauses are order-independent on purpose: a stat block does not put them in
/// a reliable order either.
pub(crate) fn parse_move(value: &str, owner: &Creature) -> Result<Move, String> {
    let mut segments = value.split("&&");
    let head = segments.next().unwrap_or("");
    let (name, mut built) = {
        let mut parts = head.split('|');
        let name = parts.next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            return Err("a move needs a name before the first `|`".into());
        }
        (name, parse_body(parts, owner)?)
    };

    let mut effects = vec![built
        .effect
        .take()
        .ok_or_else(|| format!("`{name}` does nothing - give it damage, a save, or a stance"))?];
    for segment in segments {
        let tail = parse_body(segment.split('|'), owner)?;
        effects.push(
            tail.effect
                .ok_or_else(|| format!("the `&&` part of `{name}` does nothing"))?,
        );
        built.riders.extend(tail.riders);
        built.kind = built.kind.or(tail.kind);
        built.spell_slot_level = built.spell_slot_level.or(tail.spell_slot_level);
        built.concentration |= tail.concentration;
    }

    Ok(Move {
        name,
        uses: built.uses,
        cost: built.cost,
        spell_slot_level: built.spell_slot_level,
        riders: built.riders,
        effect: if effects.len() == 1 {
            effects.pop().unwrap()
        } else {
            Effect::Sequence(effects)
        },
        concentration: built.concentration,
        kind: built.kind.unwrap_or_default(),
        before_action: false,
        bypasses_casting_restrictions: false,
    })
}

#[derive(Default)]
struct Body {
    uses: Uses,
    cost: Option<Cost>,
    riders: Vec<Rider>,
    effect: Option<Effect>,
    kind: Option<MoveKind>,
    spell_slot_level: Option<u32>,
    concentration: bool,
}

fn parse_body<'a>(
    clauses: impl Iterator<Item = &'a str>,
    owner: &Creature,
) -> Result<Body, String> {
    let mut to_hit: Option<i32> = None;
    let mut strikes: u32 = 1;
    let mut mode = RollMode::Normal;
    let mut save: Option<(Ability, i32)> = None;
    let mut half_on_success = false;
    let mut stance: Option<Condition> = None;
    let mut on_failure: Vec<(Condition, DurationSpec)> = Vec::new();
    let mut max_targets: Option<u32> = None;
    let mut damage: Vec<DamageRoll> = Vec::new();
    let mut heal: Option<HealRoll> = None;
    let mut attack = AttackKind::MELEE_WEAPON;
    let mut spell = false;
    let mut weapon_named = false;
    let mut out = Body::default();

    for clause in clauses {
        let clause = clause.trim();
        if clause.is_empty() {
            continue;
        }
        let words: Vec<&str> = clause.split_whitespace().collect();
        match words[0].to_ascii_lowercase().as_str() {
            "strikes" => strikes = count(arg(&words, 1, clause)?)?,
            "hit" => to_hit = Some(number(arg(&words, 1, clause)?)?),
            "recharge" => out.uses = Uses::Recharge(count(arg(&words, 1, clause)?)?),
            "uses" => out.uses = Uses::Limited(count(arg(&words, 1, clause)?)?),
            "advantage" => mode = RollMode::Advantage,
            "disadvantage" => mode = RollMode::Disadvantage,
            "half" => half_on_success = true,
            "targets" => max_targets = Some(count(arg(&words, 1, clause)?)?),
            "cost" => out.cost = Some(parse_cost(&words, 1, clause, owner)?),
            "slot" => {
                let level = count(arg(&words, 1, clause)?)?;
                if !(1..=crate::rules::SPELL_LEVELS).contains(&level) {
                    return Err(format!("`{clause}`: a spell slot is level 1 to 9"));
                }
                out.spell_slot_level = Some(level);
            }
            "concentration" => out.concentration = true,
            "ranged" => attack.ranged = true,
            "melee" => attack.ranged = false,
            "finesse" => attack.finesse = true,
            "weapon" => weapon_named = true,
            "spell" => {
                spell = true;
                out.kind = Some(MoveKind::Spell);
            }
            "item" => out.kind = Some(MoveKind::MagicItem),
            "object" => out.kind = Some(MoveKind::ObjectUse),
            "heal" => {
                let (dice, sides, bonus) = parse_dice(arg(&words, 1, clause)?)?;
                heal = Some(HealRoll::new(dice, sides, bonus));
            }
            "bonus" => out.riders.push(parse_bonus_vs(&words, clause)?),
            "stance" => {
                let name = arg(&words, 1, clause)?;
                stance = Some(
                    Condition::parse(name).ok_or_else(|| format!("unknown condition `{name}`"))?,
                );
            }
            "on" if arg(&words, 1, clause)?.eq_ignore_ascii_case("fail") => {
                on_failure.push(parse_on_fail(&words, clause)?);
            }
            "on" => out.riders.push(parse_on_hit(&words, clause, owner)?),
            "save" => {
                let ability = Ability::parse(arg(&words, 1, clause)?)
                    .ok_or_else(|| format!("unknown ability in `{clause}`"))?;
                if !arg(&words, 2, clause)?.eq_ignore_ascii_case("dc") {
                    return Err(format!("expected `save <ability> dc <n>`, got `{clause}`"));
                }
                save = Some((ability, number(arg(&words, 3, clause)?)?));
            }
            _ => damage.extend(parse_damage(clause)?),
        }
    }

    // A spell's strike is a spell attack, unless the move also names the
    // weapon it is made with (True Strike's shape).
    attack.spell = spell;
    attack.weapon = !spell || weapon_named;

    let on_failure = on_failure
        .into_iter()
        .map(|(condition, spec)| Ok((condition, spec.resolve(save, "on fail")?)))
        .collect::<Result<Vec<_>, String>>()?;

    out.effect = if let Some(condition) = stance {
        Some(Effect::Stance { condition })
    } else if let Some(roll) = heal {
        Some(Effect::Heal(roll))
    } else if let Some((ability, dc)) = save {
        Some(Effect::Save(SaveEffect {
            ability,
            dc,
            damage,
            half_on_success,
            on_failure,
            max_targets,
            // The DSL has no clause for it yet; a scenario that needs a
            // type-restricted save waits on that syntax, not on the engine.
            requires_type: None,
        }))
    } else if !damage.is_empty() {
        let to_hit = to_hit.ok_or("a damaging move needs a `hit +N` clause or a `save`")?;
        Some(Effect::Strikes {
            strike: Strike {
                to_hit,
                mode,
                damage,
                kind: attack,
            },
            count: strikes,
        })
    } else {
        None
    };
    Ok(out)
}

/// `on fail prone [duration]`, the condition half of a saving throw. The
/// duration may be `until save`, resolved once the move's own save is known.
fn parse_on_fail(words: &[&str], clause: &str) -> Result<(Condition, DurationSpec), String> {
    let name = arg(words, 2, clause)?;
    let condition = Condition::parse(name).ok_or_else(|| format!("unknown condition `{name}`"))?;
    let (duration, used) = parse_duration(words, 3, clause)?;
    if 3 + used != words.len() {
        return Err(format!("unexpected words at the end of `{clause}`"));
    }
    Ok((condition, duration))
}

/// `on hit save con dc 16 stunned [once] [cost focus 1] [duration]`
fn parse_on_hit(words: &[&str], clause: &str, owner: &Creature) -> Result<Rider, String> {
    if !arg(words, 1, clause)?.eq_ignore_ascii_case("hit") {
        return Err(format!("expected `on hit ...`, got `{clause}`"));
    }
    if !arg(words, 2, clause)?.eq_ignore_ascii_case("save") {
        return Err(format!("expected `on hit save ...`, got `{clause}`"));
    }
    let ability = Ability::parse(arg(words, 3, clause)?)
        .ok_or_else(|| format!("unknown ability in `{clause}`"))?;
    if !arg(words, 4, clause)?.eq_ignore_ascii_case("dc") {
        return Err(format!("expected `save <ability> dc <n>` in `{clause}`"));
    }
    let dc = number(arg(words, 5, clause)?)?;
    let name = arg(words, 6, clause)?;
    let condition = Condition::parse(name).ok_or_else(|| format!("unknown condition `{name}`"))?;

    let mut once_per_turn = false;
    let mut cost = None;
    let mut duration = Duration::ApplierTurn;
    let mut i = 7;
    while i < words.len() {
        match words[i].to_ascii_lowercase().as_str() {
            "once" => once_per_turn = true,
            "cost" => {
                cost = Some(parse_cost(words, i + 1, clause, owner)?);
                i += 2;
            }
            "until" | "for" => {
                let (spec, used) = parse_duration(words, i, clause)?;
                duration = spec.resolve(Some((ability, dc)), clause)?;
                i += used;
                continue;
            }
            // Words like "per" and "turn" in "once per turn" read as noise.
            "per" | "turn" => {}
            other => return Err(format!("unexpected `{other}` in `{clause}`")),
        }
        i += 1;
    }

    Ok(Rider::SaveOrCondition {
        ability,
        dc,
        condition,
        duration,
        cost,
        once_per_turn,
    })
}

fn parse_cost(words: &[&str], at: usize, clause: &str, owner: &Creature) -> Result<Cost, String> {
    let name = arg(words, at, clause)?;
    let amount = count(arg(words, at + 1, clause)?)?;
    let resource = owner.resource_index(name).ok_or_else(|| {
        format!("`{name}` is not a declared resource - add a `resource: {name} N` line above")
    })?;
    Ok(Cost { resource, amount })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::scenario::parse;
    use crate::rules::DamageKind;

    /// The case the multi-component damage model exists for.
    #[test]
    fn a_strike_can_deal_two_damage_types_at_once() {
        let text = "creature: x\nhp: 1\naction: Rend | hit +14 | 1d10+8 slashing, 2d4 fire\n";
        let c = &parse(text).unwrap()[0];
        let Effect::Strikes { strike, .. } = &c.actions[0].effect else {
            panic!("expected strikes")
        };
        assert_eq!(
            strike.damage,
            vec![
                DamageRoll::new(1, 10, 8, DamageKind::Slashing),
                DamageRoll::new(2, 4, 0, DamageKind::Fire),
            ]
        );
    }

    #[test]
    fn a_breath_weapon_parses_as_a_save() {
        let text = "creature: x\nhp: 1\naction: Fire Breath | recharge 5 | save dex dc 21 | 17d6 fire | half on success\n";
        let m = &parse(text).unwrap()[0].actions[0];
        assert_eq!(m.uses, Uses::Recharge(5));
        match &m.effect {
            Effect::Save(save) => {
                assert_eq!((save.ability, save.dc), (Ability::Dex, 21));
                assert!(save.half_on_success);
                assert_eq!(
                    save.damage,
                    vec![DamageRoll::new(17, 6, 0, DamageKind::Fire)]
                );
            }
            other => panic!("expected a save, got {other:?}"),
        }
    }

    /// A monk's turn, which is what forced resource pools and on-hit riders to
    /// exist. Also checks that a line ending in `|` continues.
    #[test]
    fn a_monk_turn_parses_including_its_pool_and_riders() {
        let text = "
creature: Gio
hp: 69
ac: 20
resource: focus 8
action: Staff x2 | strikes 2 | hit +9 | 1d8+6 bludgeoning |
        on hit save con dc 16 stunned once per turn cost focus 1
bonus: Flurry of Blows | cost focus 1 | strikes 2 | hit +7 | 1d8+4 force
bonus: Patient Defense | cost focus 1 | stance dodging
";
        let c = &parse(text).unwrap()[0];
        assert_eq!(c.resources[0].name, "focus");
        assert_eq!(c.resources[0].max, 8);

        let staff = &c.actions[0];
        assert!(staff.cost.is_none(), "the staff itself is free");
        match &staff.riders[0] {
            Rider::SaveOrCondition {
                ability,
                dc,
                condition,
                duration,
                cost,
                once_per_turn,
            } => {
                assert_eq!((*ability, *dc), (Ability::Con, 16));
                assert_eq!(*condition, Condition::Stunned);
                assert_eq!(*duration, Duration::ApplierTurn);
                assert_eq!(
                    *cost,
                    Some(Cost {
                        resource: 0,
                        amount: 1
                    })
                );
                assert!(*once_per_turn);
            }
            other => panic!("expected a save-or-condition rider, got {other:?}"),
        }

        assert_eq!(
            c.bonus_actions[0].cost,
            Some(Cost {
                resource: 0,
                amount: 1
            })
        );
        assert!(!c.bonus_actions[0].is_free());
        assert_eq!(
            c.bonus_actions[1].effect,
            Effect::Stance {
                condition: Condition::Dodging
            }
        );
    }

    /// `&&` is how a Multiattack that is not all the same attack gets written.
    #[test]
    fn a_move_can_be_built_from_several_effects() {
        let text = "
creature: x
hp: 10
action: Breath and a swing | uses 3 | save dex dc 16 | 2d8 cold | half on success
                          && strikes 1 | hit +9 | 1d8+6 bludgeoning
";
        let m = &parse(text).unwrap()[0].actions[0];
        assert_eq!(m.uses, Uses::Limited(3));
        let Effect::Sequence(parts) = &m.effect else {
            panic!("expected a sequence, got {:?}", m.effect)
        };
        assert_eq!(parts.len(), 2);
        assert!(matches!(parts[0], Effect::Save(_)));
        assert!(matches!(parts[1], Effect::Strikes { count: 1, .. }));
    }

    /// What kind of attack a strike is, what casting it costs, and whether
    /// it concentrates - all declared on the move.
    #[test]
    fn a_move_declares_its_attack_kind_spell_slot_and_concentration() {
        let text = "
creature: Caster
hp: 20
action: Longbow | ranged | hit +8 | 1d8+4 piercing | bonus 3d6 piercing vs dragon
action: Rapier | finesse | hit +7 | 1d8+4 piercing
action: Guiding Bolt | spell | slot 1 | ranged | hit +7 | 4d6 radiant
action: True Strike | spell | weapon | ranged | hit +11 | 1d8+5 piercing, 2d6 radiant
action: Hold | spell | slot 2 | concentration | save wis dc 15 | on fail paralyzed until save
bonus: Wand | item | hit +7 | 1d6 force
";
        let c = &parse(text).unwrap()[0];
        let kind = |i: usize| match &c.actions[i].effect {
            Effect::Strikes { strike, .. } => strike.kind,
            other => panic!("expected strikes, got {other:?}"),
        };
        assert_eq!(kind(0), AttackKind::RANGED_WEAPON);
        assert!(matches!(
            c.actions[0].riders[..],
            [Rider::BonusDamageVsCreatureType { dice_count: 3, .. }]
        ));
        assert!(kind(1).finesse && kind(1).weapon && !kind(1).ranged);
        assert_eq!(kind(2), AttackKind::RANGED_SPELL);
        assert_eq!(c.actions[2].kind, MoveKind::Spell);
        assert_eq!(c.actions[2].spell_slot_level, Some(1));
        assert!(kind(3).weapon && kind(3).spell && kind(3).ranged);

        let hold = &c.actions[4];
        assert!(hold.concentration);
        assert_eq!(hold.spell_slot_level, Some(2));
        let Effect::Save(save) = &hold.effect else {
            panic!("expected a save");
        };
        assert_eq!(
            save.on_failure,
            vec![(
                Condition::Paralyzed,
                Duration::SaveEndTurn {
                    ability: Ability::Wis,
                    dc: 15
                }
            )]
        );
        assert_eq!(c.bonus_actions[0].kind, MoveKind::MagicItem);
    }

    /// A potion is a heal from a pool, and an object.
    #[test]
    fn a_heal_move_parses() {
        let text = "creature: x\nhp: 20\nresource: potions 4\nbonus: Potion | object | cost potions 1 | heal 2d4+2\n";
        let c = &parse(text).unwrap()[0];
        let potion = &c.bonus_actions[0];
        assert_eq!(potion.kind, MoveKind::ObjectUse);
        assert_eq!(potion.effect, Effect::Heal(HealRoll::new(2, 4, 2)));
        assert!(potion.cost.is_some());
    }
}
