//! Move phrases: `|`-separated clauses describing what a move does and what
//! it costs, joined with `&&` for a move made of several effects.

use crate::creature::{
    AttackKind, AttackTrigger, Cost, Creature, Displace, Effect, Move, MoveKind, Reach, Reaction,
    ReactionTrigger, Requirement, Rider, SaveEffect, Strike, Uses,
};
use crate::dsl::grammar::lex::{arg, parse_damage, parse_dice};
use crate::dsl::grammar::traits::parse_bonus_vs;
use crate::dsl::grammar::{count, number, parse_duration, DurationSpec};
use crate::rules::{Ability, Condition, DamageRoll, Duration, HealRoll, RollMode, Size};

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

    let head_effect = built
        .effect
        .take()
        .ok_or_else(|| format!("`{name}` does nothing - give it damage, a save, or a stance"))?;
    let mut parts = vec![(head_effect, std::mem::take(&mut built.riders), built.reach)];
    for segment in segments {
        let tail = parse_body(segment.split('|'), owner)?;
        parts.push((
            tail.effect
                .ok_or_else(|| format!("the `&&` part of `{name}` does nothing"))?,
            tail.riders,
            tail.reach,
        ));
        built.kind = built.kind.or(tail.kind);
        built.spell_slot_level = built.spell_slot_level.or(tail.spell_slot_level);
        built.concentration |= tail.concentration;
        built.components = built.components.or(tail.components);
        built.legendary_cost = built.legendary_cost.or(tail.legendary_cost);
        built.worth = built.worth.or(tail.worth);
        built.range = built.range.or(tail.range);
        built.area = built.area.or(tail.area);
    }

    // One part keeps its riders and reach on the move. Several keep each
    // part's on that part: a bite's swallow is not the slam's knockdown, and
    // the slam reaches further than the bite.
    let (effect, riders, reach) = if parts.len() == 1 {
        parts.pop().unwrap()
    } else {
        let parts = parts
            .into_iter()
            .map(|(effect, riders, reach)| {
                if riders.is_empty() && reach == Reach::Any {
                    effect
                } else {
                    Effect::Part {
                        effect: Box::new(effect),
                        riders,
                        reach,
                    }
                }
            })
            .collect();
        (Effect::Sequence(parts), Vec::new(), Reach::Any)
    };

    Ok(Move {
        name,
        uses: built.uses,
        cost: built.cost,
        spell_slot_level: built.spell_slot_level,
        riders,
        effect,
        concentration: built.concentration,
        kind: built.kind.unwrap_or_default(),
        before_action: false,
        bypasses_casting_restrictions: false,
        components: built.components,
        legendary_cost: built.legendary_cost.unwrap_or(1),
        reach,
        range: built.range,
        area: built.area,
        requires: built.requires,
        spends: None,
        worth: built.worth.unwrap_or(0),
    })
}

/// Parse a reaction string against a creature owner.
pub fn parse_reaction_external(value: &str, owner: &Creature) -> Result<Reaction, String> {
    parse_reaction(value, owner)
}

/// A move with a `when <trigger>` clause anywhere after its name:
/// `Snap | when enemy pulled | hit +9 | 2d10+5 piercing`.
///
/// - `when enemy <condition>`: this creature has just given an enemy that
///   condition ([`ReactionTrigger::EnemyGains`]);
/// - `when breached`: damage has just broken through its damage threshold
///   ([`ReactionTrigger::Breached`]);
/// - `when hit`, `when hit in melee`, `when hit by a ranged weapon`: an
///   attack of that kind has just landed on it, and the reaction answers
///   whoever landed it ([`ReactionTrigger::Hit`]).
pub(crate) fn parse_reaction(value: &str, owner: &Creature) -> Result<Reaction, String> {
    let mut trigger = None;
    let mut kept: Vec<&str> = Vec::new();
    for (i, clause) in value.split('|').enumerate() {
        let words: Vec<&str> = clause.split_whitespace().collect();
        let is_when = i > 0
            && words
                .first()
                .is_some_and(|w| w.eq_ignore_ascii_case("when"));
        if !is_when {
            kept.push(clause);
            continue;
        }
        if trigger.is_some() {
            return Err(format!("`{}` has two `when` clauses", value.trim()));
        }
        trigger = Some(match words[1..] {
            [w] if w.eq_ignore_ascii_case("breached") => ReactionTrigger::Breached,
            [w] if w.eq_ignore_ascii_case("damaged") => ReactionTrigger::Damaged,
            [w, casts] if w.eq_ignore_ascii_case("enemy") && casts.eq_ignore_ascii_case("casts") => {
                ReactionTrigger::Cast { aimed: false }
            }
            [w, ..] if w.eq_ignore_ascii_case("targeted") => ReactionTrigger::Cast { aimed: true },
            [w, name] if w.eq_ignore_ascii_case("enemy") => ReactionTrigger::EnemyGains(
                Condition::parse(name).ok_or_else(|| format!("unknown condition `{name}`"))?,
            ),
            [w, ref rest @ ..] if w.eq_ignore_ascii_case("hit") => {
                ReactionTrigger::Hit(parse_attack_trigger(rest, clause)?)
            }
            _ => {
                return Err(format!(
                    "expected `when enemy <condition>`, `when breached` or `when hit [in melee]`,                      got `{}`",
                    clause.trim()
                ))
            }
        });
    }
    let trigger = trigger.ok_or_else(|| {
        format!(
            "reaction `{}` needs a `when ...` clause saying what sets it off",
            value.split('|').next().unwrap_or("").trim()
        )
    })?;
    Ok(Reaction {
        trigger,
        action: parse_move(&kept.join("|"), owner)?,
    })
}

