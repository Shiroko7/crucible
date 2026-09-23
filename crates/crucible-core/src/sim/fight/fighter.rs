//! One combatant's per-fight state: hit points, resource pools, spell slots,
//! conditions, and which of its moves are still available.

use crate::creature::{Cost, Creature, Move, MoveKind, Uses};
use crate::prob::Rng;
use crate::rules::Condition;
use crate::sim::fight::value::first_strike;
use crate::sim::fight::{Expiry, Fighter, MoveState, Slot};
use crate::sim::{Policy, Side};

impl MoveState {
    fn new(uses: Uses) -> Self {
        Self {
            remaining: match uses {
                Uses::Limited(n) => Some(n),
                _ => None,
            },
            charged: true,
        }
    }

    pub(super) fn available(&self) -> bool {
        self.charged && self.remaining.is_none_or(|r| r > 0)
    }

    fn spend(&mut self, uses: Uses) {
        if let Some(r) = self.remaining.as_mut() {
            *r = r.saturating_sub(1);
        }
        if matches!(uses, Uses::Recharge(_)) {
            self.charged = false;
        }
    }

    fn roll_recharge(&mut self, uses: Uses, rng: &mut Rng) {
        if let Uses::Recharge(floor) = uses {
            if !self.charged && rng.die(6) >= floor as i32 {
                self.charged = true;
            }
        }
    }
}

impl<'a> Fighter<'a> {
    pub(super) fn new(creature: &'a Creature, side: Side, policy: Policy, seat: usize) -> Self {
        Self {
            creature,
            side,
            policy,
            seat,
            hp: creature.hp,
            temp_hp: 0,
            actions: creature
                .actions
                .iter()
                .map(|m| MoveState::new(m.uses))
                .collect(),
            bonus_actions: creature
                .bonus_actions
                .iter()
                .map(|m| MoveState::new(m.uses))
                .collect(),
            legendary: creature
                .legendary
                .iter()
                .map(|m| MoveState::new(m.uses))
                .collect(),
            reactions: creature
                .reactions
                .iter()
                .map(|r| MoveState::new(r.action.uses))
                .collect(),
            // Available from the start of the fight, not from the creature's
            // first turn. The difference only shows when the legendary creature
            // loses initiative, which is exactly when it matters.
            legendary_left: creature.legendary_uses,
            resources: creature.resources.iter().map(|r| r.max).collect(),
            spell_slots: creature.spell_slots,
            rider_uses: creature.riders.iter().map(|r| r.initial_uses()).collect(),
            conditions: Vec::new(),
            concentration: None,
            attack_modifiers: Vec::new(),
            save_modifiers: Vec::new(),
            once_per_turn_spent: false,
            sneak_attack_spent: false,
            reaction: true,
            reactions_left: creature.reactions_per_round,
            armed: vec![false; creature.riders.len()],
            melee: fights_in_melee(creature),
            summoned_by: None,
            once_per_turn_damage_spent: false,
            reactive_ac: None,
            dead: false,
            slot_spent_this_turn: false,
            inside_damage: 0,
            zones: Vec::new(),
            stood_up: false,
            dealt: 0,
            spent: 0,
        }
    }

    pub(super) fn alive(&self) -> bool {
        self.hp > 0
    }

    /// Called up mid-fight rather than one of the combatants the fight is
    /// about - see [`Fighter::summoned_by`].
    pub(super) fn is_summon(&self) -> bool {
        self.summoned_by.is_some()
    }

    /// Down but not dead: a player character at 0 hit points that healing
    /// can still bring back.
    pub(super) fn can_revive(&self) -> bool {
        !self.alive() && !self.dead && self.creature.player_character
    }

    pub(super) fn bloodied(&self) -> bool {
        self.hp * 2 <= self.creature.hp
    }

    pub(super) fn has(&self, f: impl Fn(Condition) -> bool) -> bool {
        self.conditions.iter().any(|&(c, _)| f(c))
    }

