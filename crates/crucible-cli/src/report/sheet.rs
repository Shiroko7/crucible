//! Formats and prints the parsed creature sheet.

use crucible_core::creature::{
    AttackTrigger, Boon, Creature, Effect, Move, MoveKind, Reach, ReactionTrigger, Requirement,
    Rider, Uses,
};
use crucible_core::rules::DamageRoll;
/// Strip copy numbers like "Hero 3" back to "Hero", so resizing does not stack
/// numbers.
pub fn strip_number(name: &str) -> &str {
    match name.rsplit_once(' ') {
        Some((head, tail)) if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => head,
        _ => name,
    }
}

/// What was read, in the tool's own words.
pub fn print_sheet(roster: &[&Creature]) {
    let mut shown: Vec<&str> = Vec::new();
    for c in roster {
        let stem = strip_number(&c.name);
        if shown.contains(&stem) {
            continue; // identical copies need printing once
        }
        shown.push(stem);
        let count = roster
            .iter()
            .filter(|o| strip_number(&o.name) == stem)
            .count();
        let against = roster
            .iter()
            .find(|o| o.team != c.team)
            .copied()
            .unwrap_or(c);
        let multiple = if count > 1 {
            format!(" (x{count})")
        } else {
            String::new()
        };
        println!(
            "{stem}{multiple}  -  side {}, AC {}, {} hp, initiative {:+}",
            if c.team == 0 { "A" } else { "B" },
            c.ac,
            c.hp,
            c.initiative
        );
        let groups = [
            ("action", &c.actions),
            ("bonus", &c.bonus_actions),
            ("legendary", &c.legendary),
            ("aura", &c.auras),
        ];
        let print_move = |label: &str, m: &Move, extra: String| {
            // A move waiting on something that is not there yet - a double
            // still to be called up, a blade still to be lit - has no value
            // to print, and printing minus infinity for it would be noise.
            let value = crucible_core::sim::expected_damage(c, m, against);
            let against_name = strip_number(&against.name);
            let worth = match value.is_finite() {
                true => format!("{value:>5.1} avg vs {against_name}"),
                false => format!("{:>5} {}", "-", waiting_on(m)),
            };
            println!(
                "    {label:<10} {:<24} {:<46} {worth}",
                m.name,
                format!("{extra}{}", describe(c, m)),
            );
        };
        for (label, moves) in groups {
            for m in moves.iter() {
                print_move(label, m, String::new());
            }
        }
        for r in &c.reactions {
            let when = match r.trigger {
                ReactionTrigger::EnemyGains(condition) => {
                    format!("when enemy {}: ", condition.name())
                }
                ReactionTrigger::Breached => "when breached: ".to_string(),
            };
            print_move("reaction", &r.action, when);
        }
        if c.reactions_per_round > 1 {
            println!(
                "    {:<10} {} a round, one per turn",
                "reactions", c.reactions_per_round
            );
        }
        if c.legendary_uses > 0 {
            println!(
                "    {:<10} {} a round, one per enemy turn",
                "legendary", c.legendary_uses
            );
        }
        for r in &c.resources {
            println!("    {:<10} {} {}", "resource", r.max, r.name);
        }
        for r in &c.riders {
            println!("    {:<10} {}", "trait", describe_trait(r));
        }
        for (kind, reduction) in &c.reductions {
            println!("    {:<10} {} {:?}", "damage", kind.name(), reduction);
        }
        if !c.condition_immunities.is_empty() {
            let names: Vec<&str> = c.condition_immunities.iter().map(|c| c.name()).collect();
            println!("    {:<10} {}", "immune", names.join(", "));
        }
        if c.mouth {
            println!(
                "    {:<10} enemies stand at its mouth, beside its body or out in front; it moves by {}{}",
                "mouth",
                c.tactic.name(),
                if c.difficult_terrain {
                    ", and they move one place a turn"
                } else {
                    ""
                }
            );
        }
        println!();
    }
}

