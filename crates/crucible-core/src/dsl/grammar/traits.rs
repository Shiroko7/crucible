//! Trait phrases: an always-on or reactive modifier (a [`crate::creature::Rider`]),
//! or a flat passive stat bonus.

use crate::creature::{AttackTrigger, Creature, Rider};
use crate::dsl::grammar::lex::{
    arg, parse_damage, parse_damage_kinds, parse_dice, trailing_number,
};
use crate::dsl::grammar::{count, number, parse_duration};
use crate::rules::{Ability, Condition, CreatureType, DamageKind, DamageRoll, Reduction};

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
        // `legendary resistance 3`, or the mechanism's own name.
        "legendary" | "always" => {
            let n = words
                .iter()
                .rev()
                .find_map(|w| count(w).ok())
                .ok_or_else(|| format!("expected a number of uses in `{value}`"))?;
            Ok(TraitEffect::Rider(Rider::AlwaysSucceed { uses: n }))
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
        // `reaction ac 5 [vs ranged weapon]` - a reaction that raises AC
        // against one attack roll, optionally only a ranged weapon one.
        "reaction" => {
            if !arg(&words, 1, value)?.eq_ignore_ascii_case("ac") {
                return Err(format!("expected `reaction ac <n> ...`, got `{value}`"));
            }
            let ac_bonus = number(arg(&words, 2, value)?)?;
            let trigger = match words.get(3).map(|w| w.to_ascii_lowercase()) {
                None => AttackTrigger::AnyAttack,
                Some(w) if w == "vs" => {
                    let rest = words[4..].join(" ").to_ascii_lowercase();
                    match rest.as_str() {
                        "any" | "any attack" => AttackTrigger::AnyAttack,
                        "ranged weapon" | "ranged weapon attack" | "ranged weapon attacks" => {
                            AttackTrigger::RangedWeaponAttack
                        }
                        other => return Err(format!("`vs {other}` is not an attack trigger")),
                    }
                }
                Some(other) => return Err(format!("unexpected `{other}` in `{value}`")),
            };
            Ok(TraitEffect::Rider(Rider::ReactionOnTargeted {
                trigger,
                ac_bonus,
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
        "resistance" | "resist" => {
            let kinds = parse_damage_kinds(&words[1..].join(" "), value)?;
            if kinds.is_empty() {
                return Err(format!("`{value}` needs at least one damage type"));
            }
            Ok(TraitEffect::Resistance(kinds))
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
            "expected `bonus NdM <damage type> vs <creature type>`, got `{clause}`"
        ));
    }
    let type_word = arg(words, 4, clause)?;
    let creature_type = CreatureType::parse(type_word)
        .ok_or_else(|| format!("unknown creature type `{type_word}` in `{clause}`"))?;
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
    use crate::rules::Duration;

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
            .any(|r| matches!(r, Rider::AlwaysSucceed { uses: 3 })));
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
                    ac_bonus: 5
                },
                Rider::ReactionOnTargeted {
                    trigger: AttackTrigger::AnyAttack,
                    ac_bonus: 5
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
}
