//! Trait phrases: an always-on or reactive modifier (a [`crate::creature::Rider`]),
//! or a flat passive stat bonus.

use crate::creature::{AttackTrigger, Creature, Light, Rider};
use crate::dsl::grammar::lex::{
    arg, parse_damage, parse_damage_kinds, parse_dice, trailing_number,
};
use crate::dsl::grammar::{count, number, parse_duration};
use crate::rules::{Ability, Condition, CreatureType, DamageKind, DamageRoll, Reduction, Size};

/// The direct effect of one `trait:` line.
///
/// Most traits so far - Evasion, Legendary Resistance, Deflect Attacks - are
/// [`Rider`]s: they only matter at a specific point the combat engine already
/// hooks (a save, a reaction). A magic item's passive stat boost is not that
/// shape - a flat bonus to AC, to every saving throw, or to a spell attack/DC
/// is just a number the engine already reads straight off the creature, so it
/// is applied directly rather than routed through a rider that would have
/// nowhere new to fire. Keeping both kinds behind one `TraitEffect` is what
/// lets `traits = [...]` stay one flat list of independent, composable
/// keyword phrases regardless of which shape a given trait turns out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum TraitEffect {
    Rider(Rider),
    /// A flat, always-on bonus to Armor Class - distinct from
    /// [`Rider::ReactionOnTargeted`], which is a *reactive* AC bonus spent
    /// against one attack rather than always active.
    AcBonus(i32),
    /// A flat, always-on bonus to every saving throw.
    SavesBonus(i32),
    /// A flat, always-on bonus to spell attack rolls and spell save DC, added
    /// to the creature's existing
    /// [`crate::rules::SpellCastingProfile::item_bonus`].
    SpellBonus(i32),
    /// Resistance to one or more damage types.
    Resistance(Vec<DamageKind>),
    /// Enemies stand around this creature - see
    /// [`crate::creature::Creature::mouth`].
    Mouth,
    /// How many reactions a round this creature gets - see
    /// [`crate::creature::Creature::reactions_per_round`].
    ReactionsPerRound(u32),
    /// Around it, enemies move one zone a turn - see
    /// [`crate::creature::Creature::difficult_terrain`].
    DifficultTerrain,
    /// A store of spell levels kept in the resource pool `pool` - see
    /// [`Rider::SpellStore`], which this becomes once the pool is found.
    SpellStore {
        pool: String,
        up_to: u32,
        holds: u32,
    },
}

impl TraitEffect {
    /// Apply this effect directly onto `creature`.
    ///
    /// Every variant but [`TraitEffect::SpellBonus`] always succeeds: there is
    /// nothing to validate about adding a number to an AC, a save array, or a
    /// reduction list. A spell attack/DC bonus needs a caster to add itself
    /// to, and a creature with no `spellcasting` profile at all is the one
    /// case this cannot silently do something reasonable with, so it is
    /// reported rather than dropped.
    pub fn apply(self, creature: &mut Creature) -> Result<(), String> {
        match self {
            Self::Rider(rider) => creature.riders.push(rider),
            Self::AcBonus(n) => creature.ac += n,
            Self::SavesBonus(n) => {
                for save in creature.saves.iter_mut() {
                    *save += n;
                }
            }
            Self::SpellBonus(n) => {
                let profile = creature.spellcasting.as_mut().ok_or_else(|| {
                    "a spell attack/DC bonus trait needs this creature to already have a \
                     `spellcasting` profile"
                        .to_string()
                })?;
                profile.item_bonus += n;
            }
            Self::Resistance(kinds) => {
                for kind in kinds {
                    creature.reductions.push((kind, Reduction::Resistant));
                }
            }
            Self::Mouth => creature.mouth = true,
            Self::ReactionsPerRound(n) => creature.reactions_per_round = n.max(1),
            Self::DifficultTerrain => creature.difficult_terrain = true,
            Self::SpellStore { pool, up_to, holds } => {
                let at = creature
                    .resources
                    .iter()
                    .position(|r| r.name == pool)
                    .ok_or_else(|| format!("`{pool}` is not a declared resource"))?;
                creature.riders.push(Rider::SpellStore {
                    pool: at,
                    up_to,
                    holds,
                });
            }
        }
        Ok(())
    }
}

/// An always-on or reactive modifier, or a flat passive stat bonus.
pub fn parse_trait_external(value: &str) -> Result<TraitEffect, String> {
    parse_trait(value)
}