pub fn describe(owner: &Creature, m: &Move) -> String {
    let mut bits = vec![body(owner, &m.effect)];
    match m.kind {
        MoveKind::Standard => {}
        MoveKind::Spell => bits.push("spell".to_string()),
        MoveKind::MagicItem => bits.push("magic item".to_string()),
        MoveKind::ObjectUse => bits.push("object".to_string()),
    }
    if let Some(level) = m.spell_slot_level {
        bits.push(format!("level {level} slot"));
    }
    if m.concentration {
        bits.push("concentration".to_string());
    }
    // Only when the stat block said: an undeclared spell is read as speaking,
    // and printing "V" on every one of them would be inventing detail.
    if let Some(components) = m.components {
        bits.push(components.name());
    }
    if m.legendary_cost > 1 {
        bits.push(format!("{} legendary actions", m.legendary_cost));
    }
    if m.reach != Reach::Any {
        bits.push(format!("reach {}", m.reach.name()));
    }
    match m.uses {
        Uses::Unlimited => {}
        Uses::Limited(n) => bits.push(format!("{n} uses")),
        Uses::Recharge(n) => bits.push(format!("recharge {n}-6")),
    }
    if let Some(cost) = m.cost {
        bits.push(format!(
            "{} {}",
            cost.amount,
            owner
                .resources
                .get(cost.resource)
                .map_or("?", |r| r.name.as_str())
        ));
    }
    bits.extend(on_hit(owner, &m.riders));
    bits.join(", ")
}

/// What a move's on-hit riders do, in the sheet's words.
fn on_hit(owner: &Creature, riders: &[Rider]) -> Vec<String> {
    let mut bits = Vec::new();
    for rider in riders {
        if let Rider::BonusDamageVsCreatureType { .. }
        | Rider::ConditionOnHit { .. }
        | Rider::Swallow { .. } = rider
        {
            bits.push(describe_trait(rider));
        }
        if let Rider::SaveOrCondition {
            ability,
            dc,
            conditions,
            cost,
            ..
        } = rider
        {
            let condition = conditions
                .iter()
                .map(|c| c.name())
                .collect::<Vec<_>>()
                .join(" and ");
            let paid = cost.map_or(String::new(), |c| {
                format!(
                    ", {} {}",
                    c.amount,
                    owner
                        .resources
                        .get(c.resource)
                        .map_or("?", |r| r.name.as_str())
                )
            });
            bits.push(format!("{condition} on {} dc {dc}{paid}", ability.name()));
        }
    }
    bits
}

pub fn body(owner: &Creature, effect: &Effect) -> String {
    match effect {
        Effect::Strikes { strike, count } => format!(
            "{count}x {}{} at {:+}",
            if strike.kind.ranged {
                "ranged"
            } else {
                "melee"
            },
            match (strike.kind.weapon, strike.kind.spell) {
                (true, true) => " weapon+spell",
                (false, true) => " spell",
                _ => "",
            },
            strike.to_hit
        ),
        Effect::Save(save) => format!(
            "{} save dc {}{}{}",
            save.ability.name(),
            save.dc,
            if save.half_on_success { " half" } else { "" },
            match save.max_targets {
                Some(n) => format!(" {n} targets"),
                None => " all".to_string(),
            }
        ),
        Effect::Stance { condition } => condition.name().to_string(),
        Effect::Heal(roll) => format!("heal {}d{}{:+}", roll.count, roll.sides, roll.bonus),
        Effect::AutoHit { damage } => format!("{}x auto-hit", damage.len()),
        Effect::Buff { max_targets, .. } => match max_targets {
            Some(n) => format!("buff {n} targets"),
            None => "buff all".to_string(),
        },
        Effect::SaveOrModifier {
            ability,
            max_targets,
            ..
        } => format!(
            "{} save or debuff{}",
            ability.name(),
            match max_targets {
                Some(n) => format!(" {n} targets"),
                None => " all".to_string(),
            }
        ),
        Effect::HarmSwallowed { damage } => format!("{} to all swallowed", damage_list(damage)),
        Effect::Afflict { condition, .. } => format!("{} - no save", condition.name()),
        Effect::Boon { which, .. } => owner
            .boons
            .get(*which)
            .map_or_else(|| "a boon".to_string(), describe_boon),
        Effect::TempHp(roll) => format!("{}d{}{:+} temp hp", roll.count, roll.sides, roll.bonus),
        // Described by what each enemy actually meets, since that - not the
        // raising - is the whole of what an aura is.
        Effect::Aura { which, .. } => owner.lasting_auras.get(*which).map_or_else(
            || "an aura".to_string(),
            |aura| format!("aura: {}", body(owner, &aura.effect)),
        ),
        Effect::Summon { which } => owner.summons.get(*which).map_or_else(
            || "a summon".to_string(),
            |summon| {
                format!(
                    "summons {} (ac {}, {} hp)",
                    summon.name, summon.ac, summon.hp
                )
            },
        ),
        Effect::Sequence(parts) => parts
            .iter()
            .map(|p| body(owner, p))
            .collect::<Vec<_>>()
            .join(" + "),
        Effect::Part {
            effect,
            riders,
            reach,
        } => {
            let mut notes = on_hit(owner, riders);
            if *reach != Reach::Any {
                notes.insert(0, format!("reach {}", reach.name()));
            }
            format!("{} ({})", body(owner, effect), notes.join(", "))
        }
    }
}