    pub(super) fn incapacitated(&self) -> bool {
        self.has(Condition::incapacitated)
    }

    /// The roster index of whoever has swallowed this creature, if anyone.
    pub(super) fn swallowed_by(&self) -> Option<usize> {
        self.conditions
            .iter()
            .find_map(|&(c, expiry)| match expiry {
                Expiry::HeldBy(holder) if c == Condition::Swallowed => Some(holder),
                _ => None,
            })
    }

    /// Loses its turn entirely - what `Fight::turn` checks before letting a
    /// creature act.
    ///
    /// Incapacitated (Stunned, Paralyzed) is one way to lose a turn.
    /// [`Condition::Compelled`] (Command) is a deliberately separate second
    /// one: it steals the same turn without any of Incapacitated's other
    /// side effects, which is exactly why `Fight::legendary` keeps checking
    /// `incapacitated()` alone rather than this - Command must never cost a
    /// legendary action, only the compelled creature's own turn.
    pub(super) fn loses_turn(&self) -> bool {
        self.incapacitated() || self.has(|c| matches!(c, Condition::Compelled))
    }

    /// Is this creature currently allowed to take `m`? Only
    /// [`MoveKind::Spell`] and [`MoveKind::MagicItem`] are ever blocked - by
    /// [`Condition::blocks_magic`], and a spell that *speaks*
    /// ([`Move::needs_verbal`]) also by [`Condition::blocks_casting`] unless
    /// it is cast without components
    /// ([`Move::bypasses_casting_restrictions`]) - so an ordinary attack or
    /// stance is unaffected whatever else is active. Checked everywhere a
    /// move's legality is checked: [`Policy::choose`],
    /// [`crate::sim::fight::Fight::legal`],
    /// [`crate::sim::fight::Fight::turn`]'s execution-time recheck, and
    /// [`crate::sim::fight::Fight::repair`].
    pub(super) fn move_allowed(&self, m: &Move) -> bool {
        match m.kind {
            MoveKind::Spell => {
                // Silence takes away speech, so it stops a cast that speaks -
                // not every spell. One with no Verbal component goes through
                // it untouched, as does a charge spent to cast without
                // components at all.
                !self.has(Condition::blocks_magic)
                    && (m.bypasses_casting_restrictions
                        || !m.needs_verbal()
                        || !self.has(Condition::blocks_casting))
            }
            MoveKind::MagicItem => !self.has(Condition::blocks_magic),
            MoveKind::Standard | MoveKind::ObjectUse => true,
        }
    }

    /// Is this creature willing to spend a finite resource right now?
    ///
    /// One predicate for both hoarding policies, which differ only in when they
    /// relent. It gates move choice, on-hit riders and Legendary Resistance
    /// alike, so a thrifty dragon really does forget it has resistances.
    /// Reactions cost nothing and never consult it.
    pub(super) fn will_spend(&self) -> bool {
        match self.policy {
            Policy::Thrifty => false,
            Policy::Attrition => self.bloodied(),
            _ => true,
        }
    }

    pub(super) fn can_pay(&self, cost: Option<Cost>) -> bool {
        match cost {
            None => true,
            Some(c) => {
                self.will_spend()
                    && self
                        .resources
                        .get(c.resource)
                        .is_some_and(|&r| r >= c.amount)
            }
        }
    }

    pub(super) fn pay(&mut self, cost: Option<Cost>) {
        if let Some(c) = cost {
            if let Some(r) = self.resources.get_mut(c.resource) {
                *r = r.saturating_sub(c.amount);
            }
            self.spent += c.amount;
        }
    }