/// An always-on or reactive modifier, or a flat passive stat bonus.
pub(crate) fn parse_trait(value: &str) -> Result<TraitEffect, String> {
    let words: Vec<&str> = value.split_whitespace().collect();
    let head = words
        .first()
        .ok_or("a trait needs a name")?
        .to_ascii_lowercase();
    match head.as_str() {
        "evasion" => {
            let name = arg(&words, 1, value)?;
            let ability = Ability::parse(name)
                .ok_or_else(|| format!("unknown ability `{name}` in `{value}`"))?;
            Ok(TraitEffect::Rider(Rider::NothingOnSuccess { ability }))
        }
        // `legendary resistance 3` - turn a failed save into a success that
        // many times. `always succeed dex 3 reaction` narrows it to one kind
        // of save and makes it cost a reaction, which is a ring that rescues
        // its wearer from a botched Dexterity save: the same mechanism with
        // two of its three fields filled in rather than a second one.
        "legendary" | "always" => {
            let n = words
                .iter()
                .rev()
                .find_map(|w| count(w).ok())
                .ok_or_else(|| format!("expected a number of uses in `{value}`"))?;
            let ability = words[1..].iter().find_map(|w| Ability::parse(w));
            let reaction = words[1..]
                .iter()
                .any(|w| w.eq_ignore_ascii_case("reaction"));
            Ok(TraitEffect::Rider(Rider::AlwaysSucceed {
                uses: n,
                ability,
                reaction,
            }))
        }
        // `magic resistance` - advantage on saves against spells and magic
        // items.
        // `magic weapons` - its weapon attacks are magical, which is what
        // gets through a hide that only resists steel (`resist nonmagical`).
        "magic" => match lowered(&words[1..]).as_str() {
            "resistance" => Ok(TraitEffect::Rider(Rider::MagicResistance)),
            "weapons" | "weapon" | "attacks" => Ok(TraitEffect::Rider(Rider::MagicWeapons)),
            _ => Err(format!(
                "expected `magic resistance` or `magic weapons`, got `{value}`"
            )),
        },
        // `advantage on initiative` - the better of two rolls for its place
        // in the order.
        // `advantage on dex saves [against lightning, thunder]` - on that
        // saving throw, against anything dealing one of those types or, when
        // none is named, against everything.
        "advantage"
            if words.len() >= 4
                && words[1].eq_ignore_ascii_case("on")
                && matches!(
                    words[3].to_ascii_lowercase().as_str(),
                    "saves" | "save" | "saving"
                ) =>
        {
            let ability = Ability::parse(words[2])
                .ok_or_else(|| format!("unknown ability `{}` in `{value}`", words[2]))?;
            let kinds = match words.iter().position(|w| w.eq_ignore_ascii_case("against")) {
                Some(at) => {
                    let kinds = parse_damage_kinds(&words[at + 1..].join(" "), value)?;
                    if kinds.is_empty() {
                        return Err(format!("`{value}` names no damage type after `against`"));
                    }
                    kinds
                }
                None if words.len() == 4 => Vec::new(),
                None => {
                    return Err(format!(
                        "expected `advantage on <ability> saves [against <types>]`, got `{value}`"
                    ))
                }
            };
            Ok(TraitEffect::Rider(Rider::SaveAdvantage { ability, kinds }))
        }
        "advantage" if lowered(&words[1..]) == "on initiative" => {
            Ok(TraitEffect::Rider(Rider::InitiativeAdvantage))
        }
        // `fights up close` - it walks up to its target every turn, even to
        // cast from there, rather than keeping its distance.
        "fights" => match lowered(&words[1..]).as_str() {
            "up close" | "in melee" => Ok(TraitEffect::Rider(Rider::ClosesIn)),
            _ => Err(format!("expected `fights up close`, got `{value}`")),
        },
        // `no opportunity attacks` - nothing gets a swing at it for moving.
        "no" => match lowered(&words[1..]).as_str() {
            "opportunity attacks" | "opportunity attack" => {
                Ok(TraitEffect::Rider(Rider::NeverProvokes))
            }
            _ => Err(format!("expected `no opportunity attacks`, got `{value}`")),
        },
        // `phase through 1d10 psychic` - it walks through other creatures,
        // and each one it passes through takes that.
        "phase" | "phases" => {
            if !arg(&words, 1, value)?.eq_ignore_ascii_case("through") {
                return Err(format!(
                    "expected `phase through NdM <type>`, got `{value}`"
                ));
            }
            let rolls = parse_damage(&words[2..].join(" "))?;
            let [roll] = rolls[..] else {
                return Err(format!(
                    "expected `phase through NdM <type>`, got `{value}`"
                ));
            };
            Ok(TraitEffect::Rider(Rider::PhaseThrough { roll }))
        }
        // `mirror psychic 10` - damage of that type it would take lands on
        // everyone within that many feet of it instead.
        "mirror" | "mirrors" => {
            let (feet, kinds) = words[1..]
                .split_last()
                .ok_or_else(|| format!("expected `mirror <types> <feet>`, got `{value}`"))?;
            let kinds = parse_damage_kinds(&kinds.join(" "), value)?;
            if kinds.is_empty() {
                return Err(format!("`{value}` needs at least one damage type"));
            }
            Ok(TraitEffect::Rider(Rider::MirrorDamage {
                kinds,
                feet: count(feet)?,
            }))
        }
        // `swap when hit 60 gnoll` - a reaction: hit by an attack roll, it
        // changes places with one of its own side within that many feet
        // whose name has those words in it, and that one is hit instead.
        "swap" => {
            if lowered(&words[1..words.len().min(3)]) != "when hit" || words.len() < 5 {
                return Err(format!(
                    "expected `swap when hit <feet> <name of its kin>`, got `{value}`"
                ));
            }
            Ok(TraitEffect::Rider(Rider::SwapWhenHit {
                feet: count(words[3])?,
                kin: words[4..].join(" "),
            }))
        }
        // `passive perception 15` - what a Stealth check has to beat to
        // hide from it.
        "passive" => {
            if !arg(&words, 1, value)?.eq_ignore_ascii_case("perception") || words.len() != 3 {
                return Err(format!("expected `passive perception <n>`, got `{value}`"));
            }
            Ok(TraitEffect::Rider(Rider::PassivePerception(number(
                words[2],
            )?)))
        }
        // `in dim light`, `in darkness`, `in bright light` - where it
        // stands, for a move that needs the dark (`in dim light` on a move).
        "in" => {
            let word = arg(&words, 1, value)?;
            let light = Light::parse(word)
                .filter(|_| matches!(lowered(&words[2..]).as_str(), "" | "light"))
                .ok_or_else(|| {
                    format!("expected `in bright light`, `in dim light` or `in darkness`, got `{value}`")
                })?;
            Ok(TraitEffect::Rider(Rider::Lit(light)))
        }
        // `advantage against frightened` - on its attack rolls against
        // anything under that condition.
        "advantage" => {
            let named = match words[1..] {
                [against, name] if against.eq_ignore_ascii_case("against") => name,
                _ => {
                    return Err(format!(
                        "expected `advantage against <condition>`, got `{value}`"
                    ))
                }
            };
            let condition = Condition::parse(named)
                .ok_or_else(|| format!("unknown condition `{named}` in `{value}`"))?;
            Ok(TraitEffect::Rider(Rider::AdvantageAgainst { condition }))
        }
        // `redirect attacks wis dc 20` - once a turn, an attacker that fails
        // the save hits one of its own side instead.
        "redirect" => {
            let (ability, dc) = match words[1..] {
                [attacks, ability, dc_word, dc]
                    if attacks.eq_ignore_ascii_case("attacks")
                        && dc_word.eq_ignore_ascii_case("dc") =>
                {
                    (ability, dc)
                }
                _ => {
                    return Err(format!(
                        "expected `redirect attacks <ability> dc <n>`, got `{value}`"
                    ))
                }
            };
            Ok(TraitEffect::Rider(Rider::RedirectAttacks {
                ability: Ability::parse(ability)
                    .ok_or_else(|| format!("unknown ability `{ability}` in `{value}`"))?,
                dc: number(dc)?,
            }))
        }
        // `reflect psychic` - whoever deals it that damage takes as much.
        "reflect" => {
            let kinds = parse_damage_kinds(&words[1..].join(" "), value)?;
            if kinds.is_empty() {
                return Err(format!("`{value}` needs at least one damage type"));
            }
            Ok(TraitEffect::Rider(Rider::ReflectDamage { kinds }))
        }
        // `spell store rod up to 5 holds 50` - a pool of stored spell
        // levels that pays for slots, and is fed by the spells it stops.
        "spell"
            if words
                .get(1)
                .is_some_and(|w| w.eq_ignore_ascii_case("store")) =>
        {
            match words[2..] {
                [pool, up, to, level, holds, most]
                    if up.eq_ignore_ascii_case("up")
                        && to.eq_ignore_ascii_case("to")
                        && holds.eq_ignore_ascii_case("holds") =>
                {
                    Ok(TraitEffect::SpellStore {
                        pool: pool.to_string(),
                        up_to: count(level)?,
                        holds: count(most)?,
                    })
                }
                _ => Err(format!(
                    "expected `spell store <pool> up to <level> holds <n>`, got `{value}`"
                )),
            }
        }
        // `refuses healing` - nobody else can heal it.
        "refuses" => {
            if !value.trim().eq_ignore_ascii_case("refuses healing") {
                return Err(format!("expected `refuses healing`, got `{value}`"));
            }
            Ok(TraitEffect::Rider(Rider::RefusesHealing))
        }
        // `truesight 30` - what is invisible within that many feet is not.
        "truesight" => Ok(TraitEffect::Rider(Rider::Truesight {
            feet: count(arg(&words, 1, value)?)?,
        })),
        // `first hit 2d6 necrotic` - on the first hit of the fight only.
        "first" => {
            if !arg(&words, 1, value)?.eq_ignore_ascii_case("hit") || words.len() != 4 {
                return Err(format!("expected `first hit NdM <type>`, got `{value}`"));
            }
            let (dice, sides, bonus) = parse_dice(words[2])?;
            let damage_kind = DamageKind::parse(words[3])
                .ok_or_else(|| format!("unknown damage type `{}` in `{value}`", words[3]))?;
            Ok(TraitEffect::Rider(Rider::FirstHitDamage {
                dice_count: dice,
                dice_sides: sides,
                bonus,
                damage_kind,
            }))
        }
        // `deflect 1d10+7 bludgeoning, piercing, slashing` - a reaction.
        "deflect" | "reduce" => {
            let (dice, sides, bonus) = parse_dice(arg(&words, 1, value)?)?;
            let rest = value
                .split_once(arg(&words, 1, value)?)
                .map(|(_, r)| r)
                .unwrap_or("");
            let kinds = parse_damage_kinds(rest, value)?;
            if kinds.is_empty() {
                return Err(format!("`{value}` needs the damage types it applies to"));
            }
            Ok(TraitEffect::Rider(Rider::ReduceDamage {
                // The type on the reduction roll is never read; only its dice.
                roll: DamageRoll::new(dice, sides, bonus, DamageKind::Force),
                kinds,
            }))
        }
        // `extra damage applies to spell attacks` - a creature's Sneak
        // Attack also qualifies on a spell attack roll.
        "extra" => {
            if value
                .to_ascii_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                != ["extra", "damage", "applies", "to", "spell", "attacks"]
            {
                return Err(format!(
                    "expected `extra damage applies to spell attacks`, got `{value}`"
                ));
            }
            Ok(TraitEffect::Rider(Rider::ExtraDamageAppliesToSpellAttacks))
        }
        // `halve attack damage`, or the feature's own name `uncanny dodge` -
        // a reaction that halves one hit.
        "halve" | "uncanny" => Ok(TraitEffect::Rider(Rider::HalveAttackDamage)),
        // `reactions 3` - how many reactions a round, still one per turn.
        "reactions" => {
            let n = count(arg(&words, 1, value)?)?;
            if words.len() > 2 {
                return Err(format!("unexpected words at the end of `{value}`"));
            }
            if n == 0 {
                return Err(format!("`{value}`: a creature gets at least one"));
            }
            Ok(TraitEffect::ReactionsPerRound(n))
        }
        // `reaction ac 5 [vs ranged weapon|melee] [until next turn]` - a
        // reaction that raises AC against an attack roll: against that one
        // attack, or - with `until next turn` - against every further attack
        // it answers until the start of its holder's next turn, which is what
        // the Shield spell and a parry both do.
        "reaction" => {
            if !arg(&words, 1, value)?.eq_ignore_ascii_case("ac") {
                return Err(format!("expected `reaction ac <n> ...`, got `{value}`"));
            }
            let ac_bonus = number(arg(&words, 2, value)?)?;
            let mut rest: Vec<String> = words[3..].iter().map(|w| w.to_ascii_lowercase()).collect();
            // `until next turn` / `until your next turn`, wherever the phrase
            // ends: what is left is the trigger.
            let lasting = rest.iter().any(|w| w == "until");
            if lasting {
                let at = rest.iter().position(|w| w == "until").expect("just found");
                let tail = rest[at..].join(" ");
                if !matches!(
                    tail.as_str(),
                    "until next turn" | "until your next turn" | "until my next turn"
                ) {
                    return Err(format!("`{tail}` is not a duration in `{value}`"));
                }
                rest.truncate(at);
            }
            let trigger = match rest.split_first() {
                None => AttackTrigger::AnyAttack,
                Some((first, named)) if first == "vs" => match named.join(" ").as_str() {
                    "any" | "any attack" => AttackTrigger::AnyAttack,
                    "ranged weapon" | "ranged weapon attack" | "ranged weapon attacks" => {
                        AttackTrigger::RangedWeaponAttack
                    }
                    "melee" | "melee attack" | "melee attacks" => AttackTrigger::MeleeAttack,
                    other => return Err(format!("`vs {other}` is not an attack trigger")),
                },
                Some((other, _)) => return Err(format!("unexpected `{other}` in `{value}`")),
            };
            Ok(TraitEffect::Rider(Rider::ReactionOnTargeted {
                trigger,
                ac_bonus,
                lasting,
            }))
        }
        // `ac 2`, or the more readable `ac bonus 2` - a passive item's flat
        // Armor Class bonus.
        "ac" => Ok(TraitEffect::AcBonus(trailing_number(&words, value)?)),
        // `saves 1`, or `saves bonus 1` - a passive item's flat bonus to
        // every saving throw, as opposed to one ability at a time the way
        // the stat block's own `saves:` table works.
        "saves" | "save" => Ok(TraitEffect::SavesBonus(trailing_number(&words, value)?)),
        // `spell 1`, or `spell bonus 1` - a passive item's flat bonus to
        // spell attack rolls and spell save DC, added to the caster's own
        // `SpellCastingProfile::item_bonus`.
        "spell" => Ok(TraitEffect::SpellBonus(trailing_number(&words, value)?)),
        // `resistance fire`, or `resistance fire, cold` - a passive item's
        // grant of resistance to one or more damage types, alongside
        // whatever the stat block's own `resist:` list already carries.
        // `resist nonmagical bludgeoning, piercing, slashing` - only from
        // attacks that are not magical.
        "resistance" | "resist"
            if words
                .get(1)
                .is_some_and(|w| w.eq_ignore_ascii_case("nonmagical")) =>
        {
            let kinds = parse_damage_kinds(&words[2..].join(" "), value)?;
            if kinds.is_empty() {
                return Err(format!("`{value}` needs at least one damage type"));
            }
            Ok(TraitEffect::Rider(Rider::ResistNonmagical { kinds }))
        }
        "resistance" | "resist" => {
            let kinds = parse_damage_kinds(&words[1..].join(" "), value)?;
            if kinds.is_empty() {
                return Err(format!("`{value}` needs at least one damage type"));
            }
            Ok(TraitEffect::Resistance(kinds))
        }
        // `once per turn 1d6 piercing` - extra dice on the first hit this
        // creature lands on each of its own turns, whatever it hits with: a
        // swarm that joins one blow a turn. `... with a weapon` narrows it to
        // a weapon attack, which is how a divine strike is written.
        "once" => {
            let at = words
                .iter()
                .position(|w| parse_dice(w).is_ok())
                .ok_or_else(|| format!("expected `once per turn NdM <type>`, got `{value}`"))?;
            let (dice, sides, bonus) = parse_dice(words[at])?;
            let kind_word = arg(&words, at + 1, value)?;
            let damage_kind = DamageKind::parse(kind_word)
                .ok_or_else(|| format!("unknown damage type `{kind_word}` in `{value}`"))?;
            let tail = words[(at + 2).min(words.len())..]
                .iter()
                .map(|w| w.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            let weapon_only = match tail.as_str() {
                "" => false,
                "with a weapon" | "with weapons" | "on weapon hits" => true,
                other => {
                    return Err(format!(
                        "`{other}` is not part of `once per turn NdM <type> [with a weapon]`"
                    ))
                }
            };
            Ok(TraitEffect::Rider(Rider::OncePerTurnDamage {
                dice_count: dice,
                dice_sides: sides,
                bonus,
                damage_kind,
                weapon_only,
            }))
        }
        // `push on lightning, thunder [up to large]` - damage of those types
        // also shoves whoever takes it, if it is that size or smaller.
        "push" | "shove" => {
            if !arg(&words, 1, value)?.eq_ignore_ascii_case("on") {
                return Err(format!(
                    "expected `push on <damage types> [up to <size>]`, got `{value}`"
                ));
            }
            // `up to <size>`, if it is there at all: what is before it is the
            // damage type list.
            let (kinds_words, max_size) = match words
                .iter()
                .position(|w| w.eq_ignore_ascii_case("up") || w.eq_ignore_ascii_case("size"))
            {
                Some(at) => {
                    let size_word = words.last().copied().unwrap_or_default();
                    let size = Size::parse(size_word)
                        .ok_or_else(|| format!("unknown size `{size_word}` in `{value}`"))?;
                    (&words[2..at], size)
                }
                // Nothing said: this shoves whatever it lands on.
                None => (&words[2..], Size::Gargantuan),
            };
            let kinds = parse_damage_kinds(&kinds_words.join(" "), value)?;
            if kinds.is_empty() {
                return Err(format!("`{value}` needs at least one damage type"));
            }
            Ok(TraitEffect::Rider(Rider::PushOnDamage { kinds, max_size }))
        }
        // `quarry 1d6 force` - extra dice on every hit against whatever this
        // creature has marked as its quarry and is still concentrating on.
        "quarry" => {
            let (dice, sides, bonus) = parse_dice(arg(&words, 1, value)?)?;
            let kind_word = arg(&words, 2, value)?;
            let damage_kind = DamageKind::parse(kind_word)
                .ok_or_else(|| format!("unknown damage type `{kind_word}` in `{value}`"))?;
            if words.len() > 3 {
                return Err(format!("unexpected words at the end of `{value}`"));
            }
            Ok(TraitEffect::Rider(Rider::BonusDamageVsQuarry {
                dice_count: dice,
                dice_sides: sides,
                bonus,
                damage_kind,
            }))
        }
        // `bonus 3d6 piercing vs dragon` - extra damage dice on a hit against
        // one creature type, on every attack this creature makes: a
        // favoured-enemy bonus. A weapon's own bonus is the same words as a
        // clause on that weapon's move.
        "bonus" => Ok(TraitEffect::Rider(parse_bonus_vs(&words, value)?)),
        // `downgrade immunity poison damage, poisoned condition` - either
        // term alone, or both combined, in any order.
        "downgrade" => {
            let lower = value.to_ascii_lowercase();
            let at = lower
                .find("immunity")
                .ok_or_else(|| format!("expected `downgrade immunity ...`, got `{value}`"))?;
            let rest = &value[at + "immunity".len()..];
            let mut damage = None;
            let mut condition = None;
            for term in rest.split(',') {
                let term = term.trim();
                if term.is_empty() {
                    continue;
                }
                let mut words = term.split_whitespace();
                let (Some(name), Some(kind_word)) = (words.next(), words.next()) else {
                    return Err(format!(
                        "expected `<name> damage` or `<name> condition` in `{value}`, got `{term}`"
                    ));
                };
                if words.next().is_some() {
                    return Err(format!(
                        "`{term}` has more than a name and a type in `{value}`"
                    ));
                }
                match kind_word.to_ascii_lowercase().as_str() {
                    "damage" => {
                        damage = Some(DamageKind::parse(name).ok_or_else(|| {
                            format!("unknown damage type `{name}` in `{value}`")
                        })?);
                    }
                    "condition" => {
                        condition = Some(Condition::parse(name).ok_or_else(|| {
                            format!("unknown condition `{name}` in `{value}`")
                        })?);
                    }
                    other => {
                        return Err(format!(
                            "expected `damage` or `condition` after `{name}`, got `{other}` in `{value}`"
                        ))
                    }
                }
            }
            if damage.is_none() && condition.is_none() {
                return Err(format!(
                    "`{value}` needs at least one of a damage type or a condition to downgrade"
                ));
            }
            Ok(TraitEffect::Rider(Rider::DowngradeImmunity {
                damage,
                condition,
            }))
        }
        // `ignore resistance fire`, or `ignore resistance piercing, slashing` -
        // attacks and spells ignore resistance to those damage types.
        "ignore" | "ignores" => {
            if !arg(&words, 1, value)?.eq_ignore_ascii_case("resistance") {
                return Err(format!(
                    "expected `ignore resistance <types>`, got `{value}`"
                ));
            }
            let kinds = parse_damage_kinds(&words[2..].join(" "), value)?;
            if kinds.is_empty() {
                return Err(format!(
                    "`{value}` needs at least one damage type to ignore resistance for"
                ));
            }
            Ok(TraitEffect::Rider(Rider::IgnoreResistance { kinds }))
        }
        // `empower weapon 2d6 poison on poisoned` - an attacker-side buff,
        // dormant until this creature inflicts the named condition on a
        // target via a weapon attack, after which its weapon attacks carry
        // the extra dice for the rest of the encounter. Generic over the
        // trigger condition and the damage type; nothing here is specific to
        // poison.
        "empower" => {
            let weapon_word = arg(&words, 1, value)?;
            if !weapon_word.eq_ignore_ascii_case("weapon") {
                return Err(format!("expected `empower weapon ...`, got `{value}`"));
            }
            let (dice, sides, bonus) = parse_dice(arg(&words, 2, value)?)?;
            let kind_word = arg(&words, 3, value)?;
            let damage_kind = DamageKind::parse(kind_word)
                .ok_or_else(|| format!("unknown damage type `{kind_word}` in `{value}`"))?;
            let on_word = arg(&words, 4, value)?;
            if !on_word.eq_ignore_ascii_case("on") {
                return Err(format!("expected `on <condition>` in `{value}`"));
            }
            let condition_word = arg(&words, 5, value)?;
            let trigger = Condition::parse(condition_word)
                .ok_or_else(|| format!("unknown condition `{condition_word}` in `{value}`"))?;
            Ok(TraitEffect::Rider(Rider::ConditionTriggeredWeaponDamage {
                trigger,
                dice_count: dice,
                dice_sides: sides,
                bonus,
                damage_kind,
            }))
        }
        // `injury poison con dc 13 [poisoned] disadvantage str [duration]` -
        // a consumable injury poison coating a weapon: the next weapon hit
        // forces a saving throw, and a failure burdens the target's own future
        // saves of a second, independently chosen ability with Disadvantage -
        // plus the named condition, if any - for the stated duration. Generic
        // over both abilities; nothing here is tied to a specific poison's
        // name or flavour.
        "injury" => {
            let poison_word = arg(&words, 1, value)?;
            if !poison_word.eq_ignore_ascii_case("poison") {
                return Err(format!("expected `injury poison ...`, got `{value}`"));
            }
            let ability = Ability::parse(arg(&words, 2, value)?)
                .ok_or_else(|| format!("unknown ability in `{value}`"))?;
            if !arg(&words, 3, value)?.eq_ignore_ascii_case("dc") {
                return Err(format!("expected `dc <n>` in `{value}`"));
            }
            let dc = number(arg(&words, 4, value)?)?;
            let mut at = 5;
            let condition = if arg(&words, at, value)?.eq_ignore_ascii_case("disadvantage") {
                None
            } else {
                let name = arg(&words, at, value)?;
                at += 1;
                Some(Condition::parse(name).ok_or_else(|| {
                    format!(
                        "expected `[condition] disadvantage <ability>` in `{value}`, got `{name}`"
                    )
                })?)
            };
            if !arg(&words, at, value)?.eq_ignore_ascii_case("disadvantage") {
                return Err(format!("expected `disadvantage <ability>` in `{value}`"));
            }
            let debuffed_ability = Ability::parse(arg(&words, at + 1, value)?)
                .ok_or_else(|| format!("unknown ability in `{value}`"))?;
            let (duration, used) = parse_duration(&words, at + 2, value)?;
            if at + 2 + used != words.len() {
                return Err(format!("unexpected words at the end of `{value}`"));
            }
            Ok(TraitEffect::Rider(Rider::InjuryPoison {
                ability,
                dc,
                debuffed_ability,
                condition,
                duration: duration.resolve(Some((ability, dc)), value)?,
            }))
        }
        // `damage threshold 51 [cracks] [weak spot resists bludgeoning,
        // piercing, slashing]` - a shell that ignores any single instance of
        // damage below the threshold; see `Rider::DamageThreshold`.
        "damage" => {
            if !arg(&words, 1, value)?.eq_ignore_ascii_case("threshold") {
                return Err(format!(
                    "expected `damage threshold <n> ...`, got `{value}`"
                ));
            }
            let threshold = number(arg(&words, 2, value)?)?;
            if threshold <= 0 {
                return Err(format!("`{value}`: a damage threshold is at least 1"));
            }
            let mut at = 3;
            let cracks = words
                .get(at)
                .is_some_and(|w| w.eq_ignore_ascii_case("cracks"));
            if cracks {
                at += 1;
            }
            let mut weak_spot_resists = Vec::new();
            if at < words.len() {
                let lead: Vec<String> = words[at..]
                    .iter()
                    .take(3)
                    .map(|w| w.to_ascii_lowercase())
                    .collect();
                if lead[..] != ["weak", "spot", "resists"] && lead[..] != ["weak", "spot", "resist"]
                {
                    return Err(format!(
                        "expected `[cracks] [weak spot resists <types>]` after the threshold in `{value}`"
                    ));
                }
                weak_spot_resists = parse_damage_kinds(&words[at + 3..].join(" "), value)?;
                if weak_spot_resists.is_empty() {
                    return Err(format!("`{value}` names no damage type for the weak spot"));
                }
            }
            Ok(TraitEffect::Rider(Rider::DamageThreshold {
                threshold,
                cracks,
                weak_spot_resists,
            }))
        }
        // `mouth` - enemies stand around it: at its mouth, beside its body,
        // or out in front.
        "mouth" if words.len() == 1 => Ok(TraitEffect::Mouth),
        // `difficult terrain` - around it, enemies move one zone a turn.
        "difficult" if words.len() == 2 && words[1].eq_ignore_ascii_case("terrain") => {
            Ok(TraitEffect::DifficultTerrain)
        }
        // `digest 8d6 acid, 4d6 cold` - what each swallowed creature takes at
        // the start of this creature's turns.
        "digest" | "digestion" => {
            let rest = value.trim_start()[words[0].len()..].trim();
            let damage = parse_damage(rest)?;
            Ok(TraitEffect::Rider(Rider::Digestion { damage }))
        }
        // `regurgitate 60 con dc 22` - enough damage from inside in one turn
        // forces a save to keep everything it swallowed down.
        "regurgitate" => {
            let threshold = number(arg(&words, 1, value)?)?;
            let ability = Ability::parse(arg(&words, 2, value)?)
                .ok_or_else(|| format!("unknown ability in `{value}`"))?;
            if !arg(&words, 3, value)?.eq_ignore_ascii_case("dc") {
                return Err(format!(
                    "expected `regurgitate <damage> <ability> dc <n>`, got `{value}`"
                ));
            }
            let dc = number(arg(&words, 4, value)?)?;
            if words.len() > 5 {
                return Err(format!("unexpected words at the end of `{value}`"));
            }
            Ok(TraitEffect::Rider(Rider::Regurgitate {
                threshold,
                ability,
                dc,
            }))
        }
        other => Err(format!("unknown trait `{other}`")),
    }
}

/// Words as one lower-case phrase, for matching a trait that is nothing but
/// its words.
fn lowered(words: &[&str]) -> String {
    words
        .iter()
        .map(|w| w.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `bonus 3d6 piercing vs dragon` - extra damage dice on a hit against one
/// creature type. As a trait it rides every attack the creature makes; as a
/// move clause, only that move's.
pub(super) fn parse_bonus_vs(words: &[&str], clause: &str) -> Result<Rider, String> {
    let (dice, sides, bonus) = parse_dice(arg(words, 1, clause)?)?;
    let kind_word = arg(words, 2, clause)?;
    let damage_kind = DamageKind::parse(kind_word)
        .ok_or_else(|| format!("unknown damage type `{kind_word}` in `{clause}`"))?;
    let vs_word = arg(words, 3, clause)?;
    if !vs_word.eq_ignore_ascii_case("vs") {
        return Err(format!(
            "expected `bonus NdM <damage type> vs <creature type or condition>`, got `{clause}`"
        ));
    }
    let type_word = arg(words, 4, clause)?;
    // What it is, or what it is under: `vs dragon`, `vs frightened`.
    if let (None, Some(condition)) = (CreatureType::parse(type_word), Condition::parse(type_word)) {
        return Ok(Rider::BonusDamageVsCondition {
            dice_count: dice,
            dice_sides: sides,
            bonus,
            damage_kind,
            condition,
        });
    }
    let creature_type = CreatureType::parse(type_word).ok_or_else(|| {
        format!("`{type_word}` is neither a creature type nor a condition in `{clause}`")
    })?;
    Ok(Rider::BonusDamageVsCreatureType {
        dice_count: dice,
        dice_sides: sides,
        bonus,
        damage_kind,
        creature_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::scenario::parse;
    use crate::rules::{DamageKind, Duration, Size};

    /// `push on <types> [up to <size>]` - damage of those types also shoves
    /// whoever takes it, if it is small enough.
    #[test]
    fn damage_of_a_type_can_shove_what_it_hits() {
        let text = "
creature: x
hp: 10
trait: push on lightning, thunder up to large
trait: shove on cold
";
        let c = &parse(text).unwrap()[0];
        assert_eq!(
            c.riders[0],
            Rider::PushOnDamage {
                kinds: vec![DamageKind::Lightning, DamageKind::Thunder],
                max_size: Size::Large,
            }
        );
        assert_eq!(
            c.riders[1],
            Rider::PushOnDamage {
                kinds: vec![DamageKind::Cold],
                // Nothing said: it shoves whatever it lands on.
                max_size: Size::Gargantuan,
            }
        );

        // `with a weapon` narrows it to a weapon attack; anything else
        // after the damage type is a typo rather than a phrase.
        let narrowed =
            &parse("creature: x\nhp: 1\ntrait: once per turn 1d8 thunder with a weapon\n").unwrap()
                [0];
        assert_eq!(
            narrowed.riders[0],
            Rider::OncePerTurnDamage {
                dice_count: 1,
                dice_sides: 8,
                bonus: 0,
                damage_kind: DamageKind::Thunder,
                weapon_only: true,
            }
        );
        assert!(parse("creature: x\nhp: 1\ntrait: once per turn 1d8 thunder sideways\n").is_err());

        for bad in [
            "creature: x\nhp: 1\ntrait: push on sparkly\n",
            "creature: x\nhp: 1\ntrait: push on lightning up to colossal\n",
            "creature: x\nhp: 1\ntrait: push lightning\n",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    /// What a creature is, that the rest of its kit leans on: a hide only
    /// magic bites, the dark it keeps to, how it moves and what it does with
    /// what would hurt it.
    #[test]
    fn the_traits_of_something_not_quite_here_parse() {
        let text = "
creature: x
hp: 10
trait: resist nonmagical bludgeoning, piercing, slashing
trait: magic weapons
trait: advantage on initiative
trait: no opportunity attacks
trait: phase through 1d10 psychic
trait: mirror psychic 10
trait: swap when hit 60 rift guard
trait: passive perception 19
trait: in dim light
";
        let c = &parse(text).unwrap()[0];
        assert_eq!(
            c.riders,
            vec![
                Rider::ResistNonmagical {
                    kinds: vec![
                        DamageKind::Bludgeoning,
                        DamageKind::Piercing,
                        DamageKind::Slashing
                    ],
                },
                Rider::MagicWeapons,
                Rider::InitiativeAdvantage,
                Rider::NeverProvokes,
                Rider::PhaseThrough {
                    roll: DamageRoll::new(1, 10, 0, DamageKind::Psychic),
                },
                Rider::MirrorDamage {
                    kinds: vec![DamageKind::Psychic],
                    feet: 10,
                },
                Rider::SwapWhenHit {
                    feet: 60,
                    kin: "rift guard".to_string(),
                },
                Rider::PassivePerception(19),
                Rider::Lit(Light::Dim),
            ]
        );
        assert_eq!(
            parse_trait("advantage on dex saves").unwrap(),
            TraitEffect::Rider(Rider::SaveAdvantage {
                ability: Ability::Dex,
                kinds: vec![],
            })
        );
        assert_eq!(
            parse_trait("advantage on dex saves against lightning, thunder").unwrap(),
            TraitEffect::Rider(Rider::SaveAdvantage {
                ability: Ability::Dex,
                kinds: vec![DamageKind::Lightning, DamageKind::Thunder],
            })
        );
        assert!(parse_trait("advantage on dex saves sometimes").is_err());
        assert!(parse_trait("advantage on luck saves").is_err());
        assert_eq!(c.passive_perception(), 19);
        assert_eq!(c.light(), Light::Dim);
        // The plain resistance and the plain advantage still read as they
        // did, and the last light said is the one that holds.
        assert_eq!(c.reduction(DamageKind::Slashing), Reduction::Normal);
        let lit =
            &parse("creature: x\nhp: 1\ntrait: in darkness\ntrait: in bright light\n").unwrap()[0];
        assert_eq!(lit.light(), Light::Bright);
        assert!(matches!(
            parse_trait("advantage against prone").unwrap(),
            TraitEffect::Rider(Rider::AdvantageAgainst { .. })
        ));

        for bad in [
            "resist nonmagical",
            "magic missiles",
            "no retreat",
            "phase through walls",
            "mirror 10",
            "swap when hit 60",
            "passive perception keen",
            "in twilight",
        ] {
            assert!(parse_trait(bad).is_err(), "should be rejected: {bad}");
        }
    }

    #[test]
    fn traits_become_riders() {
        let text = "
creature: x
hp: 10
trait: evasion dex
trait: legendary resistance 3
trait: deflect 1d10+7 bludgeoning, piercing, slashing
";
        let c = &parse(text).unwrap()[0];
        assert!(c.has_evasion(Ability::Dex));
        assert!(!c.has_evasion(Ability::Con));
        assert!(c
            .riders
            .iter()
            .any(|r| matches!(r, Rider::AlwaysSucceed { uses: 3, .. })));
        let deflect = c
            .riders
            .iter()
            .find_map(|r| match r {
                Rider::ReduceDamage { kinds, roll } => Some((kinds, roll)),
                _ => None,
            })
            .expect("deflect parsed");
        assert_eq!(deflect.0.len(), 3);
        assert_eq!(
            (deflect.1.count, deflect.1.sides, deflect.1.bonus),
            (1, 10, 7)
        );
    }

    /// The four newest trait phrases, each read back as the rider it stands
    /// for - and the two widened ones keeping their old readings.
    #[test]
    fn the_newer_trait_phrases_parse_into_their_riders() {
        let text = "
creature: x
hp: 10
trait: once per turn 1d6 piercing
trait: quarry 1d6 force
trait: always succeed dex 3 reaction
trait: reaction ac 4 vs melee until next turn
trait: legendary resistance 3
trait: reaction ac 5 vs ranged weapon
";
        let c = &parse(text).unwrap()[0];
        assert_eq!(
            c.riders,
            vec![
                Rider::OncePerTurnDamage {
                    dice_count: 1,
                    dice_sides: 6,
                    bonus: 0,
                    damage_kind: DamageKind::Piercing,
                    weapon_only: false,
                },
                Rider::BonusDamageVsQuarry {
                    dice_count: 1,
                    dice_sides: 6,
                    bonus: 0,
                    damage_kind: DamageKind::Force,
                },
                Rider::AlwaysSucceed {
                    uses: 3,
                    ability: Some(Ability::Dex),
                    reaction: true,
                },
                Rider::ReactionOnTargeted {
                    trigger: AttackTrigger::MeleeAttack,
                    ac_bonus: 4,
                    lasting: true,
                },
                // Legendary Resistance is still the same words and the same
                // rider: any save, no reaction.
                Rider::AlwaysSucceed {
                    uses: 3,
                    ability: None,
                    reaction: false,
                },
                Rider::ReactionOnTargeted {
                    trigger: AttackTrigger::RangedWeaponAttack,
                    ac_bonus: 5,
                    lasting: false,
                },
            ]
        );

        for bad in [
            "creature: x\nhp: 1\ntrait: once per turn 1d6 confusion\n",
            "creature: x\nhp: 1\ntrait: quarry 1d6\n",
            "creature: x\nhp: 1\ntrait: reaction ac 4 vs melee until dawn\n",
            "creature: x\nhp: 1\ntrait: reaction ac 4 vs teeth\n",
        ] {
            assert!(parse(bad).is_err(), "should be rejected: {bad}");
        }
    }

    /// The generic "bonus damage vs a creature type" trait: any weapon or
    /// attack can carry it, parameterized entirely by the trait string - no
    /// struct or plugin names the specific weapon.
    #[test]
    fn bonus_damage_vs_creature_type_trait_parses() {
        let text = "
creature: x
hp: 10
trait: bonus 3d6 piercing vs dragon
";
        let c = &parse(text).unwrap()[0];
        assert_eq!(
            c.riders,
            vec![Rider::BonusDamageVsCreatureType {
                dice_count: 3,
                dice_sides: 6,
                bonus: 0,
                damage_kind: DamageKind::Piercing,
                creature_type: CreatureType::Dragon,
            }]
        );
    }

    /// The generic immunity-downgrade trait, parsed with both halves present
    /// at once - the case a single attacker with both a damage-type and a
    /// condition it punches through needs.
    #[test]
    fn downgrade_immunity_trait_parses_both_halves_together() {
        let text = "
creature: x
hp: 10
trait: downgrade immunity poison damage, poisoned condition
";
        let c = &parse(text).unwrap()[0];
        let rider = c
            .riders
            .iter()
            .find(|r| matches!(r, Rider::DowngradeImmunity { .. }))
            .expect("parsed a downgrade-immunity rider");
        match rider {
            Rider::DowngradeImmunity { damage, condition } => {
                assert_eq!(*damage, Some(DamageKind::Poison));
                assert_eq!(*condition, Some(Condition::Poisoned));
            }
            other => panic!("expected DowngradeImmunity, got {other:?}"),
        }
    }

    /// Each half is independently optional - an attacker might carry only
    /// one, so the parser must not require both.
    #[test]
    fn downgrade_immunity_trait_allows_either_half_alone() {
        let damage_only = parse_trait_external("downgrade immunity fire damage").unwrap();
        assert_eq!(
            damage_only,
            TraitEffect::Rider(Rider::DowngradeImmunity {
                damage: Some(DamageKind::Fire),
                condition: None,
            })
        );

        let condition_only = parse_trait_external("downgrade immunity stunned condition").unwrap();
        assert_eq!(
            condition_only,
            TraitEffect::Rider(Rider::DowngradeImmunity {
                damage: None,
                condition: Some(Condition::Stunned),
            })
        );
    }

    #[test]
    fn bonus_damage_vs_creature_type_rejects_an_unknown_damage_or_creature_type() {
        let bad_damage = parse_trait("bonus 3d6 sparkly vs dragon").unwrap_err();
        assert!(bad_damage.contains("sparkly"), "{bad_damage}");

        let bad_creature = parse_trait("bonus 3d6 piercing vs beholder-kin").unwrap_err();
        assert!(bad_creature.contains("beholder-kin"), "{bad_creature}");

        let missing_vs = parse_trait("bonus 3d6 piercing dragon").unwrap_err();
        assert!(missing_vs.contains("vs"), "{missing_vs}");
    }

    /// The four generic item-passive traits ARCH-05's spellcasting profile and
    /// the existing AC/saves/resistance fields make possible: none of them
    /// are riders, and a creature can carry any subset of them at once,
    /// exactly the way real items grant different subsets of these.
    #[test]
    fn stat_boost_traits_stack_directly_onto_the_creature_and_combine() {
        let text = "
creature: x
hp: 10
ac: 14
saves: str +1, dex +1, con +1, int +1, wis +1, cha +1
resist: cold
trait: ac 2
trait: ac bonus 1
trait: saves 1
trait: resistance fire, radiant
";
        let c = &parse(text).unwrap()[0];
        // Two separate AC-granting items stack: 14 base + 2 + 1.
        assert_eq!(c.ac, 17);
        // The flat saves trait adds on top of the stat block's own +1 to
        // every ability, not just one.
        for ability in [
            Ability::Str,
            Ability::Dex,
            Ability::Con,
            Ability::Int,
            Ability::Wis,
            Ability::Cha,
        ] {
            assert_eq!(c.save(ability), 2, "{ability:?} should be +1 base, +1 item");
        }
        // The trait-granted resistances sit alongside the stat block's own
        // `resist:` list rather than replacing it.
        assert_eq!(c.reduction(DamageKind::Cold), Reduction::Resistant);
        assert_eq!(c.reduction(DamageKind::Fire), Reduction::Resistant);
        assert_eq!(c.reduction(DamageKind::Radiant), Reduction::Resistant);
        assert_eq!(c.reduction(DamageKind::Acid), Reduction::Normal);
    }

    /// A spell attack/DC bonus needs an existing spellcasting profile to add
    /// itself to - there is nothing reasonable to do with "add 2 to a spell
    /// attack bonus" on a creature that does not cast spells, so this is
    /// reported rather than silently dropped.
    #[test]
    fn a_spell_bonus_trait_without_a_spellcasting_profile_is_an_error() {
        let text = "
creature: x
hp: 10
trait: spell 2
";
        let err = parse(text).expect_err("no spellcasting profile to add the bonus to");
        assert!(err.message.contains("spellcasting"), "{}", err.message);
    }

    /// A malformed or unrecognised trait keyword is a parse error rather than
    /// something silently ignored.
    #[test]
    fn an_unknown_trait_keyword_is_rejected() {
        let text = "
creature: x
hp: 10
trait: flight 60
";
        assert!(parse(text).is_err());
    }

    #[test]
    fn downgrade_immunity_trait_rejects_garbage() {
        let unknown_damage = parse_trait_external("downgrade immunity sparkly damage").unwrap_err();
        assert!(unknown_damage.contains("sparkly"), "{unknown_damage}");

        let unknown_condition =
            parse_trait_external("downgrade immunity confused condition").unwrap_err();
        assert!(
            unknown_condition.contains("confused"),
            "{unknown_condition}"
        );

        let missing_immunity = parse_trait_external("downgrade poison damage").unwrap_err();
        assert!(missing_immunity.contains("immunity"), "{missing_immunity}");

        let empty = parse_trait_external("downgrade immunity").unwrap_err();
        assert!(empty.contains("at least one"), "{empty}");

        let bad_type = parse_trait_external("downgrade immunity poison sparkly").unwrap_err();
        assert!(bad_type.contains("sparkly"), "{bad_type}");
    }

    /// The generic condition-triggered weapon damage buff (ITM-06): parsed
    /// entirely from the trait string, with no item or poison name anywhere
    /// near the parser.
    #[test]
    fn condition_triggered_weapon_damage_trait_parses() {
        let text = "
creature: x
hp: 10
trait: empower weapon 2d6 poison on poisoned
";
        let c = &parse(text).unwrap()[0];
        assert_eq!(
            c.riders,
            vec![Rider::ConditionTriggeredWeaponDamage {
                trigger: Condition::Poisoned,
                dice_count: 2,
                dice_sides: 6,
                bonus: 0,
                damage_kind: DamageKind::Poison,
            }]
        );
    }

    #[test]
    fn condition_triggered_weapon_damage_trait_rejects_garbage() {
        let bad_head = parse_trait_external("empower 2d6 poison on poisoned").unwrap_err();
        assert!(bad_head.contains("weapon"), "{bad_head}");

        let bad_damage =
            parse_trait_external("empower weapon 2d6 sparkly on poisoned").unwrap_err();
        assert!(bad_damage.contains("sparkly"), "{bad_damage}");

        let missing_on = parse_trait_external("empower weapon 2d6 poison poisoned").unwrap_err();
        assert!(missing_on.contains("on"), "{missing_on}");

        let bad_condition =
            parse_trait_external("empower weapon 2d6 poison on confused").unwrap_err();
        assert!(bad_condition.contains("confused"), "{bad_condition}");
    }

    /// The generic consumable injury poison trait (ITM-06): a save-forcing
    /// hit that, on a failure, burdens a second ability's future saves with
    /// disadvantage - fully parameterized, with no poison's name in the
    /// parser either.
    #[test]
    fn injury_poison_trait_parses_with_and_without_a_duration() {
        let default_duration = parse_trait_external("injury poison con dc 13 disadvantage str")
            .expect("parses without an explicit duration");
        assert_eq!(
            default_duration,
            TraitEffect::Rider(Rider::InjuryPoison {
                ability: Ability::Con,
                dc: 13,
                debuffed_ability: Ability::Str,
                condition: None,
                duration: Duration::ApplierTurn,
            })
        );

        let until_victim =
            parse_trait_external("injury poison con dc 13 disadvantage str until victim")
                .expect("parses with an explicit duration");
        assert_eq!(
            until_victim,
            TraitEffect::Rider(Rider::InjuryPoison {
                ability: Ability::Con,
                dc: 13,
                debuffed_ability: Ability::Str,
                condition: None,
                duration: Duration::VictimTurn,
            })
        );

        // A poison that also leaves its victim Poisoned, for an hour.
        let poisoning =
            parse_trait_external("injury poison con dc 17 poisoned disadvantage wis for 1 hour")
                .expect("parses with a condition and a fixed duration");
        assert_eq!(
            poisoning,
            TraitEffect::Rider(Rider::InjuryPoison {
                ability: Ability::Con,
                dc: 17,
                debuffed_ability: Ability::Wis,
                condition: Some(Condition::Poisoned),
                duration: Duration::Rounds(600),
            })
        );
    }

    #[test]
    fn injury_poison_trait_rejects_garbage() {
        let bad_head = parse_trait_external("injury con dc 13 disadvantage str").unwrap_err();
        assert!(bad_head.contains("poison"), "{bad_head}");

        let bad_ability =
            parse_trait_external("injury poison sparkly dc 13 disadvantage str").unwrap_err();
        assert!(bad_ability.contains("ability"), "{bad_ability}");

        let missing_dc = parse_trait_external("injury poison con 13 disadvantage str").unwrap_err();
        assert!(missing_dc.contains("dc"), "{missing_dc}");

        let missing_disadvantage = parse_trait_external("injury poison con dc 13 str").unwrap_err();
        assert!(
            missing_disadvantage.contains("disadvantage"),
            "{missing_disadvantage}"
        );

        let bad_duration =
            parse_trait_external("injury poison con dc 13 disadvantage str until nobody")
                .unwrap_err();
        assert!(bad_duration.contains("until"), "{bad_duration}");
    }

    /// The reaction traits, and condition immunities.
    #[test]
    fn reaction_traits_and_condition_immunities_parse() {
        let text = "
creature: x
hp: 20
trait: halve attack damage
trait: reaction ac 5 vs ranged weapon
trait: reaction ac 5
condition immune: poisoned, prone
";
        let c = &parse(text).unwrap()[0];
        assert_eq!(
            c.riders,
            vec![
                Rider::HalveAttackDamage,
                Rider::ReactionOnTargeted {
                    trigger: AttackTrigger::RangedWeaponAttack,
                    ac_bonus: 5,
                    lasting: false,
                },
                Rider::ReactionOnTargeted {
                    trigger: AttackTrigger::AnyAttack,
                    ac_bonus: 5,
                    lasting: false,
                },
            ]
        );
        assert_eq!(
            c.condition_immunities,
            vec![Condition::Poisoned, Condition::Prone]
        );
        assert_eq!(
            parse_trait("uncanny dodge").unwrap(),
            TraitEffect::Rider(Rider::HalveAttackDamage)
        );
        assert_eq!(
            parse_trait("extra damage applies to spell attacks").unwrap(),
            TraitEffect::Rider(Rider::ExtraDamageAppliesToSpellAttacks)
        );
        assert!(parse_trait("extra damage everywhere").is_err());
        assert!(parse_trait("reaction ac 5 vs sword").is_err());
        assert!(parse_trait("reaction speed 5").is_err());
    }

    #[test]
    fn a_damage_threshold_parses_with_and_without_its_crack_and_weak_spot() {
        let rider = |text: &str| match parse_trait(text).unwrap() {
            TraitEffect::Rider(r) => r,
            other => panic!("expected a rider, got {other:?}"),
        };
        assert_eq!(
            rider("damage threshold 15"),
            Rider::DamageThreshold {
                threshold: 15,
                cracks: false,
                weak_spot_resists: vec![],
            }
        );
        assert_eq!(
            rider("damage threshold 51 cracks weak spot resists bludgeoning, piercing, slashing"),
            Rider::DamageThreshold {
                threshold: 51,
                cracks: true,
                weak_spot_resists: vec![
                    DamageKind::Bludgeoning,
                    DamageKind::Piercing,
                    DamageKind::Slashing
                ],
            }
        );
        assert_eq!(
            rider("damage threshold 30 weak spot resist fire"),
            Rider::DamageThreshold {
                threshold: 30,
                cracks: false,
                weak_spot_resists: vec![DamageKind::Fire],
            }
        );
        assert!(parse_trait("damage threshold 0").is_err());
        assert!(parse_trait("damage threshold 20 shatters").is_err());
        assert!(parse_trait("damage threshold 20 weak spot resists").is_err());
        assert!(parse_trait("damage 20").is_err());
    }

    #[test]
    fn a_gullet_parses_what_it_digests_and_when_it_gives_up() {
        assert_eq!(
            parse_trait("digest 8d6 acid, 4d6 cold").unwrap(),
            TraitEffect::Rider(Rider::Digestion {
                damage: vec![
                    DamageRoll::new(8, 6, 0, DamageKind::Acid),
                    DamageRoll::new(4, 6, 0, DamageKind::Cold)
                ],
            })
        );
        assert_eq!(
            parse_trait("regurgitate 60 con dc 22").unwrap(),
            TraitEffect::Rider(Rider::Regurgitate {
                threshold: 60,
                ability: Ability::Con,
                dc: 22,
            })
        );
        assert!(parse_trait("digest lots").is_err());
        assert!(parse_trait("regurgitate 60 con 22").is_err());
        assert!(parse_trait("regurgitate 60 con dc 22 prone").is_err());
    }

    #[test]
    fn ignore_resistance_parses_damage_types() {
        assert_eq!(
            parse_trait("ignore resistance fire").unwrap(),
            TraitEffect::Rider(Rider::IgnoreResistance {
                kinds: vec![DamageKind::Fire]
            })
        );
        assert_eq!(
            parse_trait("ignores resistance piercing, slashing").unwrap(),
            TraitEffect::Rider(Rider::IgnoreResistance {
                kinds: vec![DamageKind::Piercing, DamageKind::Slashing]
            })
        );
        assert!(parse_trait("ignore resistance").is_err());
        assert!(parse_trait("ignore immunity fire").is_err());
    }
}