/// What a move is waiting for before it can be taken at all - see
/// [`Requirement`].
fn waiting_on(m: &Move) -> String {
    match m.requires {
        Some(Requirement::Summon { count, .. }) => match count {
            1 => "needs one summoned".to_string(),
            n => format!("needs {n} summoned"),
        },
        Some(Requirement::SummonRoom { max, .. }) => format!("at most {max} summoned"),
        Some(Requirement::Boon { .. }) => "needs its boon active".to_string(),
        None => "cannot land from here".to_string(),
    }
}

/// What a lasting boon does while it is up, for the sheet: the extra damage
/// it puts on a hit and what it lets its holder shrug off.
pub fn describe_boon(boon: &Boon) -> String {
    let mut parts = Vec::new();
    if let Some(damage) = boon.damage {
        parts.push(format!(
            "+{}d{}{:+} {}{}",
            damage.count,
            damage.sides,
            damage.bonus,
            damage.kind.name(),
            match (&boon.weapon, boon.weapon_only) {
                (Some(weapon), _) => format!(" with {weapon}"),
                (None, true) => " on weapon hits".to_string(),
                (None, false) => String::new(),
            }
        ));
    }
    if !boon.resist.is_empty() {
        parts.push(format!(
            "resist {}",
            boon.resist
                .iter()
                .map(|k| k.name())
                .collect::<Vec<_>>()
                .join("/")
        ));
    }
    format!("{}: {}", boon.name, parts.join(", "))
}

