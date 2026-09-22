//! Resolving a move: each of its effects - strikes, saves, heals, stances,
//! buffs - applied to its targets, and the damage that results.

use crate::creature::{Effect, Move, Reach, Rider};
use crate::prob::Rng;
use crate::rules::{apply_healing, Condition, Landed};
use crate::sim::fight::reactions::{ac_boost_reaction, react_to_hit};
use crate::sim::fight::saves::saving_throw;
use crate::sim::fight::threshold::{reducer, Shell};
use crate::sim::fight::{ActiveConcentration, Answer, ConcentrationEffect, Expiry, Fight};

impl<'a> Fight<'a> {
    /// Resolve one move and apply everything it does.
    ///
    /// A concentration move drops whatever the user was already maintaining
    /// before it does anything else - even a cast that lands on nobody still
    /// ends the old spell - and then, if it landed a condition on anyone,
    /// that becomes the new thing concentration maintains.
    pub(super) fn apply(
        &mut self,
        m: &Move,
        rng: &mut Rng,
        me: usize,
        target: usize,
        record: bool,
        line: &mut String,
    ) {
        if m.concentration {
            self.end_concentration(me);
        }
        let mut notes: Vec<String> = Vec::new();
        let mut landed: Vec<(usize, Condition)> = Vec::new();
        let mut concentration_effect: Option<ConcentrationEffect> = None;
        self.resolve(
            &m.effect,
            rng,
            me,
            target,
            &m.riders,
            m.reach,
            record,
            &mut notes,
            &mut landed,
            &mut concentration_effect,
        );
        if m.concentration {
            if let Some(effect) = concentration_effect {
                self.fighters[me].concentration = Some(ActiveConcentration { effect });
            } else if let Some(&(_, condition)) = landed.first() {
                self.fighters[me].concentration = Some(ActiveConcentration {
                    effect: ConcentrationEffect::Condition {
                        targets: landed.iter().map(|&(t, _)| t).collect(),
                        condition,
                    },
                });
            }
        }
        if !self.fighters[me].creature.reactions.is_empty() {
            self.react_to_landed(rng, me, &landed, record, &mut notes);
        }
        if record {
            if !line.is_empty() {
                line.push_str(" | ");
            }
            line.push_str(&m.name);
            if !notes.is_empty() {
                line.push_str(" (");
                line.push_str(&notes.join(", "));
                line.push(')');
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve(
        &mut self,
        effect: &Effect,
        rng: &mut Rng,
        me: usize,
        target: usize,
        move_riders: &[Rider],
        reach: Reach,
        record: bool,
        notes: &mut Vec<String>,
        landed_conditions: &mut Vec<(usize, Condition)>,
        concentration_effect: &mut Option<ConcentrationEffect>,
    ) {
        match effect {
            Effect::Strikes { strike, count } => {
                let mut current_target = target;
                // Ongoing attack-roll modifiers this attacker is carrying -
                // Bless, Bane. Cloned once: they do not change mid-move, and
                // `self.fighters` cannot stay borrowed here across the
                // mutable borrows the loop below takes for `current_target`.
                let modifiers = self.fighters[me].attack_modifiers.clone();
                let attacker = self.fighters[me].creature;
                self.lapse_on_attack(me);
                for _ in 0..*count {
                    if !self.fighters[current_target].alive()
                        || !self.in_reach(me, current_target, reach, Some(strike.kind))
                    {
                        // An aura's or a reaction's strike answers one
                        // creature; it is not redirected at another.
                        if self.sole_target.is_some() {
                            break;
                        }
                        let Some(new_target) = self
                            .pick_target_in(me, |f, i| f.in_reach(me, i, reach, Some(strike.kind)))
                        else {
                            break;
                        };
                        current_target = new_target;
                    }
                    let against = self.fighters[current_target].creature;
                    // Worked out per swing: a mark or Steady Aim is used up by
                    // the first roll it helps, so a multiattack's later swings
                    // roll without it.
                    let mode = self.attack_mode_consuming(strike.mode, me, current_target);
                    // Paralyzed: any hit against it is an automatic critical hit.
                    let force_crit = self.fighters[current_target].has(Condition::auto_crits);
                    let boost = ac_boost_reaction(&self.fighters[current_target], strike.kind);
                    let plan =
                        self.extra_damage_plan(me, current_target, strike, move_riders, mode, true);
                    // Aimed at an open weak spot when that is worth more than
                    // the shell - see `Fight::aimed_hit`.
                    let weak = self.through_weak_spot(me, current_target, Some(strike.kind))
                        && self
                            .aimed_hit(me, current_target, strike, mode, force_crit, &plan.rolls)
                            .1;
                    let (raw, landed, consumed) = strike.sample_reduced(
                        rng,
                        &reducer(attacker, against, weak),
                        against.ac,
                        mode,
                        force_crit,
                        &modifiers,
                        &plan.rolls,
                        boost.unwrap_or(0),
                        boost.is_some(),
                    );
                    if consumed {
                        self.fighters[current_target].reaction = false;
                    }
                    let (raw, answer) =
                        react_to_hit(rng, &mut self.fighters[current_target], strike, raw);
                    let (dealt, shell) = self.deal(rng, me, current_target, raw, weak);
                    if record {
                        let sneak = if plan.sneak_attack && landed != Landed::Miss {
                            " sneak"
                        } else {
                            ""
                        };
                        let shell_note = match shell {
                            Shell::Open => String::new(),
                            Shell::Absorbed => format!(" (shell took {raw})"),
                            Shell::Breached => " (breach)".to_string(),
                        };
                        notes.push(match (landed, answer) {
                            (Landed::Miss, _) if consumed => "miss (AC boosted)".to_string(),
                            (Landed::Miss, _) => "miss".to_string(),
                            (_, Some(Answer::Deflected(cut))) => {
                                format!("{dealt}{sneak} (deflected {cut}){shell_note}")
                            }
                            (_, Some(Answer::Halved)) => {
                                format!("{dealt}{sneak} (halved){shell_note}")
                            }
                            (Landed::Crit, None) => format!("{dealt} crit{sneak}{shell_note}"),
                            (Landed::Hit, None) => format!("{dealt}{sneak}{shell_note}"),
                        });
                    }
                    self.answer_breach(rng, me, current_target, shell, record, notes);
                    if landed != Landed::Miss {
                        if plan.sneak_attack {
                            self.fighters[me].sneak_attack_spent = true;
                        }
                        if let Some(choice) = plan.cunning {
                            self.resolve_cunning_strike(
                                rng,
                                me,
                                current_target,
                                choice,
                                record,
                                notes,
                                landed_conditions,
                            );
                        }
                        if strike.kind.weapon {
                            self.resolve_injury_poison(
                                rng,
                                me,
                                current_target,
                                record,
                                notes,
                                landed_conditions,
                            );
                        }
                        self.fire_on_hit(
                            rng,
                            me,
                            current_target,
                            move_riders,
                            record,
                            notes,
                            landed_conditions,
                        );
                    }
                }
            }
            Effect::Save(save) => {
                // Outside the zones around a creature with a mouth there is no
                // positioning, so an area effect catches every enemy up to its
                // target cap. Pessimistic, and stated as such.
                let mut caught: Vec<usize> = self
                    .caught(me, reach)
                    .into_iter()
                    // A type-restricted save (Hold Person's "humanoid") never
                    // catches anything else at all - not even a rolled save
                    // that then does nothing, the same way `max_targets` caps
                    // who is caught rather than who saves.
                    .filter(|&i| match &save.requires_type {
                        None => true,
                        Some(t) => self.fighters[i].creature.is_creature_type(t),
                    })
                    .collect();
                if let Some(max) = save.max_targets {
                    caught.truncate(max as usize);
                }
                let attacker = self.fighters[me].creature;
                for i in caught {
                    let against = self.fighters[i].creature;
                    let (conditions, advantage) = self.conditions_against(me, i, &save.on_failure);
                    if save.damage.is_empty()
                        && !save.on_failure.is_empty()
                        && conditions.is_empty()
                    {
                        // Immune to everything it could do: nothing to roll
                        // against, and no Legendary Resistance to waste on it.
                        if record {
                            notes.push(format!("{} immune", self.fighters[i].creature.name));
                        }
                        continue;
                    }
                    let (saved, resisted) =
                        saving_throw(&mut self.fighters, rng, i, save.ability, save.dc, advantage);
                    // Evasion is explicitly unavailable while Incapacitated.
                    let evasion = against.has_evasion(save.ability)
                        && !self.fighters[i].has(Condition::blocks_riders);
                    let weak = self.through_weak_spot(me, i, None);
                    let raw = save.sample_known_by(
                        rng,
                        &reducer(attacker, against, weak),
                        saved,
                        evasion,
                    );
                    let (dealt, shell) = self.deal(rng, me, i, raw, weak);
                    if !saved {
                        for &(condition, duration) in &conditions {
                            self.land_condition(me, i, condition, duration, landed_conditions);
                        }
                    }
                    if record {
                        let how = match (saved, resisted) {
                            (true, true) => "legendary resistance",
                            (true, false) => "saved",
                            _ => "failed",
                        };
                        let extra = if !saved && !conditions.is_empty() {
                            let names: Vec<&str> =
                                conditions.iter().map(|&(c, _)| c.name()).collect();
                            format!(" and {}", names.join(" and "))
                        } else {
                            String::new()
                        };
                        let shell_note = match shell {
                            Shell::Absorbed => format!(" (shell took {raw})"),
                            Shell::Breached => " (breach)".to_string(),
                            Shell::Open => String::new(),
                        };
                        notes.push(format!(
                            "{} {how} for {dealt}{shell_note}{extra}",
                            self.fighters[i].creature.name
                        ));
                    }
                    self.answer_breach(rng, me, i, shell, record, notes);
                }
            }
            Effect::Stance { condition } => {
                self.apply_condition(me, *condition, Expiry::TurnStart(me));
                landed_conditions.push((me, *condition));
                if record {
                    notes.push(condition.name().to_string());
                }
            }
            Effect::Heal(roll) => {
                // Healing Word, Cure Wounds, a potion: always aimed at the
                // user's own side, whoever `target` - the enemy the turn is
                // about - happens to be. See `Fight::heal_target`.
                let who = self.heal_target(me);
                let healed = roll.sample(rng);
                let f = &mut self.fighters[who];
                let (new_hp, revived) = if f.dead {
                    (f.hp, false)
                } else {
                    apply_healing(f.hp.max(0), f.creature.hp, healed)
                };
                f.hp = new_hp;
                if record {
                    let name = &f.creature.name;
                    notes.push(if revived {
                        format!("heals {name} {healed} (revives)")
                    } else {
                        format!("heals {name} {healed}")
                    });
                }
            }
            Effect::AutoHit { damage } => {
                // No attack roll and no save: every dart just lands.
                // Retargeting on a mid-resolution kill mirrors `Strikes` -
                // Magic Missile's darts do not stop because an earlier one
                // dropped the target.
                let attacker = self.fighters[me].creature;
                let mut current_target = target;
                for roll in damage {
                    if !self.fighters[current_target].alive()
                        || !self.in_reach(me, current_target, reach, None)
                    {
                        if self.sole_target.is_some() {
                            break;
                        }
                        let Some(new_target) =
                            self.pick_target_in(me, |f, i| f.in_reach(me, i, reach, None))
                        else {
                            break;
                        };
                        current_target = new_target;
                    }
                    let against = self.fighters[current_target].creature;
                    // A dart hits the creature, not a spot on it - unless it
                    // was loosed from inside.
                    let weak = self.through_weak_spot(me, current_target, None);
                    let raw = roll.sample(rng, false, reducer(attacker, against, weak)(roll.kind));
                    let (dealt, shell) = self.deal(rng, me, current_target, raw, weak);
                    if record {
                        notes.push(match shell {
                            Shell::Absorbed => format!("0 (shell took {raw})"),
                            _ => dealt.to_string(),
                        });
                    }
                    self.answer_breach(rng, me, current_target, shell, record, notes);
                }
            }
            Effect::HarmSwallowed { damage } => {
                self.harm_swallowed(rng, me, damage, record, notes);
            }
            Effect::Buff {
                attack_modifier,
                save_modifier,
                max_targets,
            } => {
                // No positioning, so "up to N of the user's own side" is the
                // user first - a caster overwhelmingly means to bless itself
                // - then fills the rest by roster order.
                let side = self.fighters[me].side;
                let mut caught: Vec<usize> = vec![me];
                caught.extend((0..self.fighters.len()).filter(|&i| {
                    i != me
                        && self.fighters[i].side == side
                        && self.fighters[i].alive()
                        && self.reaches(me, i)
                }));
                if let Some(max) = max_targets {
                    caught.truncate(*max as usize);
                }
                for &i in &caught {
                    self.fighters[i].attack_modifiers.push(*attack_modifier);
                    self.fighters[i].save_modifiers.push(*save_modifier);
                }
                if record {
                    let names: Vec<&str> = caught
                        .iter()
                        .map(|&i| self.fighters[i].creature.name.as_str())
                        .collect();
                    notes.push(format!("buffs {}", names.join(", ")));
                }
                *concentration_effect = Some(ConcentrationEffect::Modifiers {
                    targets: caught,
                    attack_modifier: *attack_modifier,
                    save_modifier: *save_modifier,
                });
            }
            Effect::SaveOrModifier {
                ability,
                attack_modifier,
                save_modifier,
                max_targets,
            } => {
                // Same pessimistic "every enemy up to the cap" reading as
                // Effect::Save - no positioning to choose among them.
                let mut caught = self.caught(me, reach);
                if let Some(max) = max_targets {
                    caught.truncate(*max as usize);
                }
                // The caster's own spell save DC, computed fresh from its
                // `SpellCastingProfile` rather than a number carried on the
                // move - see `Effect::SaveOrModifier`'s docs.
                let dc = self.fighters[me].creature.spell_save_dc().expect(
                    "a move using Effect::SaveOrModifier requires its caster to have a spellcasting profile",
                );
                let mut debuffed = Vec::new();
                for i in caught {
                    let (saved, resisted) =
                        saving_throw(&mut self.fighters, rng, i, *ability, dc, false);
                    if !saved {
                        self.fighters[i].attack_modifiers.push(*attack_modifier);
                        self.fighters[i].save_modifiers.push(*save_modifier);
                        debuffed.push(i);
                    }
                    if record {
                        let how = match (saved, resisted) {
                            (true, true) => "legendary resistance",
                            (true, false) => "saved",
                            _ => "failed",
                        };
                        notes.push(format!("{} {how}", self.fighters[i].creature.name));
                    }
                }
                *concentration_effect = Some(ConcentrationEffect::Modifiers {
                    targets: debuffed,
                    attack_modifier: *attack_modifier,
                    save_modifier: *save_modifier,
                });
            }
            Effect::Sequence(parts) => {
                for part in parts {
                    self.resolve(
                        part,
                        rng,
                        me,
                        target,
                        move_riders,
                        reach,
                        record,
                        notes,
                        landed_conditions,
                        concentration_effect,
                    );
                }
            }
            Effect::Part {
                effect,
                riders,
                reach: own,
            } => {
                let riders: Vec<Rider> = move_riders.iter().chain(riders).cloned().collect();
                self.resolve(
                    effect,
                    rng,
                    me,
                    target,
                    &riders,
                    own.within(reach),
                    record,
                    notes,
                    landed_conditions,
                    concentration_effect,
                );
            }
        }
    }

    /// Apply damage already run through resistance and reactions, then
    /// handle what it does to the target: dropping to 0 HP ends its
    /// concentration outright (no save offered, same as Incapacitated), and
    /// surviving damage forces the save that might end it anyway.
    ///
    /// Hit points stop at 0. Damage left over past 0 that reaches the
    /// creature's hit point maximum kills it outright - no healing brings
    /// that back - while anything less leaves a player character down but
    /// revivable.
    ///
    /// A swallower that drops lets go of everything it holds.
    pub(super) fn apply_damage(&mut self, rng: &mut Rng, target: usize, dealt: i32) {
        let f = &mut self.fighters[target];
        let before = f.hp;
        f.hp -= dealt;
        if f.hp <= 0 {
            if before > 0 && dealt - before >= f.creature.hp {
                f.dead = true;
            }
            f.hp = 0;
            self.end_concentration(target);
            self.release_all(target, false);
        } else if dealt > 0 {
            self.concentration_check(rng, target, dealt);
        }
    }

    /// Every enemy an area effect from `me` catches: all of them it can reach
    /// - or, while an aura or a reaction resolves, the one it answers.
    pub(super) fn caught(&self, me: usize, reach: Reach) -> Vec<usize> {
        let side = self.fighters[me].side;
        let candidates: Vec<usize> = match self.sole_target {
            Some(t) => vec![t],
            None => (0..self.fighters.len())
                .filter(|&i| self.fighters[i].side != side)
                .collect(),
        };
        candidates
            .into_iter()
            .filter(|&i| self.fighters[i].alive() && self.in_reach(me, i, reach, None))
            .collect()
    }

    /// A clamped-shut mouth opens to bite: whatever `me` is holding that
    /// lapses on an attack ends now. See [`Condition::lapses_on_attack`].
    pub(super) fn lapse_on_attack(&mut self, me: usize) {
        self.fighters[me]
            .conditions
            .retain(|&(c, _)| !c.lapses_on_attack());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Creature, SaveEffect, Strike};
    use crate::rules::{Ability, DamageKind, DamageRoll, Duration, HealRoll, Reduction};
    use crate::sim::fight::test_support::{fight_of, no_log, puncher, strike_once};
    use crate::sim::{run, run_teams, Budget, Policy, Side};

    /// Paralyzed adds one thing Stunned does not: a hit against it is an
    /// automatic critical. The paralyzer lands the condition with its action,
    /// then a bonus action against the same, now-paralyzed target should read
    /// as a crit even though the attack roll itself never approached one.
    #[test]
    fn paralyzed_turns_a_landed_bonus_action_hit_into_a_crit() {
        let mut paralyzer = puncher("paralyzer", 30, 200, 30, 0);
        paralyzer.initiative = 100;
        paralyzer.actions[0].riders.push(Rider::SaveOrCondition {
            ability: Ability::Con,
            dc: 99, // never saved, so the first hit always paralyzes
            conditions: vec![Condition::Paralyzed],
            duration: Duration::ApplierTurn,
            cost: None,
            once_per_turn: false,
        });
        // A second, separate strike so its own `force_crit` check runs after
        // the action above has already applied the condition.
        paralyzer.bonus_actions.push(Move::new(
            "Follow-up",
            Effect::Strikes {
                strike: Strike::new(30, vec![DamageRoll::new(1, 4, 0, DamageKind::Bludgeoning)]),
                count: 1,
            },
        ));

        let mut victim = puncher("victim", 1, 200, -100, 0);
        victim.initiative = -100;

        let mut rng = Rng::new(21);
        let mut log = Some(Vec::new());
        run(
            &mut rng,
            [&paralyzer, &victim],
            [Policy::Greedy; 2],
            1,
            &mut log,
        );
        let narration = log.unwrap().join("\n");
        assert!(
            narration.contains("crit"),
            "a hit against a paralyzed target must be an automatic crit:\n{narration}"
        );
    }

    /// An area effect catches every enemy, which is the whole reason a party
    /// cannot simply out-number a dragon.
    #[test]
    fn an_area_effect_hits_the_whole_other_side() {
        let mut breather = Creature::new("breather", 20, 500);
        breather.team = 0;
        breather.initiative = 100;
        breather.actions.push(Move::new(
            "Breath",
            Effect::Save(SaveEffect {
                ability: Ability::Dex,
                dc: 99,
                damage: vec![DamageRoll::new(0, 6, 10, DamageKind::Fire)],
                half_on_success: true,
                on_failure: vec![],
                max_targets: None,
                requires_type: None,
            }),
        ));
        let mut victim = Creature::new("victim", 10, 1_000);
        victim.team = 1;

        let roster: Vec<&Creature> = vec![&breather, &victim, &victim, &victim];
        let mut rng = Rng::new(12);
        let mut log = no_log();
        let o = run_teams(
            &mut rng,
            &roster,
            [Policy::Greedy; 2],
            1,
            Budget::default(),
            &mut log,
        );
        // Three victims, 10 damage each, in one round.
        assert_eq!(o.damage_dealt[0], 30);

        // With a cap, only that many are caught.
        let mut capped = breather.clone();
        if let Effect::Save(save) = &mut capped.actions[0].effect {
            save.max_targets = Some(2);
        }
        let roster: Vec<&Creature> = vec![&capped, &victim, &victim, &victim];
        let mut rng = Rng::new(12);
        let mut log = no_log();
        let o = run_teams(
            &mut rng,
            &roster,
            [Policy::Greedy; 2],
            1,
            Budget::default(),
            &mut log,
        );
        assert_eq!(o.damage_dealt[0], 20);
    }

    /// When a multiattack drops an enemy with strikes left, the remaining
    /// strikes should retarget rather than being discarded.
    #[test]
    fn strikes_retarget_when_a_target_drops_mid_turn() {
        let mut attacker = Creature::new("attacker", 10, 100);
        attacker.team = 0;
        attacker.initiative = 100;
        attacker.actions.push(Move::new(
            "Double Strike",
            Effect::Strikes {
                strike: Strike::new(30, vec![DamageRoll::new(0, 6, 10, DamageKind::Bludgeoning)]),
                count: 2,
            },
        ));

        let mut target1 = Creature::new("target1", 10, 5);
        target1.team = 1;
        let mut target2 = Creature::new("target2", 10, 50);
        target2.team = 1;

        let roster = [&attacker, &target1, &target2];
        let mut rng = Rng::new(42);
        let mut log = no_log();
        let o = run_teams(
            &mut rng,
            &roster,
            [Policy::Greedy; 2],
            1,
            Budget::default(),
            &mut log,
        );

        // 10 damage to target1 (dropping it from 5 to 0) + 10 damage to target2
        assert_eq!(o.damage_dealt[0], 20);
        assert_eq!(o.deaths[1], 1);
        assert_eq!(o.survivors[1], 1);
    }

    /// A heal goes to the healer's own side whoever the turn's target is,
    /// and a downed player character comes back up.
    #[test]
    fn heal_effect_revives_a_downed_ally() {
        let healer = puncher("healer", 10, 20, 5, 2);
        let mut downed = puncher("downed", 10, 30, 5, 2);
        downed.player_character = true;
        let enemy = puncher("enemy", 10, 30, 5, 2);
        let roster = [(&healer, Side::A), (&downed, Side::A), (&enemy, Side::B)];
        let mut rng = Rng::new(1);
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            1,
            Budget::default(),
            &mut None,
        );
        fight.fighters[1].hp = 0;

        // 1d4+3 is 4..=7: always enough to clear zero against a 30 hp max, so
        // the revive is deterministic without pinning the roll.
        let heal = Effect::Heal(HealRoll::new(1, 4, 3));
        let mut notes = Vec::new();
        // Aimed at the enemy (index 2), as every move's target is.
        fight.resolve(
            &heal,
            &mut rng,
            0,
            2,
            &[],
            crate::creature::Reach::Any,
            true,
            &mut notes,
            &mut Vec::new(),
            &mut None,
        );

        assert!((4..=7).contains(&fight.fighters[1].hp));
        assert_eq!(fight.fighters[2].hp, 30, "the enemy is never healed");
        assert!(
            notes.iter().any(|n| n.contains("revives")),
            "regaining hp from 0 should revive: {notes:?}"
        );
    }

    /// A monster at 0 is dead, and so is a player character dropped by a
    /// blow with its hit point maximum left over: no heal brings either
    /// back.
    #[test]
    fn the_dead_are_not_revived() {
        let healer = puncher("healer", 10, 20, 5, 2);
        let mut pc = puncher("pc", 10, 30, 5, 2);
        pc.player_character = true;
        let monster = puncher("monster", 10, 30, 5, 2);
        let enemy = puncher("enemy", 10, 30, 5, 2);
        let roster = [
            (&healer, Side::A),
            (&pc, Side::A),
            (&monster, Side::A),
            (&enemy, Side::B),
        ];
        let mut rng = Rng::new(1);
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            1,
            Budget::default(),
            &mut None,
        );
        // 10 hp left, then a 40-point hit: 30 over, a whole maximum.
        fight.fighters[1].hp = 10;
        fight.apply_damage(&mut rng, 1, 40);
        assert!(fight.fighters[1].dead);
        fight.fighters[2].hp = 0;

        assert!(!fight.fighters[1].can_revive());
        assert!(!fight.fighters[2].can_revive(), "a monster dies at 0");
        let heal = Effect::Heal(HealRoll::new(1, 4, 3));
        fight.resolve(
            &heal,
            &mut rng,
            0,
            3,
            &[],
            crate::creature::Reach::Any,
            false,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut None,
        );
        assert_eq!(fight.fighters[1].hp, 0);
        assert_eq!(fight.fighters[2].hp, 0);
    }

    #[test]
    fn heal_effect_clamps_at_max_hp_and_does_not_revive_the_merely_wounded() {
        let healer = puncher("healer", 10, 20, 5, 2);
        let wounded = puncher("wounded", 10, 10, 5, 2);
        let enemy = puncher("enemy", 10, 30, 5, 2);
        let roster = [(&healer, Side::A), (&wounded, Side::A), (&enemy, Side::B)];
        let mut rng = Rng::new(1);
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            1,
            Budget::default(),
            &mut None,
        );
        // Already above zero, and close enough to its own 10 hp max that
        // even the smallest roll (4) would overshoot it.
        fight.fighters[1].hp = 8;

        let heal = Effect::Heal(HealRoll::new(1, 4, 3));
        let mut notes = Vec::new();
        fight.resolve(
            &heal,
            &mut rng,
            0,
            2,
            &[],
            crate::creature::Reach::Any,
            true,
            &mut notes,
            &mut Vec::new(),
            &mut None,
        );

        assert_eq!(
            fight.fighters[1].hp, 10,
            "healing cannot push a creature past its own max hp"
        );
        assert!(
            notes.iter().all(|n| !n.contains("revives")),
            "was never down, so nothing to revive: {notes:?}"
        );
    }

