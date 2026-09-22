//! An attack roll as it happens: its roll mode, from every condition in
//! play, and the extra damage dice it carries - Sneak Attack's gate, bonus
//! dice against a creature type, an armed weapon buff - including what
//! Cunning Strike spends them on.

use crate::creature::{Rider, Strike};
use crate::rules::{Condition, DamageKind, DamageRoll, RollMode, Size};
use crate::sim::fight::{Cunning, ExtraPlan, Fight, Fighter};

impl<'a> Fight<'a> {
    /// As [`attack_mode`], but also uses up the one-shot sources of
    /// advantage the roll just drew on: [`Condition::Marked`] on `target` -
    /// "the next attack roll made against this target", hit or miss - and
    /// Steady Aim on `attacker` - "your next attack roll". Both are cleared
    /// the instant they have been read into this roll's mode rather than
    /// waiting for a turn boundary the way every other condition's
    /// [`crate::sim::fight::Expiry`] does.
    pub(super) fn attack_mode_consuming(
        &mut self,
        base: RollMode,
        attacker: usize,
        target: usize,
    ) -> RollMode {
        let mode = attack_mode(
            base,
            &self.fighters[attacker],
            &self.fighters[target],
            false,
        );
        self.fighters[target]
            .conditions
            .retain(|&(c, _)| c != Condition::Marked);
        self.fighters[attacker]
            .conditions
            .retain(|&(c, _)| !c.advantage_on_attacks());
        mode
    }

    /// The extra damage one attack roll from `me` against `target` carries,
    /// rolled in `mode` - worked out before the roll, because Cunning
    /// Strike's dice come off Sneak Attack before anything is rolled:
    ///
    /// - bonus dice against the target's creature type, from the attacker
    ///   itself or from this move's own weapon
    ///   ([`Rider::BonusDamageVsCreatureType`]);
    /// - a weapon buff an earlier condition armed
    ///   ([`Rider::ConditionTriggeredWeaponDamage`]), on a weapon attack;
    /// - Sneak Attack ([`Rider::ConditionalExtraDamage`]), if `allow_sneak`,
    ///   its once-per-turn budget is unspent, and the roll qualifies: a
    ///   finesse or ranged weapon (or a spell attack, for a creature carrying
    ///   [`Rider::ExtraDamageAppliesToSpellAttacks`]), not at disadvantage,
    ///   with advantage or an ally next to the target
    ///   ([`Fight::ally_adjacent`]). Its dice take the triggering strike's
    ///   own damage type - the weapon's, or the spell's.
    ///
    /// Pure, so the same plan a live attack rolls is the one a policy scores.
    pub(super) fn extra_damage_plan(
        &self,
        me: usize,
        target: usize,
        strike: &Strike,
        move_riders: &[Rider],
        mode: RollMode,
        allow_sneak: bool,
    ) -> ExtraPlan {
        let attacker = &self.fighters[me];
        let creature = attacker.creature;
        let victim = self.fighters[target].creature;
        let own_kind = strike.primary_kind().unwrap_or(DamageKind::Force);
        let mut plan = ExtraPlan::default();

        for rider in creature.riders.iter().chain(move_riders) {
            if let (Some(r), Rider::BonusDamageVsCreatureType { damage_kind, .. }) = (
                rider.bonus_damage_vs_creature_type(victim.creature_type),
                rider,
            ) {
                plan.rolls.push(DamageRoll::new(
                    r.dice_count,
                    r.dice_sides,
                    r.bonus,
                    *damage_kind,
                ));
            }
        }

        if strike.kind.weapon {
            for (i, rider) in creature.riders.iter().enumerate() {
                if let (Some(r), Rider::ConditionTriggeredWeaponDamage { damage_kind, .. }) =
                    (rider.weapon_damage_if_armed(attacker.armed[i]), rider)
                {
                    plan.rolls.push(DamageRoll::new(
                        r.dice_count,
                        r.dice_sides,
                        r.bonus,
                        *damage_kind,
                    ));
                }
            }
        }

        if allow_sneak {
            let extended = creature.extra_damage_applies_to_spell_attacks();
            let adjacent = self.ally_adjacent(me, target);
            if let Some(pool) = creature.riders.iter().find_map(|r| {
                r.extra_damage_for_strike(
                    strike.kind,
                    strike.primary_kind(),
                    mode,
                    adjacent,
                    attacker.sneak_attack_spent,
                    extended,
                )
            }) {
                let mut dice = pool.dice_count;
                plan.cunning = self.cunning_strike_choice(me, target, strike, dice);
                if plan.cunning.is_some() {
                    dice -= 1;
                }
                plan.rolls.push(DamageRoll::new(
                    dice,
                    pool.dice_sides,
                    pool.bonus,
                    pool.kind.unwrap_or(own_kind),
                ));
                plan.sneak_attack = true;
            }
        }
        plan
    }