pub fn describe_trait(rider: &Rider) -> String {
    match rider {
        Rider::NothingOnSuccess { ability } => {
            format!("evasion: a made {} save takes nothing", ability.name())
        }
        Rider::AlwaysSucceed {
            uses,
            ability,
            reaction,
        } => format!(
            "{}{uses}/fight{}",
            match ability {
                Some(a) => format!("a failed {} save succeeds instead, ", a.name()),
                None => "legendary resistance ".to_string(),
            },
            if *reaction { " (reaction)" } else { "" }
        ),
        Rider::ReduceDamage { kinds, roll } => format!(
            "reaction: reduce {} damage by {}d{}{:+}",
            kinds.iter().map(|k| k.name()).collect::<Vec<_>>().join("/"),
            roll.count,
            roll.sides,
            roll.bonus
        ),
        Rider::HalveAttackDamage => "reaction: halve one attack's damage".to_string(),
        Rider::SaveOrCondition {
            ability,
            dc,
            conditions,
            ..
        } => format!(
            "{} on a failed {} dc {dc}",
            conditions
                .iter()
                .map(|c| c.name())
                .collect::<Vec<_>>()
                .join(" and "),
            ability.name()
        ),
        Rider::ConditionalExtraDamage {
            dice_count,
            dice_sides,
            once_per_turn,
        } => format!(
            "+{dice_count}d{dice_sides} on advantage or an adjacent ally, not disadvantage{}",
            if *once_per_turn { " (once/turn)" } else { "" }
        ),
        Rider::ReactionOnTargeted {
            ac_bonus,
            trigger,
            lasting,
        } => format!(
            "reaction: +{ac_bonus} ac vs {} {}{}",
            if *lasting { "every" } else { "one" },
            match trigger {
                AttackTrigger::AnyAttack => "attack",
                AttackTrigger::RangedWeaponAttack => "ranged weapon attack",
                AttackTrigger::MeleeAttack => "melee attack",
            },
            if *lasting { " until its next turn" } else { "" }
        ),
        Rider::CunningStrike { dc } => {
            format!("cunning strike: spend sneak attack dice dc {dc}")
        }
        Rider::BonusDamageVsCreatureType {
            dice_count,
            dice_sides,
            bonus,
            damage_kind,
            creature_type,
        } => format!(
            "+{dice_count}d{dice_sides}{bonus:+} {} vs {}",
            damage_kind.name(),
            creature_type.name()
        ),
        Rider::OncePerTurnDamage {
            dice_count,
            dice_sides,
            bonus,
            damage_kind,
        } => format!(
            "+{dice_count}d{dice_sides}{bonus:+} {} on its first hit each of its turns",
            damage_kind.name()
        ),
        Rider::BonusDamageVsQuarry {
            dice_count,
            dice_sides,
            bonus,
            damage_kind,
        } => format!(
            "+{dice_count}d{dice_sides}{bonus:+} {} against its marked quarry",
            damage_kind.name()
        ),
        Rider::CunningStrikeTrip => {
            "cunning strike: trip (1d6, dex save vs cunning strike dc or prone)".to_string()
        }
        Rider::CunningStrikeWithdraw => {
            "cunning strike: withdraw (1d6, move half speed without opportunity attacks)"
                .to_string()
        }
        Rider::DowngradeImmunity { damage, condition } => {
            let parts: Vec<String> = damage
                .iter()
                .map(|k| format!("{} damage immunity to resistance", k.name()))
                .chain(
                    condition
                        .iter()
                        .map(|c| format!("{} immunity to a save with advantage", c.name())),
                )
                .collect();
            format!("downgrade {}", parts.join(" and "))
        }
        Rider::ExtraDamageAppliesToSpellAttacks => {
            "conditional extra damage also applies to spell attacks".to_string()
        }
        Rider::ConditionTriggeredWeaponDamage {
            trigger,
            dice_count,
            dice_sides,
            bonus,
            damage_kind,
        } => format!(
            "after inflicting {}: weapon attacks deal +{dice_count}d{dice_sides}{bonus:+} {} for the rest of the fight",
            trigger.name(),
            damage_kind.name()
        ),
        Rider::InjuryPoison {
            ability,
            dc,
            debuffed_ability,
            condition,
            ..
        } => format!(
            "injury poison (1 dose): next weapon hit forces {} dc {dc}, fail gives {}disadvantage on {} saves",
            ability.name(),
            condition.map_or(String::new(), |c| format!("{} and ", c.name())),
            debuffed_ability.name()
        ),
        Rider::ConditionOnHit { condition, .. } => {
            format!("{} on hit (no save)", condition.name())
        }
        Rider::DamageThreshold {
            threshold,
            cracks,
            weak_spot_resists,
        } => {
            let mut text = format!("damage threshold {threshold}: less in one go does nothing");
            if *cracks {
                text.push_str(", a breach cracks it open until the round ends");
            }
            if !weak_spot_resists.is_empty() {
                text.push_str(&format!(
                    ", weak spot resists {}",
                    weak_spot_resists
                        .iter()
                        .map(|k| k.name())
                        .collect::<Vec<_>>()
                        .join("/")
                ));
            }
            text
        }
        Rider::Swallow { max_size } => format!("swallows {} or smaller on hit", max_size.name()),
        Rider::Digestion { damage } => format!("digests {} a turn", damage_list(damage)),
        Rider::Regurgitate {
            threshold,
            ability,
            dc,
        } => format!(
            "{threshold}+ damage from inside in a turn: {} dc {dc} or regurgitate",
            ability.name()
        ),
    }
}

fn damage_list(damage: &[DamageRoll]) -> String {
    damage
        .iter()
        .map(|r| format!("{}d{}{:+} {}", r.count, r.sides, r.bonus, r.kind.name()))
        .collect::<Vec<_>>()
        .join(", ")
}
