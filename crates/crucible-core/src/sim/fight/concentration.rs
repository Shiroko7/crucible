//! Concentration: one maintained spell per caster, the Constitution save
//! damage forces, and everything that ends when it breaks - conditions, and
//! Bless and Bane's modifiers.

use crate::prob::Rng;
use crate::rules::Ability;
use crate::sim::fight::saves::saving_throw;
use crate::sim::fight::{ConcentrationEffect, Fight};

/// DC for the Constitution save concentration takes when its holder takes
/// `damage`: 10, or half the damage taken, whichever is higher. 5e rounds
/// the half down, which integer division already does for a non-negative
/// dividend.
fn concentration_dc(damage: i32) -> i32 {
    (damage / 2).max(10)
}

impl<'a> Fight<'a> {
    /// The Constitution save concentration takes when its holder is
    /// damaged. Reuses [`saving_throw`], so an auto-failing condition and
    /// Legendary Resistance both apply to it exactly as they do to any other
    /// save.
    pub(super) fn concentration_check(&mut self, rng: &mut Rng, who: usize, damage: i32) {
        if self.fighters[who].concentration.is_none() {
            return;
        }
        let dc = concentration_dc(damage);
        let (saved, _resisted) =
            saving_throw(&mut self.fighters, rng, who, Ability::Con, dc, false);
        if !saved {
            self.end_concentration(who);
        }
    }

    /// End `who`'s concentration, if they have any, clearing whatever it was
    /// maintaining from every combatant it was maintained on.
    pub(super) fn end_concentration(&mut self, who: usize) {
        let Some(active) = self.fighters[who].concentration.take() else {
            return;
        };
        match active.effect {
            ConcentrationEffect::Condition { targets, condition } => {
                for t in targets {
                    if let Some(f) = self.fighters.get_mut(t) {
                        f.conditions.retain(|&(c, _)| c != condition);
                    }
                }
            }
            ConcentrationEffect::Modifiers {
                targets,
                attack_modifier,
                save_modifier,
            } => {
                for t in targets {
                    if let Some(f) = self.fighters.get_mut(t) {
                        f.attack_modifiers.retain(|&m| m != attack_modifier);
                        f.save_modifiers.retain(|&m| m != save_modifier);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Creature, Effect, Move, SaveEffect, Strike, Uses};
    use crate::rules::{
        save_success_chance, AttackModifier, Condition, DamageKind, DamageRoll, Duration,
        SaveModifier, SpellCastingProfile,
    };
    use crate::sim::fight::test_support::no_log;
    use crate::sim::fight::{ActiveConcentration, Expiry};
    use crate::sim::{Budget, Policy, Side};

    /// DC 10, or half the damage taken, whichever is higher - and 5e's "half,
    /// rounded down" so the boundary sits exactly at 20/21 damage rather than
    /// drifting up at odd numbers.
    #[test]
    fn concentration_dc_is_ten_or_half_the_damage_whichever_is_higher() {
        assert_eq!(concentration_dc(0), 10);
        assert_eq!(concentration_dc(10), 10);
        assert_eq!(concentration_dc(19), 10);
        assert_eq!(concentration_dc(20), 10);
        assert_eq!(concentration_dc(21), 10, "half of 21 rounds down to 10");
        assert_eq!(concentration_dc(22), 11);
        assert_eq!(concentration_dc(41), 20, "half of 41 rounds down to 20");
        assert_eq!(concentration_dc(42), 21);
    }

    /// Casting a second concentration spell ends the first immediately, even
    /// though nothing here ever deals damage - a check that only fires on
    /// "took damage and failed the save" would miss the rule that *starting*
    /// a new one is what breaks the old one.
    #[test]
    fn a_second_concentration_spell_ends_the_first() {
        let cast = |condition: Condition| {
            Move::new(
                "Spell",
                Effect::Save(SaveEffect {
                    ability: Ability::Wis,
                    dc: 99, // never saved
                    damage: vec![],
                    half_on_success: false,
                    on_failure: vec![(condition, Duration::ApplierTurn)],
                    max_targets: None,
                    requires_type: None,
                }),
            )
            .with_concentration()
        };

        let mut caster = Creature::new("caster", 10, 100);
        caster.actions.push(cast(Condition::Prone));
        caster.bonus_actions.push(cast(Condition::Blinded));
        let victim = Creature::new("victim", 10, 100);

        let roster = [(&caster, Side::A), (&victim, Side::B)];
        let mut rng = Rng::new(1);
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

        assert!(
            !fight.fighters[1].has(|c| c == Condition::Prone),
            "the action's condition must be cleared once the bonus action starts concentrating on something else"
        );
        assert!(
            fight.fighters[1].has(|c| c == Condition::Blinded),
            "the second spell's condition must still land"
        );
        let active = fight.fighters[0]
            .concentration
            .as_ref()
            .expect("still concentrating on the second spell");
        match &active.effect {
            ConcentrationEffect::Condition { targets, condition } => {
                assert_eq!(*condition, Condition::Blinded);
                assert_eq!(*targets, vec![1]);
            }
            ConcentrationEffect::Modifiers { .. } => {
                panic!("expected a Condition concentration effect")
            }
        }
    }

    /// A failed concentration save clears the condition from every combatant
    /// it was maintained on, not just one - the shape an area concentration
    /// spell (Hypnotic Pattern) needs.
    #[test]
    fn a_failed_concentration_save_clears_the_effect_from_every_target() {
        let mut caster = Creature::new("caster", 10, 100);
        caster.saves[Ability::Con.index()] = -100; // never saves
        let a = Creature::new("a", 10, 100);
        let b = Creature::new("b", 10, 100);

        let roster = [(&caster, Side::A), (&a, Side::B), (&b, Side::B)];
        let mut rng = Rng::new(2);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        fight.fighters[1]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(1)));
        fight.fighters[2]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(2)));
        fight.fighters[0].concentration = Some(ActiveConcentration {
            effect: ConcentrationEffect::Condition {
                targets: vec![1, 2],
                condition: Condition::Poisoned,
            },
        });

        fight.concentration_check(&mut rng, 0, 100); // dc 50, and the save always fails
        assert!(fight.fighters[0].concentration.is_none());
        assert!(!fight.fighters[1].has(|c| c == Condition::Poisoned));
        assert!(!fight.fighters[2].has(|c| c == Condition::Poisoned));
    }