    /// What a rogue with Cunning Strike spends a Sneak Attack die on, if
    /// anything - one effect per hit, as at 5th level.
    ///
    /// Poison whenever the target is not already Poisoned and can be:
    /// disadvantage on every attack roll it makes is worth far more than one
    /// die of damage against anything that attacks. Otherwise Trip, for a
    /// melee attacker whose target is Large or smaller and still standing -
    /// never a ranged one, for whom a Prone target is harder to hit, not
    /// easier. Never Withdraw: there is no movement here for it to buy.
    fn cunning_strike_choice(
        &self,
        me: usize,
        target: usize,
        strike: &Strike,
        dice: u32,
    ) -> Option<Cunning> {
        let creature = self.fighters[me].creature;
        let dc = creature.riders.iter().find_map(Rider::cunning_strike_dc)?;
        if dice == 0 {
            return None;
        }
        let victim = &self.fighters[target];
        let can_take = |c: Condition| {
            !victim.creature.immune_to_condition(c)
                || creature
                    .riders
                    .iter()
                    .any(|r| r.downgrades_condition_immunity(c))
        };
        if !victim.has(|c| c == Condition::Poisoned) && can_take(Condition::Poisoned) {
            return Some(Cunning::Poison { dc });
        }
        let knows_trip = creature
            .riders
            .iter()
            .any(|r| matches!(r, Rider::CunningStrikeTrip));
        if knows_trip
            && !strike.kind.ranged
            && victim.creature.size <= Size::Large
            && !victim.has(|c| c == Condition::Prone)
            && can_take(Condition::Prone)
        {
            return Some(Cunning::Trip { dc });
        }
        None
    }

    /// Is an ally of `me` within 5 feet of `target`, for Sneak Attack?
    ///
    /// There is no positioning here, so this is read off who is fighting what:
    /// another creature on `me`'s side, alive and not Incapacitated, that
    /// fights in melee (`fighter::fights_in_melee`) and
    /// is going after this same target ([`Fight::pick_target`]). A party's
    /// archers never count for each other; its front line counts for everyone.
    fn ally_adjacent(&self, me: usize, target: usize) -> bool {
        let side = self.fighters[me].side;
        self.fighters.iter().enumerate().any(|(i, f)| {
            i != me
                && f.side == side
                && f.alive()
                && !f.incapacitated()
                && f.melee
                && self.pick_target(i) == Some(target)
        })
    }
}