/// Which attacks a `when hit` reaction answers: nothing at all for any of
/// them, `in melee` for a blade, `by a ranged weapon` for an arrow.
///
/// The same three an [`crate::creature::AttackTrigger`] names, since it is
/// the same question a raised shield asks - see [`ReactionTrigger::Hit`].
fn parse_attack_trigger(words: &[&str], clause: &str) -> Result<AttackTrigger, String> {
    let phrase = words
        .iter()
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| w != "a" && w != "an" && w != "attack" && w != "attacks")
        .collect::<Vec<_>>()
        .join(" ");
    Ok(match phrase.as_str() {
        "" | "by any" | "any" => AttackTrigger::AnyAttack,
        "in melee" | "by melee" | "melee" => AttackTrigger::MeleeAttack,
        "by ranged weapon" | "ranged weapon" | "by ranged" | "ranged" => {
            AttackTrigger::RangedWeaponAttack
        }
        other => {
            return Err(format!(
                "`hit {other}` is not an attack trigger in `{}` - use `when hit`,                  `when hit in melee` or `when hit by a ranged weapon`",
                clause.trim()
            ))
        }
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
    legendary_cost: Option<u32>,
    worth: Option<i32>,
    reach: Reach,
    range: Option<u32>,
    area: Option<u32>,
    components: Option<crate::creature::Components>,
    requires: Option<Requirement>,
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
    let mut temp_hp: Option<HealRoll> = None;
    let mut on_failure: Vec<(Condition, DurationSpec)> = Vec::new();
    let mut max_targets: Option<u32> = None;
    let mut damage: Vec<DamageRoll> = Vec::new();
    let mut heal: Option<HealRoll> = None;
    let mut attack = AttackKind::MELEE_WEAPON;
    let mut spell = false;
    let mut weapon_named = false;
    let mut weapon_label: Option<&str> = None;
    let mut to_swallowed = false;
    let mut autohit = false;
    let mut counter = false;
    let mut teleport: Option<u32> = None;
    let mut requires_type: Option<String> = None;
    let mut hide: Option<i32> = None;
    let mut thrown: Option<(u32, bool)> = None;
    let mut blast: Option<(u32, Vec<DamageRoll>)> = None;
    let mut on_success: Vec<DamageRoll> = Vec::new();
    let mut out = Body::default();

    for clause in clauses {
        let clause = clause.trim();
        if clause.is_empty() {
            continue;
        }
        let words: Vec<&str> = clause.split_whitespace().collect();
        match words[0].to_ascii_lowercase().as_str() {
            "autohit" => autohit = true,
            "counter" => counter = true,
            "worth" => out.worth = Some(number(arg(&words, 1, clause)?)?),
            "teleport" => teleport = Some(count(arg(&words, 1, clause)?)?),
            // `one at a time`: what this lands is lifted from whoever else
            // was under it.
            "one" => {
                if !clause.eq_ignore_ascii_case("one at a time") {
                    return Err(format!("expected `one at a time`, got `{clause}`"));
                }
                out.riders.push(Rider::OneAtATime);
            }
            // `escape 10d10 psychic`: whoever it holds can break out by
            // taking that.
            "escape" => {
                let rolls = parse_damage(clause.trim()[words[0].len()..].trim())?;
                let [roll] = rolls[..] else {
                    return Err(format!("expected `escape NdM <type>`, got `{clause}`"));
                };
                out.riders.push(Rider::EscapeFor { roll });
            }
            // `on kill raise <name>`: one of its user's `minions`.
            "on" if arg(&words, 1, clause)?.eq_ignore_ascii_case("kill") => {
                if !arg(&words, 2, clause)?.eq_ignore_ascii_case("raise") {
                    return Err(format!("expected `on kill raise <minion>`, got `{clause}`"));
                }
                let name = words[3..].join(" ");
                let which = owner
                    .summons
                    .iter()
                    .position(|s| s.name.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| format!("`{name}` is not one of this creature's minions"))?;
                out.riders.push(Rider::RaiseSlain { which });
            }
            // `strikes each`: one swing at every creature in reach.
            "strikes" if arg(&words, 1, clause)?.eq_ignore_ascii_case("each") => {
                out.riders.push(Rider::EachInReach);
            }
            "strikes" => strikes = count(arg(&words, 1, clause)?)?,
            // `magical`: these attacks get through a hide that only resists
            // steel.
            "magical" | "magic" => out.riders.push(Rider::MagicWeapons),
            // `no opportunity attacks`: the turn it is taken, its user
            // moves its speed again and nothing gets a swing at it.
            "no" => {
                if !clause.eq_ignore_ascii_case("no opportunity attacks") {
                    return Err(format!("expected `no opportunity attacks`, got `{clause}`"));
                }
                out.riders.push(Rider::NeverProvokes);
            }
            // `hide +7`: a Stealth check to go unseen.
            "hide" => hide = Some(number(arg(&words, 1, clause)?)?),
            // `in dim light`: only while its user stands in dim light or
            // darkness.
            "in" => {
                if !matches!(
                    clause.to_ascii_lowercase().as_str(),
                    "in dim light" | "in darkness" | "in dim light or darkness"
                ) {
                    return Err(format!("expected `in dim light`, got `{clause}`"));
                }
                out.requires = Some(Requirement::Shadow);
            }
            // `all hit 2d8 psychic [save con dc 17 stunned ...]`: what every
            // strike of the move landing on one creature adds.
            "all" => {
                if !arg(&words, 1, clause)?.eq_ignore_ascii_case("hit") {
                    return Err(format!(
                        "expected `all hit NdM <type> [save ...]`, got `{clause}`"
                    ));
                }
                let at = words
                    .iter()
                    .position(|w| w.eq_ignore_ascii_case("save"))
                    .unwrap_or(words.len());
                let damage = match at > 2 {
                    true => parse_damage(&words[2..at].join(" "))?,
                    false => Vec::new(),
                };
                let then = match at < words.len() {
                    true => {
                        let as_on_hit: Vec<&str> = ["on", "hit"]
                            .into_iter()
                            .chain(words[at..].iter().copied())
                            .collect();
                        Some(Box::new(parse_on_hit(&as_on_hit, clause, owner)?))
                    }
                    false => None,
                };
                if damage.is_empty() && then.is_none() {
                    return Err(format!("`{clause}` adds nothing"));
                }
                out.riders.push(Rider::AllHit { damage, then });
            }
            // `blast 10 6d12 psychic`: what everything within that many
            // feet of where a thrown creature stood takes.
            "blast" => {
                let feet = count(arg(&words, 1, clause)?)?;
                blast = Some((feet, parse_damage(&words[2..].join(" "))?));
            }
            // `on fail thrown 60 [away]`: whoever fails is put somewhere
            // else within that many feet.
            "on" if arg(&words, 1, clause)?.eq_ignore_ascii_case("fail")
                && arg(&words, 2, clause)?.eq_ignore_ascii_case("thrown") =>
            {
                let feet = count(arg(&words, 3, clause)?)?;
                let away = match words.get(4) {
                    None => false,
                    Some(w) if w.eq_ignore_ascii_case("away") && words.len() == 5 => true,
                    Some(_) => {
                        return Err(format!(
                            "expected `on fail thrown <feet> [away]`, got `{clause}`"
                        ))
                    }
                };
                thrown = Some((feet, away));
            }
            // `on success 3d12 psychic`: what saving against it still costs.
            "on" if arg(&words, 1, clause)?.eq_ignore_ascii_case("success") => {
                on_success = parse_damage(&words[2..].join(" "))?;
            }
            "hit" => to_hit = Some(number(arg(&words, 1, clause)?)?),
            "recharge" => out.uses = Uses::Recharge(count(arg(&words, 1, clause)?)?),
            "uses" => out.uses = Uses::Limited(count(arg(&words, 1, clause)?)?),
            "advantage" => mode = RollMode::Advantage,
            "disadvantage" => mode = RollMode::Disadvantage,
            // `half on miss`: an attack that still lands half its damage.
            "half"
                if clause.eq_ignore_ascii_case("half on miss")
                    || clause.eq_ignore_ascii_case("half on a miss") =>
            {
                out.riders.push(Rider::HalfOnMiss);
            }
            "half" => half_on_success = true,
            // `long 60`: an attack can be made that far off, past its
            // `range`, at disadvantage.
            "long" => out.riders.push(Rider::LongRange {
                feet: count(arg(&words, 1, clause)?)?,
            }),
            // `chains 30`: each creature that fails the save passes it on
            // to the nearest enemy within that many feet.
            "chains" | "chain" => out.riders.push(Rider::Chains {
                feet: count(arg(&words, 1, clause)?)?,
            }),
            // `line 60`: its user rides the line to a free space that far
            // off, catching everything on the way.
            "line" => out.riders.push(Rider::RideLine {
                feet: count(arg(&words, 1, clause)?)?,
            }),
            // `spares target`: a burst around its target that leaves the
            // target itself out.
            "spares" => {
                if !clause.eq_ignore_ascii_case("spares target") {
                    return Err(format!("expected `spares target`, got `{clause}`"));
                }
                out.riders.push(Rider::SparesTarget);
            }
            // `takes bonus action`: an action that uses up the bonus action
            // as well.
            "takes" => {
                if !clause.eq_ignore_ascii_case("takes bonus action") {
                    return Err(format!("expected `takes bonus action`, got `{clause}`"));
                }
                out.riders.push(Rider::TakesBonusAction);
            }
            // `placed 120`, on an aura that is raised: put down around
            // whoever it is aimed at, up to that far away, and left there.
            "placed" => out.riders.push(Rider::Anchored {
                feet: count(arg(&words, 1, clause)?)?,
            }),
            // `difficult terrain`, on an aura that is placed: whoever starts
            // a turn in it covers half its speed.
            "difficult" => {
                if !clause.eq_ignore_ascii_case("difficult terrain") {
                    return Err(format!("expected `difficult terrain`, got `{clause}`"));
                }
                out.riders.push(Rider::Hinders);
            }
            // `also cost divine 3`: a second pool, beside its `cost`.
            "also" => {
                if !arg(&words, 1, clause)?.eq_ignore_ascii_case("cost") {
                    return Err(format!("expected `also cost <pool> <n>`, got `{clause}`"));
                }
                out.riders
                    .push(Rider::AlsoCosts(parse_cost(&words, 2, clause, owner)?));
            }
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
            // `only undead` - a save nothing else is even caught by, which
            // is Hold Person's "you can only target a humanoid" and Turn
            // Undead's "each Undead of your choice".
            "only" => {
                let name = arg(&words, 1, clause)?;
                if crate::rules::CreatureType::parse(name).is_none() {
                    return Err(format!("unknown creature type `{name}` in `{clause}`"));
                }
                requires_type = Some(name.to_string());
            }
            // `components v`, `components verbal, somatic`, `components none`
            // - which of the three a cast needs, so Silence can stop the ones
            // that speak rather than every spell. See `creature::Components`.
            "components" => {
                let rest = clause
                    .trim()
                    .strip_prefix(words[0])
                    .unwrap_or_default()
                    .trim();
                out.components =
                    Some(crate::creature::Components::parse(rest).ok_or_else(|| {
                        format!("`{rest}` is not a component list in `{clause}`")
                    })?);
            }
            "points" => {
                let n = count(arg(&words, 1, clause)?)?;
                if n == 0 {
                    return Err(format!("`{clause}`: a legendary action takes at least one"));
                }
                out.legendary_cost = Some(n);
            }
            "swallowed" => to_swallowed = true,
            "reach" => {
                let word = arg(&words, 1, clause)?;
                out.reach = Reach::parse(word).ok_or_else(|| {
                    format!("`{word}` is not a reach - use mouth, near, front or any")
                })?;
            }
            // `range 150`, `area 20` - feet on the grid: how far the move
            // reaches (a 10-foot reach is `range 10`), and how close to its
            // target an enemy has to be for an area to catch it too.
            "range" => out.range = Some(count(arg(&words, 1, clause)?)?),
            "area" => out.area = Some(count(arg(&words, 1, clause)?)?),
            "ranged" => attack.ranged = true,
            "melee" => attack.ranged = false,
            "finesse" => attack.finesse = true,
            // `weapon`, or `weapon <name>` to say which one this swing is
            // made with - what an enchantment laid on a single blade rides.
            "weapon" => {
                weapon_named = true;
                weapon_label = words.get(1).copied();
            }
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
            // `temp 1d10+2` - temporary hit points for the user, which is a
            // ward rather than a heal: see `Effect::TempHp`.
            "temp" => {
                let (dice, sides, bonus) = parse_dice(arg(&words, 1, clause)?)?;
                temp_hp = Some(HealRoll::new(dice, sides, bonus));
            }
            // `bonus 2d6 psychic with advantage`: on a hit whose roll had it.
            "bonus"
                if words.len() == 5
                    && words[3].eq_ignore_ascii_case("with")
                    && words[4].eq_ignore_ascii_case("advantage") =>
            {
                let rolls = parse_damage(&words[1..3].join(" "))?;
                let [roll] = rolls[..] else {
                    return Err(format!(
                        "expected `bonus NdM <type> with advantage`, got `{clause}`"
                    ));
                };
                out.riders.push(Rider::BonusDamageWithAdvantage { roll });
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

    if requires_type.is_some() && save.is_none() {
        return Err(
            "`only <creature type>` restricts a saving throw: give the move a `save`".into(),
        );
    }

    let on_failure = on_failure
        .into_iter()
        .map(|(condition, spec)| Ok((condition, spec.resolve(save, "on fail")?)))
        .collect::<Result<Vec<_>, String>>()?;

    if thrown.is_none() && (blast.is_some() || !on_success.is_empty()) {
        return Err(
            "`blast` and `on success` belong to a save that throws: add `on fail thrown <feet>`"
                .into(),
        );
    }

    out.effect = if let Some(bonus) = hide {
        Some(Effect::Hide { bonus })
    } else if let Some((feet, away)) = thrown {
        let (ability, dc) = save.ok_or("`on fail thrown` needs a `save`")?;
        if !damage.is_empty() || !on_failure.is_empty() {
            return Err(
                "a save that throws says what it does in `blast` and `on success`, and nothing else"
                    .into(),
            );
        }
        let (radius, blast) = blast.unwrap_or_default();
        Some(Effect::Displace(Displace {
            ability,
            dc,
            feet,
            away,
            radius,
            blast,
            on_success,
        }))
    } else if let Some(condition) = stance {
        Some(Effect::Stance { condition })
    } else if let Some(roll) = temp_hp {
        Some(Effect::TempHp(roll))
    } else if let Some(roll) = heal {
        Some(Effect::Heal(roll))
    } else if to_swallowed {
        if damage.is_empty() || save.is_some() || to_hit.is_some() {
            return Err(
                "`swallowed` damage lands without a roll: give it damage and no `hit` or `save`"
                    .into(),
            );
        }
        Some(Effect::HarmSwallowed { damage })
    } else if autohit {
        if damage.is_empty() || save.is_some() || to_hit.is_some() {
            return Err(
                "`autohit` damage lands without a roll: give it damage and no `hit` or `save`"
                    .into(),
            );
        }
        Some(Effect::AutoHit { damage })
    } else if counter && save.is_none() {
        Some(Effect::Counter)
    } else if let Some(feet) = teleport {
        Some(Effect::Teleport { feet })
    } else if let Some((ability, dc)) = save {
        Some(Effect::Save(SaveEffect {
            ability,
            dc,
            damage,
            half_on_success,
            on_failure,
            max_targets,
            requires_type,
        }))
    } else if !damage.is_empty() {
        let to_hit =
            to_hit.ok_or("a damaging move needs a `hit +N` clause, a `save`, or `autohit`")?;
        Some(Effect::Strikes {
            strike: Strike {
                to_hit,
                mode,
                damage,
                kind: attack,
                weapon: weapon_label.map(str::to_string),
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

/// `on hit save con dc 16 stunned [and <condition>]... [once] [cost focus 1]
/// [duration]`; `on hit swallow <size>` - swallow a target of that size or
/// smaller; or `on hit <condition> [duration]` - a condition that lands with
/// the hit and offers no save at all, which is what a weapon mastery like Vex
/// does.
fn parse_on_hit(words: &[&str], clause: &str, owner: &Creature) -> Result<Rider, String> {
    if !arg(words, 1, clause)?.eq_ignore_ascii_case("hit") {
        return Err(format!("expected `on hit ...`, got `{clause}`"));
    }
    if arg(words, 2, clause)?.eq_ignore_ascii_case("swallow") {
        let size = arg(words, 3, clause)?;
        if words.len() > 4 {
            return Err(format!("unexpected words at the end of `{clause}`"));
        }
        return Ok(Rider::Swallow {
            max_size: Size::parse(size)
                .ok_or_else(|| format!("unknown size `{size}` in `{clause}`"))?,
        });
    }
    // A bare condition: no save, no cost, no budget - `on hit vexed until
    // end`. Read before the `save` form so a condition named `save` could
    // never be mistaken for one, and after `swallow`, which is its own shape.
    if !arg(words, 2, clause)?.eq_ignore_ascii_case("save") {
        let name = arg(words, 2, clause)?;
        let condition =
            Condition::parse(name).ok_or_else(|| format!("unknown condition `{name}`"))?;
        let (spec, used) = parse_duration(words, 3, clause)?;
        if 3 + used != words.len() {
            return Err(format!("unexpected words at the end of `{clause}`"));
        }
        return Ok(Rider::ConditionOnHit {
            condition,
            duration: spec.resolve(None, clause)?,
        });
    }
    let ability = Ability::parse(arg(words, 3, clause)?)
        .ok_or_else(|| format!("unknown ability in `{clause}`"))?;
    if !arg(words, 4, clause)?.eq_ignore_ascii_case("dc") {
        return Err(format!("expected `save <ability> dc <n>` in `{clause}`"));
    }
    let dc = number(arg(words, 5, clause)?)?;
    let condition_at = |at: usize| -> Result<Condition, String> {
        let name = arg(words, at, clause)?;
        Condition::parse(name).ok_or_else(|| format!("unknown condition `{name}`"))
    };
    let mut conditions = vec![condition_at(6)?];

    let mut once_per_turn = false;
    let mut cost = None;
    let mut duration = Duration::ApplierTurn;
    let mut i = 7;
    while i < words.len() {
        match words[i].to_ascii_lowercase().as_str() {
            // `prone and pushed`: more off the same failed save.
            "and" => {
                conditions.push(condition_at(i + 1)?);
                i += 1;
            }
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
        conditions,
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
    use crate::rules::{DamageKind, DamageRoll};

    /// The clauses a creature that bends space and hunts from the dark is
    /// written in.
    #[test]
    fn the_clauses_for_warped_space_and_ambush_parse() {
        let text = "creature: x\nhp: 30\n\
             action: Claws | strikes 2 | hit +7 | 1d8+4 slashing \
             | bonus 2d6 psychic with advantage | magical\n\
             action: Slams | strikes 2 | hit +9 | 2d8+5 bludgeoning \
             | all hit 2d8 psychic save con dc 17 stunned until victim end\n\
             action: Reap | recharge 5 | range 10 | strikes each | hit +9 | 2d8+5 bludgeoning \
             | on hit save dex dc 17 prone\n\
             action: Flurry | recharge 5 | strikes 6 | hit +7 | 1d8+4 slashing \
             | no opportunity attacks\n\
             action: Fold | recharge 6 | range 30 | save wis dc 19 | on fail thrown 60 \
             | blast 10 6d12 psychic | on success 3d12 psychic\n\
             bonus: Shadow | in dim light | hide +7\n";
        let c = &parse(text).unwrap()[0];
        assert_eq!(
            c.actions[0].riders,
            vec![
                Rider::BonusDamageWithAdvantage {
                    roll: DamageRoll::new(2, 6, 0, DamageKind::Psychic),
                },
                Rider::MagicWeapons,
            ]
        );
        let Rider::AllHit { damage, then } = &c.actions[1].riders[0] else {
            panic!("expected what every blow landing adds");
        };
        assert_eq!(damage, &vec![DamageRoll::new(2, 8, 0, DamageKind::Psychic)]);
        assert_eq!(
            then.as_deref(),
            Some(&Rider::SaveOrCondition {
                ability: Ability::Con,
                dc: 17,
                conditions: vec![Condition::Stunned],
                duration: Duration::VictimNextTurnEnd,
                cost: None,
                once_per_turn: false,
            })
        );
        assert!(c.actions[2].riders.contains(&Rider::EachInReach));
        assert!(matches!(
            c.actions[2].effect,
            Effect::Strikes { count: 1, .. }
        ));
        assert_eq!(c.actions[3].riders, vec![Rider::NeverProvokes]);
        assert_eq!(
            c.actions[4].effect,
            Effect::Displace(Displace {
                ability: Ability::Wis,
                dc: 19,
                feet: 60,
                away: false,
                radius: 10,
                blast: vec![DamageRoll::new(6, 12, 0, DamageKind::Psychic)],
                on_success: vec![DamageRoll::new(3, 12, 0, DamageKind::Psychic)],
            })
        );
        assert_eq!(c.bonus_actions[0].effect, Effect::Hide { bonus: 7 });

        {
            // A storm thrown by hand: a bolt that jumps, a line ridden, a blow
            // that half lands anyway and bursts around what it struck.
            let text = "creature: x\nhp: 30\nresource: sky 5\nresource: storm 3\n\
             action: Arc | cost sky 2 | range 120 | save dex dc 17 | 3d8 lightning \
             | half on success | chains 30\n\
             action: Ride | cost sky 3 | also cost storm 1 | range 60 | line 60 \
             | save dex dc 17 | 4d6 lightning | half on success\n\
             action: Arrow | ranged | range 60 | takes bonus action | hit +6 | 4d8 lightning \
             | half on miss && area 10 | spares target | save dex dc 17 | 2d8 lightning \
             | half on success\n";
            let c = &parse(text).unwrap()[0];
            assert_eq!(c.actions[0].riders, vec![Rider::Chains { feet: 30 }]);
            assert_eq!(
                c.actions[1].riders,
                vec![
                    Rider::AlsoCosts(Cost {
                        resource: 1,
                        amount: 1
                    }),
                    Rider::RideLine { feet: 60 },
                ]
            );
            let Effect::Sequence(parts) = &c.actions[2].effect else {
                panic!("expected an attack and then a burst");
            };
            assert!(matches!(
                &parts[0],
                Effect::Part { riders, .. }
                    if riders == &vec![Rider::TakesBonusAction, Rider::HalfOnMiss]
            ));
            assert!(matches!(
                &parts[1],
                Effect::Part { riders, .. } if riders == &vec![Rider::SparesTarget]
            ));
            for bad in [
                "action: X | hit +1 | 1d6 fire | spares nobody\n",
                "action: X | hit +1 | 1d6 fire | takes its time\n",
                "action: X | hit +1 | 1d6 fire | also cost nothing 1\n",
                "action: X | save dex dc 10 | 1d6 fire | chains far\n",
            ] {
                let text = format!("creature: x\nhp: 1\n{bad}");
                assert!(parse(&text).is_err(), "should be rejected: {bad}");
            }
        }
        assert_eq!(c.bonus_actions[0].requires, Some(Requirement::Shadow));

        for bad in [
            "action: X | hit +1 | 1d6 fire | all hit\n",
            "action: X | hit +1 | 1d6 fire | no quarter\n",
            "action: X | hit +1 | 1d6 fire | in a hurry\n",
            "action: X | save wis dc 10 | blast 10 1d6 fire\n",
            "action: X | on fail thrown 60 | blast 10 1d6 fire\n",
            "action: X | save wis dc 10 | 1d6 fire | on fail thrown 60\n",
            "action: X | save wis dc 10 | on fail thrown 60 sideways\n",
        ] {
            let text = format!("creature: x\nhp: 1\n{bad}");
            assert!(parse(&text).is_err(), "should be rejected: {bad}");
        }
    }

    /// A reaction that answers whoever hits it - the three phrasings, and the
    /// blows each one waits for.
    #[test]
    fn a_reaction_can_wait_for_a_blow_that_lands() {
        let text = "creature: x\nhp: 30\n\
             reaction: Wrath | when hit in melee | uses 3 | save dex dc 16 | 2d8 lightning \
             | half on success\n\
             reaction: Spite | when hit | 2d6 psychic | hit +0\n\
             reaction: Bristle | when hit by a ranged weapon | save dex dc 14 | 1d6 piercing\n";
        let c = &parse(text).unwrap()[0];
        assert_eq!(
            c.reactions[0].trigger,
            ReactionTrigger::Hit(AttackTrigger::MeleeAttack)
        );
        assert_eq!(c.reactions[0].action.uses, Uses::Limited(3));
        assert_eq!(
            c.reactions[1].trigger,
            ReactionTrigger::Hit(AttackTrigger::AnyAttack)
        );
        assert_eq!(
            c.reactions[2].trigger,
            ReactionTrigger::Hit(AttackTrigger::RangedWeaponAttack)
        );

        let Effect::Save(save) = &c.reactions[0].action.effect else {
            panic!("expected a saving throw");
        };
        assert_eq!(save.dc, 16);
        assert!(save.half_on_success);

        let bad =
            parse("creature: x\nhp: 1\nreaction: X | when hit sideways | 1d6 fire | hit +0\n");
        assert!(bad.is_err(), "`when hit sideways` is not a trigger");
    }

    /// `only <type>` restricts who a save even catches - and it needs a save
    /// to restrict.
    #[test]
    fn a_save_can_be_restricted_to_one_creature_type() {
        let text = "creature: x\nhp: 30\n\
             action: Turn | save wis dc 15 | only undead | on fail frightened until damaged\n";
        let c = &parse(text).unwrap()[0];
        let Effect::Save(save) = &c.actions[0].effect else {
            panic!("expected a saving throw");
        };
        assert_eq!(save.requires_type.as_deref(), Some("undead"));
        assert_eq!(
            save.on_failure,
            vec![(Condition::Frightened, Duration::RoundsOrDamaged(10))]
        );

        let unknown =
            parse("creature: x\nhp: 1\naction: T | save wis dc 15 | only eldritch | 1d6 fire\n");
        assert!(unknown.is_err(), "`eldritch` is not a creature type");
        let no_save = parse("creature: x\nhp: 1\naction: T | only undead | hit +5 | 1d6 fire\n");
        assert!(no_save.is_err(), "a type restriction needs a save");
    }

    /// A weapon a build swings alongside others: named, so an enchantment
    /// can be laid on that one blade, carrying a mastery that marks on a hit
    /// with no save, and dealing whichever of two damage types suits.
    #[test]
    fn a_named_weapon_can_mark_on_a_hit_and_choose_its_damage_type() {
        let text = "creature: x\nhp: 1\n\
             action: Blades | strikes 2 | weapon shortsword | finesse | hit +10 \
             | 1d6+6 slashing | on hit vexed until end \
             && weapon frostreaver | finesse | hit +11 | 1d6+7 slashing or cold\n";
        let c = &parse(text).unwrap()[0];
        let Effect::Sequence(parts) = &c.actions[0].effect else {
            panic!("`&&` joins two parts");
        };

        let Effect::Part { effect, riders, .. } = &parts[0] else {
            panic!("the first part carries the mastery");
        };
        assert_eq!(
            riders[..],
            [Rider::ConditionOnHit {
                condition: Condition::Vexed,
                duration: Duration::ApplierNextTurnEnd,
            }]
        );
        let Effect::Strikes { strike, count } = effect.as_ref() else {
            panic!("two swings");
        };
        assert_eq!(*count, 2);
        assert_eq!(strike.weapon.as_deref(), Some("shortsword"));
        assert!(strike.kind.finesse && strike.kind.weapon && !strike.kind.spell);

        let Effect::Strikes { strike, .. } = &parts[1] else {
            panic!("the second part is a plain swing");
        };
        assert!(
            strike.made_with("Frostreaver"),
            "matched case-insensitively"
        );
        assert_eq!(
            strike.damage,
            vec![DamageRoll::new(1, 6, 7, DamageKind::Slashing).or(DamageKind::Cold)]
        );

        for bad in [
            "creature: x\nhp: 1\naction: A | hit +5 | 1d6 slashing or\n",
            "creature: x\nhp: 1\naction: A | hit +5 | 1d6 slashing or smugness\n",
            "creature: x\nhp: 1\naction: A | hit +5 | 1d6 slashing and cold\n",
            "creature: x\nhp: 1\naction: A | hit +5 | 1d6 slashing | on hit smugness\n",
        ] {
            assert!(parse(bad).is_err(), "should be rejected: {bad}");
        }
    }

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
creature: Monk
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
                conditions,
                duration,
                cost,
                once_per_turn,
            } => {
                assert_eq!((*ability, *dc), (Ability::Con, 16));
                assert_eq!(*conditions, vec![Condition::Stunned]);
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

    /// Each part of a Multiattack keeps its own on-hit riders: the bite's
    /// swallow is not the slams' knockdown.
    #[test]
    fn a_multiattack_keeps_each_parts_riders_on_that_part() {
        let text = "
creature: x
hp: 10
action: Multiattack | hit +9 | 2d10+5 piercing | on hit swallow large
                    && strikes 2 | hit +9 | 2d8+5 bludgeoning | on hit save str dc 17 prone
                    && stance exposed
";
        let m = &parse(text).unwrap()[0].actions[0];
        assert!(m.riders.is_empty(), "nothing rides the whole move");
        let Effect::Sequence(parts) = &m.effect else {
            panic!("expected a sequence, got {:?}", m.effect)
        };
        let riders_of = |p: &Effect| match p {
            Effect::Part { riders, .. } => riders.clone(),
            _ => Vec::new(),
        };
        assert_eq!(
            riders_of(&parts[0]),
            vec![Rider::Swallow {
                max_size: Size::Large
            }]
        );
        assert!(matches!(
            riders_of(&parts[1])[..],
            [Rider::SaveOrCondition { ref conditions, .. }] if conditions == &[Condition::Prone]
        ));
        assert_eq!(
            parts[2],
            Effect::Stance {
                condition: Condition::Exposed
            }
        );
    }

    /// Around a creature with a mouth, each part of a move reaches only where
    /// it says - and one save can knock prone and push at once.
    #[test]
    fn a_reach_rides_its_own_part_and_one_save_can_land_two_conditions() {
        let text = "
creature: x
hp: 10
trait: mouth
trait: difficult terrain
tactic: hit and run
action: Bite | reach mouth | hit +9 | 2d10 piercing
action: Maul | reach mouth | hit +9 | 2d10 piercing
            && reach near | hit +9 | 2d8 bludgeoning | on hit save str dc 17 prone and pushed until victim
";
        let c = &parse(text).unwrap()[0];
        assert!(c.mouth && c.difficult_terrain);
        assert_eq!(c.tactic, crate::creature::Tactic::HitAndRun);
        assert_eq!(c.actions[0].reach, Reach::Mouth);

        let maul = &c.actions[1];
        assert_eq!(maul.reach, Reach::Any, "each part keeps its own");
        let Effect::Sequence(parts) = &maul.effect else {
            panic!("expected a sequence, got {:?}", maul.effect)
        };
        let reach_of = |p: &Effect| match p {
            Effect::Part { reach, .. } => *reach,
            _ => Reach::Any,
        };
        assert_eq!(reach_of(&parts[0]), Reach::Mouth);
        assert_eq!(reach_of(&parts[1]), Reach::Near);
        let Effect::Part { riders, .. } = &parts[1] else {
            panic!("the slam carries its rider")
        };
        assert!(matches!(
            &riders[..],
            [Rider::SaveOrCondition { conditions, duration: Duration::VictimTurn, .. }]
                if conditions == &[Condition::Prone, Condition::Pushed]
        ));

        let far =
            parse("creature: x\nhp: 1\naction: Bite | reach yonder | hit +9 | 1d4 piercing\n")
                .unwrap_err();
        assert!(far.message.contains("not a reach"), "{far}");
        let lost = parse("creature: x\nhp: 1\ntactic: dance\n").unwrap_err();
        assert!(lost.message.contains("not a tactic"), "{lost}");
    }

    #[test]
    fn legendary_points_and_a_squeeze_of_the_swallowed_parse() {
        let text = "
creature: x
hp: 10
legendary: Squeeze | points 2 | swallowed | 4d10 bludgeoning
legendary: Swipe | hit +9 | 1d6 slashing
";
        let c = &parse(text).unwrap()[0];
        assert_eq!(c.legendary[0].legendary_cost, 2);
        assert_eq!(
            c.legendary[0].effect,
            Effect::HarmSwallowed {
                damage: vec![DamageRoll::new(4, 10, 0, DamageKind::Bludgeoning)]
            }
        );
        assert_eq!(c.legendary[1].legendary_cost, 1, "one unless it says so");

        let rolled =
            parse("creature: x\nhp: 1\nlegendary: Squeeze | swallowed | hit +3 | 1d4 acid\n")
                .unwrap_err();
        assert!(rolled.message.contains("without a roll"), "{rolled}");
        let free =
            parse("creature: x\nhp: 1\nlegendary: Nap | points 0 | stance dodging\n").unwrap_err();
        assert!(free.message.contains("at least one"), "{free}");
    }

    #[test]
    fn a_reaction_is_a_move_with_a_when_clause() {
        let text = "
creature: x
hp: 10
reaction: Snap | when enemy pulled | hit +9 | 2d10 piercing
reaction: Spray | save dex dc 18 | 4d10 piercing | half on success | when breached
";
        let c = &parse(text).unwrap()[0];
        assert_eq!(
            c.reactions[0].trigger,
            ReactionTrigger::EnemyGains(Condition::Pulled)
        );
        assert_eq!(c.reactions[0].action.name, "Snap");
        assert!(matches!(
            c.reactions[0].action.effect,
            Effect::Strikes { .. }
        ));
        assert_eq!(c.reactions[1].trigger, ReactionTrigger::Breached);
        assert!(matches!(c.reactions[1].action.effect, Effect::Save(_)));

        let untriggered =
            parse("creature: x\nhp: 1\nreaction: Snap | hit +9 | 2d10 piercing\n").unwrap_err();
        assert!(untriggered.message.contains("`when ...`"), "{untriggered}");
        let odd =
            parse("creature: x\nhp: 1\nreaction: Snap | when sneezes | hit +9 | 2d10 piercing\n")
                .unwrap_err();
        assert!(odd.message.contains("when enemy <condition>"), "{odd}");
    }

    #[test]
    fn an_aura_is_a_move_every_enemy_meets() {
        let text = "creature: x\nhp: 10\naura: Undertow | save str dc 15 | on fail pulled\n";
        let c = &parse(text).unwrap()[0];
        let Effect::Save(save) = &c.auras[0].effect else {
            panic!("expected a save")
        };
        assert_eq!(save.on_failure[0].0, Condition::Pulled);
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

    #[test]
    fn an_autohit_move_parses() {
        let text = "creature: x\nhp: 20\naction: Darts | autohit | 3d4+3 force\n";
        let c = &parse(text).unwrap()[0];
        let darts = &c.actions[0];
        assert_eq!(
            darts.effect,
            Effect::AutoHit {
                damage: vec![DamageRoll::new(3, 4, 3, DamageKind::Force)]
            }
        );

        // Autohit in reaction
        let text_reaction =
            "creature: x\nhp: 20\nreaction: Thorns | when hit in melee | autohit | 4d4 piercing\n";
        let c_react = &parse(text_reaction).unwrap()[0];
        let thorns = &c_react.reactions[0];
        assert_eq!(
            thorns.trigger,
            ReactionTrigger::Hit(AttackTrigger::MeleeAttack)
        );
        assert_eq!(
            thorns.action.effect,
            Effect::AutoHit {
                damage: vec![DamageRoll::new(4, 4, 0, DamageKind::Piercing)]
            }
        );

        // Autohit with hit or save is an error
        let invalid =
            parse("creature: x\nhp: 20\naction: Bad | autohit | hit +5 | 1d6 fire\n").unwrap_err();
        assert!(invalid.message.contains("without a roll"), "{invalid}");
    }
}
