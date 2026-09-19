//! Formats and prints the parsed creature sheet.

use crucible_core::creature::{Creature, Effect, Move, Rider, Uses};

/// Strip copy numbers like "Hero 3" back to "Hero", so resizing does not stack numbers.
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
        ];
        for (label, moves) in groups {
            for m in moves.iter() {
                println!(
                    "    {label:<10} {:<24} {:<46} {:>5.1} avg vs {}",
                    m.name,
                    describe(c, m),
                    m.effect.mean_damage(against),
                    strip_number(&against.name)
                );
            }
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
        println!();
    }
}

pub fn describe(owner: &Creature, m: &Move) -> String {
    let mut bits = vec![body(&m.effect)];
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
    for rider in &m.riders {
        if let Rider::SaveOrCondition {
            ability,
            dc,
            condition,
            cost,
            ..
        } = rider
        {
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
            bits.push(format!(
                "{} on {} dc {dc}{paid}",
                condition.name(),
                ability.name()
            ));
        }
    }
    bits.join(", ")
}

pub fn body(effect: &Effect) -> String {
    match effect {
        Effect::Strikes { strike, count } => format!("{count}x at {:+}", strike.to_hit),
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
        Effect::Sequence(parts) => parts.iter().map(body).collect::<Vec<_>>().join(" + "),
    }
}

pub fn describe_trait(rider: &Rider) -> String {
    match rider {
        Rider::NothingOnSuccess { ability } => {
            format!("evasion: a made {} save takes nothing", ability.name())
        }
        Rider::AlwaysSucceed { uses } => format!("legendary resistance {uses}/fight"),
        Rider::ReduceDamage {
            kinds,
            roll,
            per_round,
        } => format!(
            "reduce {} damage by {}d{}{:+} ({per_round}/round)",
            kinds.iter().map(|k| k.name()).collect::<Vec<_>>().join("/"),
            roll.count,
            roll.sides,
            roll.bonus
        ),
        Rider::SaveOrCondition {
            ability,
            dc,
            condition,
            ..
        } => format!(
            "{} on a failed {} dc {dc}",
            condition.name(),
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
            per_round,
            ..
        } => format!("reaction: +{ac_bonus} ac vs one targeting attack ({per_round}/round)"),
    }
}