    /// A save that is beaten leaves concentration alone - the counterpart to
    /// the failure case above, so a check that always breaks it cannot pass
    /// both.
    #[test]
    fn a_beaten_concentration_save_keeps_it_active() {
        let mut caster = Creature::new("caster", 10, 100);
        caster.saves[Ability::Con.index()] = 100; // always saves
        let victim = Creature::new("victim", 10, 100);
        let roster = [(&caster, Side::A), (&victim, Side::B)];
        let mut rng = Rng::new(3);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        fight.fighters[1]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(1)));
        fight.fighters[0].concentration = Some(ActiveConcentration {
            effect: ConcentrationEffect::Condition {
                targets: vec![1],
                condition: Condition::Poisoned,
            },
        });

        fight.concentration_check(&mut rng, 0, 100);
        assert!(fight.fighters[0].concentration.is_some());
        assert!(fight.fighters[1].has(|c| c == Condition::Poisoned));
    }

    /// Dropping to 0 HP ends concentration outright - no save offered, unlike
    /// ordinary damage. A Con save bonus of +100 would beat any DC, so if the
    /// implementation quietly rolled one anyway this would still pass; the
    /// point is that it must not need to.
    #[test]
    fn dropping_to_zero_hp_ends_concentration_without_a_save() {
        let mut caster = Creature::new("caster", 10, 100);
        caster.saves[Ability::Con.index()] = 100;
        let victim = Creature::new("victim", 10, 100);
        let roster = [(&caster, Side::A), (&victim, Side::B)];
        let mut rng = Rng::new(4);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        fight.fighters[1]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(1)));
        fight.fighters[0].concentration = Some(ActiveConcentration {
            effect: ConcentrationEffect::Condition {
                targets: vec![1],
                condition: Condition::Poisoned,
            },
        });

        fight.apply_damage(&mut rng, 0, fight.fighters[0].hp);
        assert!(fight.fighters[0].concentration.is_none());
        assert!(!fight.fighters[1].has(|c| c == Condition::Poisoned));
    }

    /// Becoming Incapacitated ends concentration immediately too, the same
    /// way 0 HP does - no save, because an Incapacitated creature cannot
    /// concentrate on anything at all.
    #[test]
    fn becoming_incapacitated_ends_concentration_without_a_save() {
        let mut caster = Creature::new("caster", 10, 100);
        caster.saves[Ability::Con.index()] = 100;
        let victim = Creature::new("victim", 10, 100);
        let roster = [(&caster, Side::A), (&victim, Side::B)];
        let mut rng = Rng::new(5);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        fight.fighters[1]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(1)));
        fight.fighters[0].concentration = Some(ActiveConcentration {
            effect: ConcentrationEffect::Condition {
                targets: vec![1],
                condition: Condition::Poisoned,
            },
        });

        fight.apply_condition(0, Condition::Stunned, Expiry::TurnStart(0));
        assert!(fight.fighters[0].concentration.is_none());
        assert!(!fight.fighters[1].has(|c| c == Condition::Poisoned));
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

    /// Spiritual Weapon never sets [`Move::concentration`], so it can be
    /// summoned alongside a genuinely concentrated spell without either one
    /// displacing the other - unlike casting a second concentration spell,
    /// which ends the first (`a_second_concentration_spell_ends_the_first`,
    /// above). The action establishes concentration on `Hold`; the bonus
    /// action then casts Spiritual Weapon, and both effects have to still be
    /// standing afterwards.
    #[test]
    fn spiritual_weapon_coexists_with_an_active_concentration_spell_without_disturbing_it() {
        let hold = Move::new(
            "Hold",
            Effect::Save(SaveEffect {
                ability: Ability::Wis,
                dc: 99, // never saved
                damage: vec![],
                half_on_success: false,
                on_failure: vec![(Condition::Prone, Duration::ApplierTurn)],
                max_targets: None,
                requires_type: None,
            }),
        )
        .with_concentration();

        let mut caster = Creature::new("caster", 10, 100);
        caster.spell_slots.set_max(2, 1);
        caster.actions.push(hold);
        caster.bonus_actions.push(spiritual_weapon_cast_move(6, 3));
        caster
            .bonus_actions
            .push(spiritual_weapon_strike_again_move(6, 3));

        let mut victim = Creature::new("victim", 1, 1_000);
        victim.saves[Ability::Wis.index()] = -100; // never saves

        let roster = [(&caster, Side::A), (&victim, Side::B)];
        let mut rng = Rng::new(3_001);
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

        assert!(
            fight.fighters[1].has(|c| c == Condition::Prone),
            "the action's concentration effect should have landed"
        );
        let active = fight.fighters[0]
            .concentration
            .as_ref()
            .expect("still concentrating on Hold");
        match &active.effect {
            ConcentrationEffect::Condition { targets, condition } => {
                assert_eq!(*condition, Condition::Prone);
                assert_eq!(*targets, vec![1]);
            }
            other => panic!("expected a Condition concentration effect, got {other:?}"),
        }

        assert_eq!(
            fight.fighters[0].spell_slots.available(2),
            0,
            "Spiritual Weapon's own bonus action should still have spent its slot"
        );
        assert!(
            !fight.fighters[0].bonus_actions[0].available(),
            "Spiritual Weapon's cast should have fired alongside the concentration spell"
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

    fn bane_move() -> Move {
        Move::new(
            "Bane",
            Effect::SaveOrModifier {
                ability: Ability::Cha,
                attack_modifier: AttackModifier::PenaltyDice { count: 1, sides: 4 },
                save_modifier: SaveModifier::PenaltyDice { count: 1, sides: 4 },
                max_targets: Some(3),
            },
        )
        .with_concentration()
        .with_spell_slot(1)
    }

    /// Bless buffs up to three creatures on the caster's own side - the
    /// caster included, and first, since a caster overwhelmingly means to
    /// bless itself - and spends a 1st-level slot to do it. A fourth ally
    /// beyond the cap gets nothing.
    #[test]
    fn bless_buffs_up_to_three_allies_including_the_caster_and_spends_a_slot() {
        let mut caster = Creature::new("caster", 10, 50);
        caster.spell_slots.set_max(1, 2);
        caster.actions.push(bless_move());
        let ally1 = Creature::new("ally1", 10, 50);
        let ally2 = Creature::new("ally2", 10, 50);
        let ally3 = Creature::new("ally3", 10, 50); // beyond Bless's cap of three
        let enemy = Creature::new("enemy", 10, 50);

        let roster = [
            (&caster, Side::A),
            (&ally1, Side::A),
            (&ally2, Side::A),
            (&ally3, Side::A),
            (&enemy, Side::B),
        ];
        let mut rng = Rng::new(200);
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

        assert_eq!(
            fight.fighters[0].spell_slots.available(1),
            1,
            "casting Bless spends one 1st-level slot"
        );

        for i in 0..3 {
            assert_eq!(
                fight.fighters[i].attack_modifiers,
                vec![AttackModifier::BonusDice { count: 1, sides: 4 }],
                "fighter {i} should carry Bless's attack bonus"
            );
            assert_eq!(
                fight.fighters[i].save_modifiers,
                vec![SaveModifier::BonusDice { count: 1, sides: 4 }],
                "fighter {i} should carry Bless's save bonus"
            );
        }
        assert!(
            fight.fighters[3].attack_modifiers.is_empty(),
            "the fourth ally is beyond Bless's cap of three targets"
        );
        assert!(fight.fighters[3].save_modifiers.is_empty());

        let active = fight.fighters[0]
            .concentration
            .as_ref()
            .expect("Bless is a concentration spell");
        match &active.effect {
            ConcentrationEffect::Modifiers { targets, .. } => assert_eq!(*targets, vec![0, 1, 2]),
            ConcentrationEffect::Condition { .. } => {
                panic!("expected a Modifiers concentration effect")
            }
        }
    }

    /// Bane forces a Charisma save against the caster's own spell save DC -
    /// only the targets that fail it carry the penalty, and the ones that
    /// succeed are untouched.
    #[test]
    fn bane_debuffs_only_the_targets_that_fail_their_charisma_save() {
        let mut caster = Creature::new("caster", 10, 50);
        caster.spell_slots.set_max(1, 1);
        caster.spellcasting = Some(SpellCastingProfile::new(Ability::Cha, 4, 3)); // dc 15
        caster.actions.push(bane_move());

        let mut weak_save = Creature::new("weak", 10, 50);
        weak_save.saves[Ability::Cha.index()] = -100; // always fails
        let mut strong_save = Creature::new("strong", 10, 50);
        strong_save.saves[Ability::Cha.index()] = 100; // always succeeds

        let roster = [
            (&caster, Side::A),
            (&weak_save, Side::B),
            (&strong_save, Side::B),
        ];
        let mut rng = Rng::new(201);
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

        assert_eq!(
            fight.fighters[0].spell_slots.available(1),
            0,
            "casting Bane spends one 1st-level slot"
        );
        assert_eq!(
            fight.fighters[1].attack_modifiers,
            vec![AttackModifier::PenaltyDice { count: 1, sides: 4 }],
            "the creature that failed its save should carry Bane's penalty"
        );
        assert_eq!(
            fight.fighters[1].save_modifiers,
            vec![SaveModifier::PenaltyDice { count: 1, sides: 4 }]
        );
        assert!(
            fight.fighters[2].attack_modifiers.is_empty(),
            "the creature that succeeded its save should be unaffected"
        );
        assert!(fight.fighters[2].save_modifiers.is_empty());

        let active = fight.fighters[0]
            .concentration
            .as_ref()
            .expect("Bane is a concentration spell");
        match &active.effect {
            ConcentrationEffect::Modifiers { targets, .. } => assert_eq!(*targets, vec![1]),
            ConcentrationEffect::Condition { .. } => {
                panic!("expected a Modifiers concentration effect")
            }
        }
    }

    /// Bane's DC is read from the caster's own `SpellCastingProfile` at the
    /// moment it resolves, never a fixed number on the move - raising the
    /// caster's spellcasting stat alone, with nothing else different, turns
    /// a save that always succeeds into one that always fails.
    #[test]
    fn banes_save_dc_tracks_the_casters_spellcasting_profile_not_a_fixed_number() {
        let failed = |profile: SpellCastingProfile| -> bool {
            let mut caster = Creature::new("caster", 10, 50);
            caster.spell_slots.set_max(1, 1);
            caster.spellcasting = Some(profile);
            caster.actions.push(bane_move());
            // A save bonus high enough that whether it succeeds is decided
            // entirely by the DC, not by the die roll underneath it.
            let mut target = Creature::new("target", 10, 50);
            target.saves[Ability::Cha.index()] = 25;

            let roster = [(&caster, Side::A), (&target, Side::B)];
            let mut rng = Rng::new(202);
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
            !fight.fighters[1].attack_modifiers.is_empty()
        };

        // dc 10 vs a +25 save: needed <= 1, so it always succeeds regardless
        // of the roll.
        let weak = SpellCastingProfile::new(Ability::Cha, 0, 2);
        assert!(
            !failed(weak),
            "a DC of 10 against a +25 save must always succeed"
        );

        // dc 46 vs the same +25 save: needed >= 21, so it always fails
        // regardless of the roll - nothing changed except the profile.
        let strong = SpellCastingProfile::new(Ability::Cha, 20, 18);
        assert!(
            failed(strong),
            "a DC of 46 against a +25 save must always fail"
        );
    }

    /// Starting Bless ends whatever the caster was concentrating on before -
    /// even a Condition-based spell, the sibling direction to
    /// `a_second_concentration_spell_ends_the_first`, which only covers two
    /// Condition-based spells.
    #[test]
    fn casting_bless_ends_a_prior_condition_based_concentration() {
        let mut caster = Creature::new("caster", 10, 100);
        caster.spell_slots.set_max(1, 1);
        caster.actions.push(bless_move());
        let enemy = Creature::new("enemy", 10, 100);
        let roster = [(&caster, Side::A), (&enemy, Side::B)];
        let mut rng = Rng::new(204);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        fight.fighters[1]
            .conditions
            .push((Condition::Poisoned, Expiry::TurnStart(1)));
        fight.fighters[0].concentration = Some(ActiveConcentration {
            effect: ConcentrationEffect::Condition {
                targets: vec![1],
                condition: Condition::Poisoned,
            },
        });

        fight.take_turn(1, 0, &mut rng, &mut log, None);

        assert!(
            !fight.fighters[1].has(|c| c == Condition::Poisoned),
            "the old concentration effect must be cleared when Bless starts a new one"
        );
        let active = fight.fighters[0]
            .concentration
            .as_ref()
            .expect("now concentrating on Bless");
        match &active.effect {
            ConcentrationEffect::Modifiers { .. } => {}
            ConcentrationEffect::Condition { .. } => {
                panic!("expected the new Modifiers effect, not the old Condition one")
            }
        }
    }

    /// A failed concentration save clears Bless/Bane's ongoing modifiers
    /// from every target that had them - the `Modifiers` sibling of
    /// `a_failed_concentration_save_clears_the_effect_from_every_target`,
    /// which only covers the `Condition` case.
    #[test]
    fn a_failed_concentration_save_clears_bless_or_banes_modifiers_from_every_target() {
        let mut caster = Creature::new("caster", 10, 100);
        caster.saves[Ability::Con.index()] = -100; // never saves
        let a = Creature::new("a", 10, 100);
        let b = Creature::new("b", 10, 100);

        let roster = [(&caster, Side::A), (&a, Side::B), (&b, Side::B)];
        let mut rng = Rng::new(205);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );

        let attack_modifier = AttackModifier::PenaltyDice { count: 1, sides: 4 };
        let save_modifier = SaveModifier::PenaltyDice { count: 1, sides: 4 };
        fight.fighters[1].attack_modifiers.push(attack_modifier);
        fight.fighters[1].save_modifiers.push(save_modifier);
        fight.fighters[2].attack_modifiers.push(attack_modifier);
        fight.fighters[2].save_modifiers.push(save_modifier);
        fight.fighters[0].concentration = Some(ActiveConcentration {
            effect: ConcentrationEffect::Modifiers {
                targets: vec![1, 2],
                attack_modifier,
                save_modifier,
            },
        });

        fight.concentration_check(&mut rng, 0, 100); // dc 50, and the save always fails
        assert!(fight.fighters[0].concentration.is_none());
        assert!(fight.fighters[1].attack_modifiers.is_empty());
        assert!(fight.fighters[1].save_modifiers.is_empty());
        assert!(fight.fighters[2].attack_modifiers.is_empty());
        assert!(fight.fighters[2].save_modifiers.is_empty());
    }

    /// `saving_throw` is what `concentration_check` calls for a creature's
    /// own Con save, so a fighter carrying Bless's save bonus - from
    /// blessing itself - sees it raise that save's success rate exactly as
    /// `save_success_chance` predicts. This is "Bless applies to its own
    /// concentration save" made concrete and checked for exact agreement,
    /// not just plausibility.
    #[test]
    fn a_blessed_fighters_own_saving_throw_including_a_concentration_check_gets_the_bonus_die() {
        let caster = Creature::new("caster", 10, 100);
        let other = Creature::new("other", 10, 100);
        let roster = [(&caster, Side::A), (&other, Side::B)];
        let mut rng = Rng::new(206);
        let mut log = no_log();
        let mut fight = Fight::new(
            &mut rng,
            &roster,
            [Policy::InOrder; 2],
            5,
            Budget::default(),
            &mut log,
        );
        fight.fighters[0]
            .save_modifiers
            .push(SaveModifier::BonusDice { count: 1, sides: 4 });

        let (save_bonus, dc) = (0, 14);
        let modifiers = [SaveModifier::BonusDice { count: 1, sides: 4 }];
        let exact = save_success_chance(save_bonus, dc, &modifiers);
        let n = 100_000;
        let mut successes = 0usize;
        for _ in 0..n {
            let (saved, _) =
                saving_throw(&mut fight.fighters, &mut rng, 0, Ability::Con, dc, false);
            if saved {
                successes += 1;
            }
        }
        let got = successes as f64 / n as f64;
        let tol = 5.0 * (exact * (1.0 - exact) / n as f64).sqrt() + 1e-4;
        assert!(
            (got - exact).abs() < tol,
            "P(save succeeds) sampled {got:.5}, exact {exact:.5}, tol {tol:.5}"
        );
    }
}