    /// An attacker that downgrades poison immunity deals half, not nothing,
    /// to a poison-immune target, and makes a Poisoned-immune target roll -
    /// with advantage - instead of shrugging the condition off.
    #[test]
    fn a_downgraded_immunity_applies_live() {
        let corrupter = Creature::new("corrupter", 15, 50).with_rider(Rider::DowngradeImmunity {
            damage: Some(DamageKind::Poison),
            condition: Some(Condition::Poisoned),
        });
        let plain = Creature::new("plain", 15, 50);
        let mut golem =
            Creature::new("golem", 10, 1_000).with_condition_immunity(Condition::Poisoned);
        golem
            .reductions
            .push((DamageKind::Poison, Reduction::Immune));
        let roster = [(&corrupter, Side::A), (&golem, Side::B), (&plain, Side::A)];
        let (mut fight, mut rng) = fight_of(&roster, 15);

        let venom = Move::new(
            "Venom",
            Effect::AutoHit {
                damage: vec![DamageRoll::new(0, 1, 10, DamageKind::Poison)],
            },
        );
        assert_eq!(strike_once(&mut fight, &mut rng, 0, 1, &venom), 5);
        assert_eq!(strike_once(&mut fight, &mut rng, 2, 1, &venom), 0);

        let poisoning = [(Condition::Poisoned, Duration::VictimTurn)];
        assert_eq!(
            fight.conditions_against(0, 1, &poisoning),
            (poisoning.to_vec(), true)
        );
        assert_eq!(
            fight.conditions_against(2, 1, &poisoning),
            (Vec::new(), false)
        );
    }
}