/// The 5e stacking rule: any advantage and any disadvantage cancel to a flat
/// roll, however many of each there are. `steady` adds one more source of
/// advantage - a policy asking what Steady Aim would be worth.
pub(super) fn attack_mode(
    base: RollMode,
    attacker: &Fighter<'_>,
    target: &Fighter<'_>,
    steady: bool,
) -> RollMode {
    let mut advantage = base == RollMode::Advantage || steady;
    let mut disadvantage = base == RollMode::Disadvantage;
    for &(c, _) in &target.conditions {
        advantage |= c.advantage_to_attackers();
        disadvantage |= c.disadvantage_to_attackers();
    }
    for &(c, _) in &attacker.conditions {
        disadvantage |= c.disadvantage_on_attacks();
        // Steady Aim: see `Condition::advantage_on_attacks`.
        advantage |= c.advantage_on_attacks();
    }
    match (advantage, disadvantage) {
        (true, false) => RollMode::Advantage,
        (false, true) => RollMode::Disadvantage,
        _ => RollMode::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Creature, Effect};
    use crate::prob::Rng;
    use crate::rules::Reduction;
    use crate::sim::fight::test_support::{
        bow, fight_of, no_log, puncher, sneak_attacker, strike_once,
    };
    use crate::sim::fight::Expiry;
    use crate::sim::{Budget, Policy, Side};

    // Every damage rider, Cunning Strike choice, injury dose, weapon buff and
    // reaction exercised here existed and was unit-tested as a mechanism (in
    // `creature::rider`) before the fight loop actually used any of them.
    // These pin them to the loop.

    /// Steady Aim grants advantage on the attacker's own roll (and cancels
    /// against a source of disadvantage the same as any other), the same
    /// stacking rule proven above for Poisoned/Blinded's self-inflicted
    /// disadvantage - `attack_mode` is where both are actually read, so this
    /// exercises the wiring directly rather than only the flag on `Condition`.
    #[test]
    fn steady_aim_grants_advantage_via_attack_mode() {
        let plain = Creature::new("plain", 10, 10);
        let mut rogue = Fighter::new(&plain, Side::A, Policy::Greedy, 0);
        let target = Fighter::new(&plain, Side::B, Policy::Greedy, 0);
        assert_eq!(
            attack_mode(RollMode::Normal, &rogue, &target, false),
            RollMode::Normal
        );

        rogue.add_condition(Condition::SteadyAim, Expiry::TurnStart(0));
        assert_eq!(
            attack_mode(RollMode::Normal, &rogue, &target, false),
            RollMode::Advantage,
            "Steady Aim should grant advantage on the attacker's own roll"
        );

        // A separate source of disadvantage on the attacker still cancels it,
        // the same 5e stacking rule every other pair of conditions follows.
        rogue.add_condition(Condition::Poisoned, Expiry::TurnStart(0));
        assert_eq!(
            attack_mode(RollMode::Normal, &rogue, &target, false),
            RollMode::Normal,
            "advantage and disadvantage from unrelated sources should cancel"
        );
    }

    /// Guiding Bolt's mark: Advantage on the next attack roll against its
    /// holder, from *any* attacker - not only whoever applied it - and used
    /// up by that one roll rather than sitting around for the rest of the
    /// fight.
    #[test]
    fn marked_grants_advantage_to_the_next_attack_by_any_attacker_then_clears() {
        let caster = Creature::new("caster", 10, 20);
        let target = Creature::new("target", 10, 20);
        let bystander = Creature::new("bystander", 10, 20);
        let roster = [
            (&caster, Side::A),
            (&target, Side::B),
            (&bystander, Side::A),
        ];
        let mut rng = Rng::new(11);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::Greedy; 2],
            5,
            Budget::default(),
            &mut log,
        );

        fight.apply_condition(1, Condition::Marked, Expiry::TurnStart(1));

        // A completely different attacker (index 2, not whoever applied the
        // mark at index 0) still gets Advantage off it.
        let mode = fight.attack_mode_consuming(RollMode::Normal, 2, 1);
        assert_eq!(mode, RollMode::Advantage);
        assert!(
            !fight.fighters[1].has(|c| c == Condition::Marked),
            "the mark is consumed by that one attack roll"
        );

        // A further attack against the same target no longer benefits.
        let mode = fight.attack_mode_consuming(RollMode::Normal, 0, 1);
        assert_eq!(mode, RollMode::Normal);
    }

    /// The live attack is the exact model's attack: rolled through the fight
    /// loop, with Sneak Attack qualifying on advantage and a slaying weapon's
    /// dice against its favoured prey, the damage done must follow the
    /// exact distribution of the same strike with the same riders - against
    /// a target that resists part of it.
    #[test]
    fn a_live_attack_with_riders_agrees_with_the_exact_path() {
        let rogue = sneak_attacker("rogue");
        let mut dragon = Creature::new("dragon", 16, 100_000)
            .with_creature_type(crate::rules::CreatureType::Dragon);
        dragon
            .reductions
            .push((DamageKind::Fire, Reduction::Resistant));
        let mut m = bow(7, RollMode::Advantage);
        if let Effect::Strikes { strike, .. } = &mut m.effect {
            strike
                .damage
                .push(DamageRoll::new(1, 6, 0, DamageKind::Fire));
        }
        m.riders.push(Rider::BonusDamageVsCreatureType {
            dice_count: 2,
            dice_sides: 6,
            bonus: 0,
            damage_kind: DamageKind::Piercing,
            creature_type: crate::rules::CreatureType::Dragon,
        });
        let roster = [(&rogue, Side::A), (&dragon, Side::B)];
        let (mut fight, mut rng) = fight_of(&roster, 21);

        let Effect::Strikes { strike, .. } = &m.effect else {
            unreachable!()
        };
        let plan = fight.extra_damage_plan(0, 1, strike, &m.riders, RollMode::Advantage, true);
        assert!(
            plan.sneak_attack,
            "advantage with a ranged weapon qualifies"
        );
        assert_eq!(plan.rolls.len(), 2, "the slaying dice and the sneak attack");
        let exact = strike.damage_pmf_from(&rogue, &dragon, RollMode::Advantage, &[], &plan.rolls);

        let n = 100_000;
        let (lo, hi) = (exact.min(), exact.max());
        let mut counts = vec![0usize; (hi - lo + 1) as usize];
        for _ in 0..n {
            fight.fighters[1].hp = 100_000;
            fight.fighters[0].sneak_attack_spent = false;
            let d = strike_once(&mut fight, &mut rng, 0, 1, &m);
            counts[(d - lo) as usize] += 1;
        }
        for (i, &c) in counts.iter().enumerate() {
            let value = lo + i as i32;
            let want = exact.prob(value);
            let got = c as f64 / n as f64;
            let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
            assert!(
                (got - want).abs() < tol,
                "P(damage = {value}) sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
            );
        }
    }

    /// Sneak Attack's gate, live: advantage qualifies; a plain roll with no
    /// ally beside the target does not; a melee ally engaging the same
    /// target does; a ranged ally does not; disadvantage overrides an ally.
    #[test]
    fn sneak_attack_qualifies_live_on_advantage_or_a_melee_ally() {
        let rogue = sneak_attacker("rogue");
        let target = Creature::new("target", 10, 1_000);
        let brawler = puncher("brawler", 10, 50, 5, 2);
        let archer = sneak_attacker("archer").with_action(bow(5, RollMode::Normal));
        let normal = bow(5, RollMode::Normal);
        let Effect::Strikes { strike, .. } = &normal.effect else {
            unreachable!()
        };

        let alone = [(&rogue, Side::A), (&target, Side::B)];
        let (fight, _) = fight_of(&alone, 1);
        assert!(
            !fight
                .extra_damage_plan(0, 1, strike, &[], RollMode::Normal, true)
                .sneak_attack
        );
        assert!(
            fight
                .extra_damage_plan(0, 1, strike, &[], RollMode::Advantage, true)
                .sneak_attack
        );

        let with_brawler = [(&rogue, Side::A), (&target, Side::B), (&brawler, Side::A)];
        let (mut fight, _) = fight_of(&with_brawler, 1);
        assert!(fight.ally_adjacent(0, 1));
        assert!(
            fight
                .extra_damage_plan(0, 1, strike, &[], RollMode::Normal, true)
                .sneak_attack
        );
        assert!(
            !fight
                .extra_damage_plan(0, 1, strike, &[], RollMode::Disadvantage, true)
                .sneak_attack,
            "disadvantage overrides an adjacent ally"
        );
        fight.fighters[2]
            .conditions
            .push((Condition::Stunned, Expiry::TurnStart(2)));
        assert!(
            !fight.ally_adjacent(0, 1),
            "an incapacitated ally does not count"
        );

        let with_archer = [(&rogue, Side::A), (&target, Side::B), (&archer, Side::A)];
        let (fight, _) = fight_of(&with_archer, 1);
        assert!(
            !fight.ally_adjacent(0, 1),
            "an archer is not beside the target"
        );
    }

    /// Once per turn: a second qualifying hit in the same turn gets no
    /// extra dice, and the budget comes back when the next turn starts.
    #[test]
    fn sneak_attack_is_spent_once_per_turn_live() {
        let rogue = sneak_attacker("rogue");
        let target = Creature::new("target", 1, 1_000);
        let roster = [(&rogue, Side::A), (&target, Side::B)];
        let (mut fight, mut rng) = fight_of(&roster, 3);
        // +30 against AC 1: a miss only on a natural 1.
        let m = bow(30, RollMode::Advantage);
        let Effect::Strikes { strike, .. } = &m.effect else {
            unreachable!()
        };

        let mut first = 0;
        while first == 0 {
            first = strike_once(&mut fight, &mut rng, 0, 1, &m);
        }
        assert!(fight.fighters[0].sneak_attack_spent);
        assert!(
            !fight
                .extra_damage_plan(0, 1, strike, &[], RollMode::Advantage, true)
                .sneak_attack
        );
        fight.start_of_turn(1);
        assert!(
            !fight.fighters[0].sneak_attack_spent,
            "back on the next turn"
        );
    }

    /// A weapon's own bonus against a creature type rides only that weapon,
    /// and only against that type.
    #[test]
    fn a_weapons_bonus_vs_creature_type_applies_only_to_its_prey() {
        let archer = Creature::new("archer", 15, 50);
        let dragon = Creature::new("dragon", 10, 1_000)
            .with_creature_type(crate::rules::CreatureType::Dragon);
        let giant =
            Creature::new("giant", 10, 1_000).with_creature_type(crate::rules::CreatureType::Giant);
        let mut slayer = bow(5, RollMode::Normal);
        slayer.riders.push(Rider::BonusDamageVsCreatureType {
            dice_count: 3,
            dice_sides: 6,
            bonus: 0,
            damage_kind: DamageKind::Piercing,
            creature_type: crate::rules::CreatureType::Dragon,
        });
        let Effect::Strikes { strike, .. } = &slayer.effect else {
            unreachable!()
        };
        let roster = [(&archer, Side::A), (&dragon, Side::B), (&giant, Side::B)];
        let (fight, _) = fight_of(&roster, 1);
        let vs_dragon =
            fight.extra_damage_plan(0, 1, strike, &slayer.riders, RollMode::Normal, true);
        let vs_giant =
            fight.extra_damage_plan(0, 2, strike, &slayer.riders, RollMode::Normal, true);
        assert_eq!(
            vs_dragon.rolls,
            vec![DamageRoll::new(3, 6, 0, DamageKind::Piercing)]
        );
        assert!(vs_giant.rolls.is_empty());
    }
}