    /// Is a slot of exactly `level` available and something this policy is
    /// willing to spend? A spell slot is exactly the kind of finite resource
    /// `will_spend` already gates `cost` behind. And only one a turn: under
    /// the 2024 rules a creature can expend at most one spell slot on a
    /// single turn, so a slotted action and a slotted bonus action never go
    /// together.
    pub(super) fn can_cast(&self, level: Option<u32>) -> bool {
        match level {
            None => true,
            Some(lvl) => {
                self.will_spend()
                    && !self.slot_spent_this_turn
                    && self.spell_slots.available(lvl) > 0
            }
        }
    }

    /// Spend a slot of `level`, counting it the same way `pay` counts a
    /// resource cost. A no-op for a move with no slot cost.
    pub(super) fn cast_spell_slot(&mut self, level: Option<u32>) {
        if let Some(lvl) = level {
            self.spell_slots.cast(lvl);
            self.spent += 1;
            self.slot_spent_this_turn = true;
        }
    }

    /// Spend a move's own budget, counting it as a resource burnt.
    pub(super) fn spend_move(&mut self, slot: Slot, pick: usize, uses: Uses) {
        if !matches!(uses, Uses::Unlimited) {
            self.spent += 1;
        }
        slot.states_mut(self)[pick].spend(uses);
    }

    /// Spend this creature's reaction on its `i`th [`crate::creature::Reaction`],
    /// with whatever that move costs out of its own budget.
    pub(super) fn spend_reaction(&mut self, i: usize) {
        let uses = self.creature.reactions[i].action.uses;
        if !matches!(uses, Uses::Unlimited) {
            self.spent += 1;
        }
        self.reactions[i].spend(uses);
        self.spend_reaction_budget();
    }

    /// Spend the reaction itself - the one thing every reaction shares.
    /// Takes it for this turn, and one off what is left for the round.
    pub(super) fn spend_reaction_budget(&mut self) {
        self.reaction = false;
        self.reactions_left = self.reactions_left.saturating_sub(1);
    }

    /// The start of any turn, its own or anybody else's: a creature with
    /// reactions left this round has one again. One per turn, however many
    /// the round allows.
    pub(super) fn offer_reaction(&mut self) {
        if self.reactions_left > 0 {
            self.reaction = true;
        }
    }

    pub(super) fn add_condition(&mut self, condition: Condition, expiry: Expiry) {
        if !self.conditions.iter().any(|&(c, _)| c == condition) {
            self.conditions.push((condition, expiry));
        }
    }
}

impl Slot {
    pub(super) fn moves(self, c: &Creature) -> &[Move] {
        match self {
            Slot::Action => &c.actions,
            Slot::Bonus => &c.bonus_actions,
            Slot::Legendary => &c.legendary,
        }
    }

    pub(super) fn states<'f>(self, f: &'f Fighter<'_>) -> &'f [MoveState] {
        match self {
            Slot::Action => &f.actions,
            Slot::Bonus => &f.bonus_actions,
            Slot::Legendary => &f.legendary,
        }
    }

    fn states_mut<'f>(self, f: &'f mut Fighter<'_>) -> &'f mut [MoveState] {
        match self {
            Slot::Action => &mut f.actions,
            Slot::Bonus => &mut f.bonus_actions,
            Slot::Legendary => &mut f.legendary,
        }
    }
}

/// Everything that comes back at the start of a creature's turn: recharge rolls,
/// its reaction, its legendary actions, and its once-per-turn riders.
pub(super) fn refresh(f: &mut Fighter<'_>, rng: &mut Rng) {
    let creature = f.creature;
    for slot in [Slot::Action, Slot::Bonus, Slot::Legendary] {
        let moves = slot.moves(creature);
        let states = slot.states_mut(f);
        for (state, m) in states.iter_mut().zip(moves) {
            state.roll_recharge(m.uses, rng);
        }
    }
    for (state, r) in f.reactions.iter_mut().zip(&creature.reactions) {
        state.roll_recharge(r.action.uses, rng);
    }
    f.legendary_left = creature.legendary_uses;
    f.once_per_turn_spent = false;
    f.once_per_turn_damage_spent = false;
    f.reactions_left = creature.reactions_per_round;
    f.reaction = true;
    f.reactive_ac = None;
    f.slot_spent_this_turn = false;
}

/// Does `creature` fight in melee? Read off its first action that makes an
/// attack roll - the one its declaration order says it leads with, the
/// in-order policy's own reading.
fn fights_in_melee(creature: &Creature) -> bool {
    creature
        .actions
        .iter()
        .find_map(|m| first_strike(&m.effect))
        .is_some_and(|s| !s.kind.ranged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Effect, Resource, Strike};
    use crate::rules::{AttackModifier, DamageKind, DamageRoll, RollMode, SaveModifier};
    use crate::sim::fight::test_support::{bow, no_log, puncher};
    use crate::sim::fight::Fight;
    use crate::sim::{run, Budget};

    /// A shared pool, not a per-move budget: spending it on one move has to take
    /// it away from the other.
    #[test]
    fn moves_drawing_on_one_pool_compete_for_it() {
        let cost = Cost {
            resource: 0,
            amount: 1,
        };
        let mut hero = puncher("hero", 10, 100, 10, 0);
        hero.resources.push(Resource {
            name: "focus".into(),
            max: 2,
        });
        hero.bonus_actions.push(
            Move::new(
                "Flurry",
                Effect::Strikes {
                    strike: Strike::new(10, vec![DamageRoll::new(0, 6, 100, DamageKind::Force)]),
                    count: 1,
                },
            )
            .with_cost(cost),
        );
        let dummy = Creature::new("dummy", 10, 100_000);

        let mut rng = Rng::new(2);
        let mut log = no_log();
        let o = run(&mut rng, [&hero, &dummy], [Policy::Greedy; 2], 6, &mut log);
        let from_flurry = 200;
        assert!(
            o.damage_dealt[0] >= from_flurry && o.damage_dealt[0] < from_flurry + 100,
            "expected exactly two Flurries, got {} damage",
            o.damage_dealt[0]
        );
    }

    // The Spiritual Weapon moves built here mirror exactly what
    // `features::spells::SpiritualWeaponPlugin` registers, but are
    // constructed directly rather than imported from `features` - `sim`
    // sits below `features` in the subsystem order (see `lib.rs`'s module
    // docs), so a test that exercises the fight engine's own bookkeeping has
    // no business depending on the feature layer above it.

    /// The initial cast: a Bonus Action, a 2nd-level slot, and - critically
    /// for the "pay once, then repeat" shape - `Uses::Limited(1)` so it can
    /// never be taken a second time even if a slot is still available.
    fn spiritual_weapon_cast_move(to_hit: i32, ability_modifier: i32) -> Move {
        Move::new(
            "Spiritual Weapon",
            Effect::Strikes {
                strike: Strike::new(
                    to_hit,
                    vec![DamageRoll::new(1, 8, ability_modifier, DamageKind::Force)],
                ),
                count: 1,
            },
        )
        .with_uses(Uses::Limited(1))
        .with_spell_slot(2)
    }

    /// The repeat: an identical strike, free and unlimited, so once the
    /// move above is spent this is the only bonus action left that still
    /// qualifies.
    fn spiritual_weapon_strike_again_move(to_hit: i32, ability_modifier: i32) -> Move {
        Move::new(
            "Spiritual Weapon (Strike Again)",
            Effect::Strikes {
                strike: Strike::new(
                    to_hit,
                    vec![DamageRoll::new(1, 8, ability_modifier, DamageKind::Force)],
                ),
                count: 1,
            },
        )
    }

    /// The initial cast spends the caster's one 2nd-level slot and then
    /// becomes permanently unavailable for the rest of the fight
    /// (`Uses::Limited(1)`); the repeat strike never touches the slot pool
    /// at all, on this round or any later one.
    #[test]
    fn spiritual_weapons_initial_cast_spends_a_slot_and_the_repeat_strike_does_not() {
        let mut caster = Creature::new("caster", 20, 100);
        caster.spell_slots.set_max(2, 1);
        caster.bonus_actions.push(spiritual_weapon_cast_move(6, 3));
        caster
            .bonus_actions
            .push(spiritual_weapon_strike_again_move(6, 3));

        let dummy = Creature::new("dummy", 1, 1_000); // AC 1: almost everything hits

        let roster = [(&caster, Side::A), (&dummy, Side::B)];
        let mut rng = Rng::new(3_000);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        assert_eq!(fight.fighters[0].spell_slots.available(2), 1);
        assert!(
            fight.fighters[0].bonus_actions[0].available(),
            "the cast starts available"
        );

        fight.take_turn(1, 0, &mut rng, &mut log, None);
        assert_eq!(
            fight.fighters[0].spell_slots.available(2),
            0,
            "the initial cast spends the one 2nd-level slot"
        );
        assert!(
            !fight.fighters[0].bonus_actions[0].available(),
            "the cast is a once-per-fight bonus action, spent or not"
        );
        assert!(
            fight.fighters[0].bonus_actions[1].available(),
            "the repeat strike is unlimited and stays available"
        );

        fight.take_turn(2, 0, &mut rng, &mut log, None);
        assert_eq!(
            fight.fighters[0].spell_slots.available(2),
            0,
            "the repeat strike must not spend a second slot"
        );
        assert!(
            fight.fighters[0].bonus_actions[1].available(),
            "the repeat strike stays available across rounds"
        );
    }

    fn bless_move() -> Move {
        Move::new(
            "Bless",
            Effect::Buff {
                attack_modifier: AttackModifier::BonusDice { count: 1, sides: 4 },
                save_modifier: SaveModifier::BonusDice { count: 1, sides: 4 },
                max_targets: Some(3),
            },
        )
        .with_concentration()
        .with_spell_slot(1)
    }

    /// A caster with no 1st-level slots left cannot cast Bless again -
    /// `can_cast` gates a spell slot exactly like `can_pay` gates a
    /// resource-pool cost.
    #[test]
    fn a_caster_with_no_slots_left_cannot_cast_bless_again() {
        let mut caster = Creature::new("caster", 10, 50);
        caster.spell_slots.set_max(1, 1);
        caster.actions.push(bless_move());
        let enemy = Creature::new("enemy", 10, 50);
        let roster = [(&caster, Side::A), (&enemy, Side::B)];
        let mut rng = Rng::new(203);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        fight.take_turn(1, 0, &mut rng, &mut log, None);
        assert_eq!(fight.fighters[0].spell_slots.available(1), 0);
        assert!(
            !fight.fighters[0].attack_modifiers.is_empty(),
            "sanity check: the first cast should have landed"
        );

        // Simulate the first Bless having already ended, then try again with
        // no slots left.
        fight.fighters[0].concentration = None;
        fight.fighters[0].attack_modifiers.clear();
        fight.fighters[0].save_modifiers.clear();

        fight.take_turn(2, 0, &mut rng, &mut log, None);
        assert!(
            fight.fighters[0].attack_modifiers.is_empty(),
            "no slot left, so Bless should not have been cast a second time"
        );
        assert!(fight.fighters[0].concentration.is_none());
    }

    fn move_of(kind: MoveKind) -> Move {
        Move::new("x", Effect::Sequence(Vec::new())).with_kind(kind)
    }

    #[test]
    fn suppressed_blocks_only_spell_and_magic_item_moves() {
        let creature = Creature::new("x", 10, 10);
        let mut fighter = Fighter::new(&creature, Side::A, Policy::Greedy, 0);
        for kind in [
            MoveKind::Standard,
            MoveKind::ObjectUse,
            MoveKind::MagicItem,
            MoveKind::Spell,
        ] {
            assert!(
                fighter.move_allowed(&move_of(kind)),
                "{kind:?} should be unblocked with no conditions active"
            );
        }

        fighter
            .conditions
            .push((Condition::Suppressed, Expiry::TurnStart(0)));
        assert!(fighter.move_allowed(&move_of(MoveKind::Standard)));
        assert!(fighter.move_allowed(&move_of(MoveKind::ObjectUse)));
        assert!(
            !fighter.move_allowed(&move_of(MoveKind::MagicItem)),
            "a magic item activation must be blocked while suppressed"
        );
        assert!(
            !fighter.move_allowed(&move_of(MoveKind::Spell)),
            "casting a spell must be blocked while suppressed"
        );
        assert!(
            !fighter.move_allowed(&move_of(MoveKind::Spell).with_bypasses_casting_restrictions()),
            "casting without components does not get round being unable to cast at all"
        );

        // Expiry: once the condition is gone, both are usable again.
        fighter.conditions.clear();
        assert!(fighter.move_allowed(&move_of(MoveKind::MagicItem)));
        assert!(fighter.move_allowed(&move_of(MoveKind::Spell)));
    }

    /// Silenced stops ordinary casting and nothing else - and a cast made
    /// without components gets through it.
    #[test]
    fn silenced_blocks_spells_unless_cast_without_components() {
        let creature = Creature::new("x", 10, 10);
        let mut fighter = Fighter::new(&creature, Side::A, Policy::Greedy, 0);
        fighter
            .conditions
            .push((Condition::Silenced, Expiry::TurnStart(0)));
        assert!(!fighter.move_allowed(&move_of(MoveKind::Spell)));
        assert!(
            fighter.move_allowed(&move_of(MoveKind::Spell).with_bypasses_casting_restrictions())
        );
        assert!(fighter.move_allowed(&move_of(MoveKind::MagicItem)));
        assert!(fighter.move_allowed(&move_of(MoveKind::Standard)));
    }

    /// Only one spell slot a turn: once the action has spent one, a slotted
    /// bonus action is not castable until the caster's next turn.
    #[test]
    fn only_one_spell_slot_is_spent_a_turn() {
        let mut caster = Creature::new("caster", 10, 50);
        caster.spell_slots.set_max(1, 4);
        let mut f = Fighter::new(&caster, Side::A, Policy::Greedy, 0);
        f.cast_spell_slot(Some(1));
        assert!(!f.can_cast(Some(1)), "a second slot this turn");
        assert!(f.can_cast(None), "a cantrip is fine");
        let mut rng = Rng::new(1);
        refresh(&mut f, &mut rng);
        assert!(f.can_cast(Some(1)), "a new turn, a new slot");
    }

    /// Slots are one pool per level, spent by whichever spell of that level
    /// is cast, and a move needing an empty level is not offered.
    #[test]
    fn spells_of_one_level_share_one_pool_of_slots() {
        let mut caster = Creature::new("caster", 10, 50);
        caster.spell_slots.set_max(1, 1);
        let bolt = bow(5, RollMode::Normal).with_spell_slot(1);
        let blessing = Move::new(
            "Blessing",
            Effect::Buff {
                attack_modifier: AttackModifier::BonusDice { count: 1, sides: 4 },
                save_modifier: SaveModifier::BonusDice { count: 1, sides: 4 },
                max_targets: Some(1),
            },
        )
        .with_spell_slot(1);
        caster.actions = vec![bolt.clone(), blessing];
        let mut f = Fighter::new(&caster, Side::A, Policy::Greedy, 0);
        assert!(f.can_cast(bolt.spell_slot_level));
        f.cast_spell_slot(bolt.spell_slot_level);
        refresh(&mut f, &mut Rng::new(1));
        assert!(
            !f.can_cast(Some(1)),
            "the only 1st-level slot is gone for both"
        );
    }
}
