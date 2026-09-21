//! True Strike (2024 cantrip).

use crate::creature::{AttackKind, Effect, Move, MoveKind, Rider, Strike};
use crate::dsl::grammar;
use crate::dsl::grammar::TraitEffect;
use crate::features::{
    CreatureBuilder, FeatureError, FeaturePlugin, FeatureRegistry, FeatureResult,
};
use crate::rules::{Attack, DamageKind, DamageRider, DamageRoll, SpellCastingProfile};

/// A trait phrase that must describe a [`Rider`] - `bonus 3d6 piercing vs
/// dragon` - rather than a flat stat bonus, for a plugin that attaches it to
/// one of its own moves.
fn parse_rider_phrase(phrase: &str) -> FeatureResult<Rider> {
    match grammar::parse_trait_external(phrase).map_err(FeatureError::InvalidConfiguration)? {
        TraitEffect::Rider(rider) => Ok(rider),
        other => Err(FeatureError::InvalidConfiguration(format!(
            "`{phrase}` is a flat bonus ({other:?}), not something a weapon carries onto a hit"
        ))),
    }
}

/// True Strike (2024 cantrip): an Action. Make a weapon attack, but use the
/// caster's spellcasting ability modifier instead of Strength or Dexterity
/// for both the attack roll and the weapon's damage roll. On a hit, the
/// target also takes extra Radiant damage that scales with the caster's
/// level - 2d6 at the base tier (character level 1-4), more at higher tiers
/// per the SRD's cantrip-scaling convention.
///
/// Every number that varies by build or by level is a plugin parameter rather
/// than baked in, the same reason
/// [`crate::features::classes::rogue::SneakAttackPlugin`] takes `dice_count`
/// instead of a hardcoded "4d6": which weapon is wielded changes
/// `weapon_dice_count`/`weapon_dice_sides`/`weapon_damage_kind` and
/// `finesse_or_ranged`, and the caster's cantrip-scaling tier changes
/// `radiant_dice_count` (this task only wires up the base 2d6 tier as a
/// caller-supplied value, not a hardcoded one - a later config simply passes a
/// bigger number for a higher tier).
///
/// The attack roll and the weapon's flat damage bonus are read from the
/// caster's [`SpellCastingProfile`] rather than any Strength or Dexterity
/// score: [`SpellCastingProfile::attack_bonus`] (ability modifier +
/// proficiency + item bonus, the standard spell attack formula) replaces the
/// weapon's usual to-hit, and `ability_modifier` alone (no proficiency, the
/// same as a normal weapon's Strength or Dexterity modifier) replaces the
/// weapon's usual flat damage bonus. Neither is ever a literal number picked
/// by this plugin.
///
/// This still reads as a genuine weapon attack for anything that gates on
/// that - Sneak Attack, most obviously. [`TrueStrikePlugin::attack`] flags
/// the resulting [`Attack`] with both [`Attack::is_spell_attack`] (it is a
/// spell) and [`Attack::finesse_or_ranged`] (whenever the wielded weapon
/// itself has that property), and the live move carries the same two facts
/// in its strike's [`AttackKind`]. The two are independent and additive: a
/// rogue using True Strike with a rapier still qualifies for Sneak Attack
/// through the ordinary weapon gate -
/// [`crate::creature::Rider::extra_damage_for`] - with or without any
/// build that also extends Sneak Attack to spell attacks (see
/// [`crate::creature::Rider::extra_damage_for_with_spell_attack_extension`]).
///
/// The attack is made *with* the wielded weapon, so whatever that weapon
/// brings comes along: its magic bonus (and its ammunition's) to both the
/// attack and damage roll - [`TrueStrikePlugin::weapon_bonus`] - and any
/// damage rider it carries, like a slaying weapon's extra dice against its
/// favoured prey - [`TrueStrikePlugin::weapon_riders`].
#[derive(Debug, Clone)]
pub struct TrueStrikePlugin {
    /// The wielded weapon's own damage dice - unrelated to the caster's
    /// level, since it is the weapon that determines this, not the spell.
    pub weapon_dice_count: u32,
    pub weapon_dice_sides: u32,
    pub weapon_damage_kind: DamageKind,
    /// Whether the wielded weapon has the finesse or ranged property - see
    /// [`Attack::finesse_or_ranged`]. `false` for anything else (a
    /// non-finesse melee weapon).
    pub finesse_or_ranged: bool,
    /// Whether the wielded weapon is a ranged one specifically - which part
    /// of `finesse_or_ranged` holds. Decides whether the live strike is a
    /// melee or a ranged attack roll.
    pub ranged: bool,
    /// The wielded weapon's own magic bonus, plus its ammunition's - added to
    /// the attack roll and to the weapon's damage roll, exactly as it would
    /// be for an ordinary attack with that weapon.
    pub weapon_bonus: i32,
    /// Damage riders the wielded weapon itself carries onto every hit - a
    /// slaying weapon's [`Rider::BonusDamageVsCreatureType`].
    pub weapon_riders: Vec<Rider>,
    /// The cantrip-scaling tier's bonus Radiant dice count - 2 at the base
    /// tier, more at higher character levels.
    pub radiant_dice_count: u32,
    pub radiant_dice_sides: u32,
    pub radiant_damage_kind: DamageKind,
}

impl TrueStrikePlugin {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        weapon_dice_count: u32,
        weapon_dice_sides: u32,
        weapon_damage_kind: DamageKind,
        finesse_or_ranged: bool,
        radiant_dice_count: u32,
        radiant_dice_sides: u32,
        radiant_damage_kind: DamageKind,
    ) -> Self {
        Self {
            weapon_dice_count,
            weapon_dice_sides,
            weapon_damage_kind,
            finesse_or_ranged,
            ranged: false,
            weapon_bonus: 0,
            weapon_riders: Vec::new(),
            radiant_dice_count,
            radiant_dice_sides,
            radiant_damage_kind,
        }
    }

    /// The wielded weapon is a ranged one - see [`TrueStrikePlugin::ranged`].
    /// Implies `finesse_or_ranged`.
    pub fn with_ranged(mut self) -> Self {
        self.ranged = true;
        self.finesse_or_ranged = true;
        self
    }

    /// The wielded weapon's (and ammunition's) magic bonus - see
    /// [`TrueStrikePlugin::weapon_bonus`].
    pub fn with_weapon_bonus(mut self, bonus: i32) -> Self {
        self.weapon_bonus = bonus;
        self
    }

    /// A damage rider the wielded weapon carries - see
    /// [`TrueStrikePlugin::weapon_riders`].
    pub fn with_weapon_rider(mut self, rider: Rider) -> Self {
        self.weapon_riders.push(rider);
        self
    }

    /// The live strike's [`AttackKind`]: a weapon attack made as part of a
    /// spell, ranged or finesse per the wielded weapon.
    fn attack_kind(&self) -> AttackKind {
        AttackKind {
            weapon: true,
            spell: true,
            ranged: self.ranged,
            finesse: self.finesse_or_ranged && !self.ranged,
        }
    }

    /// A standard True Strike: the base 2d6 Radiant tier, Radiant damage
    /// type, over whatever weapon `weapon_dice_count`/`weapon_dice_sides`/
    /// `weapon_damage_kind`/`finesse_or_ranged` describe.
    pub fn base_tier(
        weapon_dice_count: u32,
        weapon_dice_sides: u32,
        weapon_damage_kind: DamageKind,
        finesse_or_ranged: bool,
    ) -> Self {
        Self::new(
            weapon_dice_count,
            weapon_dice_sides,
            weapon_damage_kind,
            finesse_or_ranged,
            2,
            6,
            DamageKind::Radiant,
        )
    }

    /// The [`Attack`] True Strike resolves as, against `profile` - the
    /// caster's own [`SpellCastingProfile`], never a hardcoded number. The
    /// weapon's dice carry the caster's ability modifier as their flat bonus
    /// in place of Strength or Dexterity, and the scaling tier's Radiant
    /// dice ride alongside as a [`DamageRider`] - doubled on a crit exactly
    /// like the weapon's own dice, which is correct: a critical hit doubles
    /// every damage die an attack rolls, not only the weapon's (see
    /// `rules::attack`'s module docs).
    ///
    /// Roll mode and ally-adjacency are situational, not a property of the
    /// spell, so they are left at [`Attack`]'s defaults here - a caller
    /// chains [`Attack::with_mode`] / [`Attack::with_ally_adjacent`] for the
    /// attack actually being resolved, the same way any other [`Attack`] is
    /// built up.
    pub fn attack(&self, profile: SpellCastingProfile) -> Attack {
        Attack::new(
            profile.attack_bonus() + self.weapon_bonus,
            self.weapon_dice_count,
            self.weapon_dice_sides,
            profile.ability_modifier + self.weapon_bonus,
        )
        .with_is_spell_attack(true)
        .with_finesse_or_ranged(self.finesse_or_ranged)
        .with_damage_rider(DamageRider::new(
            self.radiant_dice_count,
            self.radiant_dice_sides,
        ))
    }
}

impl FeaturePlugin for TrueStrikePlugin {
    fn id(&self) -> &'static str {
        "true_strike"
    }

    fn name(&self) -> &str {
        "True Strike"
    }

    fn apply(&self, builder: &mut CreatureBuilder) -> FeatureResult<()> {
        let profile = builder.creature.spellcasting.ok_or_else(|| {
            FeatureError::PrerequisiteNotMet(
                "True Strike needs this creature's spellcasting profile declared first".to_string(),
            )
        })?;
        let strike = Strike::new(
            profile.attack_bonus() + self.weapon_bonus,
            vec![
                DamageRoll::new(
                    self.weapon_dice_count,
                    self.weapon_dice_sides,
                    profile.ability_modifier + self.weapon_bonus,
                    self.weapon_damage_kind,
                ),
                DamageRoll::new(
                    self.radiant_dice_count,
                    self.radiant_dice_sides,
                    0,
                    self.radiant_damage_kind,
                ),
            ],
        )
        .with_kind(self.attack_kind());
        let mut mv = Move::new("True Strike", Effect::Strikes { strike, count: 1 })
            .with_kind(MoveKind::Spell);
        for rider in &self.weapon_riders {
            mv = mv.with_rider(rider.clone());
        }
        builder.add_action(mv);
        Ok(())
    }
}

pub(super) fn register(registry: &mut FeatureRegistry) {
    // True Strike (2024 cantrip): a weapon attack using the caster's own
    // SpellCastingProfile, plus scaling Radiant dice - every number here
    // is a TOML parameter, see `spells::TrueStrikePlugin`.
    registry.register("true_strike", |val| {
        let damage_kind = |key: &str, default: Option<&str>| -> FeatureResult<DamageKind> {
            let word = match (val.get(key).and_then(|v| v.as_str()), default) {
                (Some(word), _) => word,
                (None, Some(default)) => default,
                (None, None) => {
                    return Err(FeatureError::InvalidConfiguration(format!(
                        "true_strike needs a `{key}`"
                    )))
                }
            };
            DamageKind::parse(word).ok_or_else(|| FeatureError::UnknownDamageKind(word.to_string()))
        };

        let weapon_dice_count = val
            .get("weapon_dice_count")
            .and_then(|v| v.as_integer())
            .ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "true_strike needs a `weapon_dice_count` (the wielded weapon's own dice count)"
                        .to_string(),
                )
            })? as u32;
        let weapon_dice_sides = val
            .get("weapon_dice_sides")
            .and_then(|v| v.as_integer())
            .ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "true_strike needs a `weapon_dice_sides` (the wielded weapon's own die size)"
                        .to_string(),
                )
            })? as u32;
        let weapon_damage_kind = damage_kind("weapon_damage_kind", None)?;
        let finesse_or_ranged = val
            .get("finesse_or_ranged")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let ranged = val.get("ranged").and_then(|v| v.as_bool()).unwrap_or(false);
        let weapon_bonus = val
            .get("weapon_bonus")
            .and_then(|v| v.as_integer())
            .unwrap_or(0) as i32;
        let weapon_riders = match val.get("weapon_traits").and_then(|v| v.as_array()) {
            None => Vec::new(),
            Some(list) => list
                .iter()
                .map(|v| {
                    v.as_str()
                        .ok_or_else(|| {
                            FeatureError::InvalidConfiguration(
                                "true_strike `weapon_traits` is a list of trait phrases"
                                    .to_string(),
                            )
                        })
                        .and_then(parse_rider_phrase)
                })
                .collect::<FeatureResult<Vec<Rider>>>()?,
        };
        let radiant_dice_count = val
            .get("radiant_dice_count")
            .and_then(|v| v.as_integer())
            .ok_or_else(|| {
                FeatureError::InvalidConfiguration(
                    "true_strike needs a `radiant_dice_count` (its cantrip-scaling tier's bonus dice, 2 at the base tier)"
                        .to_string(),
                )
            })? as u32;
        let radiant_dice_sides = val
            .get("radiant_dice_sides")
            .and_then(|v| v.as_integer())
            .unwrap_or(6) as u32;
        let radiant_damage_kind = damage_kind("radiant_damage_kind", Some("radiant"))?;

        let mut plugin = TrueStrikePlugin::new(
            weapon_dice_count,
            weapon_dice_sides,
            weapon_damage_kind,
            finesse_or_ranged,
            radiant_dice_count,
            radiant_dice_sides,
            radiant_damage_kind,
        )
        .with_weapon_bonus(weapon_bonus);
        if ranged {
            plugin = plugin.with_ranged();
        }
        for rider in weapon_riders {
            plugin = plugin.with_weapon_rider(rider);
        }
        Ok(Box::new(plugin))
    });
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::features::classes::rogue::SneakAttackPlugin;
    use crate::prob::Rng;
    use crate::rules::{damage_pmf, sample_damage, Ability, Defense, RollMode};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// A rapier-wielding caster: finesse weapon, 1d8, Wisdom-based casting.
    fn rapier_true_strike() -> TrueStrikePlugin {
        TrueStrikePlugin::base_tier(1, 8, DamageKind::Piercing, true)
    }

    fn wis_profile(ability_modifier: i32, proficiency_bonus: i32) -> SpellCastingProfile {
        SpellCastingProfile::new(Ability::Wis, ability_modifier, proficiency_bonus)
    }

    /// The whole point of taking a `SpellCastingProfile` rather than a
    /// number: two different profiles must produce two different attacks,
    /// each matching that profile's own formula, not one fixed constant this
    /// plugin picked.
    #[test]
    fn attack_roll_and_damage_bonus_come_from_the_spellcasting_profile_not_hardcoded() {
        let plugin = rapier_true_strike();

        let modest = wis_profile(2, 3);
        let attack = plugin.attack(modest);
        assert_eq!(attack.to_hit, modest.attack_bonus());
        assert_eq!(attack.damage_bonus, modest.ability_modifier);

        let potent = wis_profile(5, 6).with_item_bonus(1);
        let attack = plugin.attack(potent);
        assert_eq!(attack.to_hit, potent.attack_bonus());
        assert_eq!(attack.damage_bonus, potent.ability_modifier);

        assert_ne!(
            modest.attack_bonus(),
            potent.attack_bonus(),
            "the two profiles must actually differ for this test to mean anything"
        );
    }

    /// [`Attack::is_spell_attack`] is always set; [`Attack::finesse_or_ranged`]
    /// tracks the wielded weapon, independently.
    #[test]
    fn the_attack_is_flagged_as_a_spell_attack_and_carries_the_weapons_own_finesse_or_ranged_flag()
    {
        let profile = wis_profile(3, 2);

        let finesse = TrueStrikePlugin::base_tier(1, 8, DamageKind::Piercing, true).attack(profile);
        assert!(finesse.is_spell_attack);
        assert!(finesse.finesse_or_ranged);

        let non_finesse =
            TrueStrikePlugin::base_tier(1, 8, DamageKind::Bludgeoning, false).attack(profile);
        assert!(non_finesse.is_spell_attack);
        assert!(!non_finesse.finesse_or_ranged);
    }

    /// On a hit, the weapon's own dice and the configured Radiant dice both
    /// land, and a crit doubles both pools identically - exactly the shape
    /// [`crate::rules::rider_pmf`] already guarantees for any
    /// [`DamageRider`], not reimplemented here.
    #[test]
    fn on_hit_damage_is_the_weapons_own_dice_plus_the_configured_radiant_dice() {
        let profile = wis_profile(4, 3); // ability_modifier 4
        let plugin = TrueStrikePlugin::base_tier(2, 6, DamageKind::Slashing, false);
        let attack = plugin.attack(profile);
        let defense = Defense::new(1, 60); // AC 1: every non-fumble roll hits

        let pmf = damage_pmf(&attack, &defense);
        assert!(close(pmf.total(), 1.0));

        // A hit's maximum: weapon 2d6 (12) + damage_bonus 4 + radiant 2d6 (12) = 28.
        // A crit doubles every die but not the flat bonus: weapon 4d6 (24) +
        // damage_bonus 4 + radiant 4d6 (24) = 52 - the overall maximum, since
        // the crit branch dominates the hit branch.
        let hit_max = 2 * 6 + 4 + 2 * 6;
        let crit_max = 2 * 2 * 6 + 4 + 2 * 2 * 6;
        assert!(crit_max > hit_max);
        assert_eq!(pmf.max(), crit_max);

        // Every hit (crit or not) includes the flat damage_bonus plus at
        // least the two pools' minimums (zero), so the mean strictly exceeds
        // what the weapon alone would deal - the radiant dice are additive,
        // not a replacement.
        let weapon_only = Attack::new(attack.to_hit, 2, 6, 4);
        assert!(damage_pmf(&attack, &defense).mean() > damage_pmf(&weapon_only, &defense).mean());
    }

    /// The point of the whole exercise: True Strike is still a *weapon*
    /// attack, so a rogue using it with a finesse weapon and Advantage still
    /// triggers Sneak-Attack-style extra damage through the ordinary weapon
    /// gate - reading the dice count straight off the creature's own
    /// [`SneakAttackPlugin`] configuration, never a hardcoded "5d6".
    #[test]
    fn sneak_attack_style_extra_damage_still_triggers_under_advantage_with_a_finesse_weapon() {
        // An arbitrary, deliberately non-default dice count - the whole
        // point is that the test never repeats this number as a literal
        // anywhere else; it is read back off the plugin/rider instead.
        let sneak_attack = SneakAttackPlugin::new(3);
        let builder = CreatureBuilder::new("True Strike Rogue", 15, 40)
            .apply_feature(&sneak_attack)
            .expect("sneak attack applies");
        let rider = builder.creature.riders[0].clone();
        let Rider::ConditionalExtraDamage {
            dice_count,
            dice_sides,
            ..
        } = rider
        else {
            panic!("expected a ConditionalExtraDamage rider");
        };
        assert_eq!(dice_count, sneak_attack.dice_count);
        assert_eq!(dice_sides, sneak_attack.dice_sides);

        let profile = wis_profile(3, 2);
        let attack = rapier_true_strike()
            .attack(profile)
            .with_mode(RollMode::Advantage);
        assert!(
            attack.finesse_or_ranged,
            "True Strike over a rapier must still read as a finesse weapon attack"
        );

        let extra = rider
            .extra_damage_for(&attack, false)
            .expect("advantage plus a finesse weapon should qualify for Sneak Attack");
        assert_eq!(extra, DamageRider::new(dice_count, dice_sides));
    }

    /// Without a qualifying weapon (no finesse or ranged property), Sneak
    /// Attack does not trigger off True Strike either - the spell-attack
    /// flag alone is not enough through the plain gate, matching AT-02's
    /// existing "no extension present" behaviour.
    #[test]
    fn a_non_finesse_weapon_does_not_qualify_for_sneak_attack_even_at_advantage() {
        let rider = Rider::ConditionalExtraDamage {
            dice_count: 3,
            dice_sides: 6,
            once_per_turn: true,
        };
        let profile = wis_profile(3, 2);
        let attack = TrueStrikePlugin::base_tier(1, 10, DamageKind::Bludgeoning, false)
            .attack(profile)
            .with_mode(RollMode::Advantage);
        assert_eq!(rider.extra_damage_for(&attack, false), None);
    }

    /// Applying the plugin adds an Action built from the caster's profile,
    /// not from any Strength or Dexterity score, and requires spellcasting
    /// to already be declared - the same "well-formed feature, unmet
    /// prerequisite" shape `prestige_spellcasting` uses.
    #[test]
    fn applying_the_plugin_adds_an_action_using_the_declared_spellcasting_profile() {
        let mut builder = CreatureBuilder::new("Caster", 15, 30);
        let profile = wis_profile(4, 3);
        builder.set_spellcasting(profile);

        let plugin = TrueStrikePlugin::base_tier(1, 8, DamageKind::Piercing, true);
        plugin.apply(&mut builder).expect("prerequisite is met");

        let action = builder
            .creature
            .actions
            .last()
            .expect("an action was added");
        assert_eq!(action.name, "True Strike");
        let Effect::Strikes { strike, count } = &action.effect else {
            panic!("expected a Strikes effect");
        };
        assert_eq!(*count, 1);
        assert_eq!(strike.to_hit, profile.attack_bonus());
        assert_eq!(
            strike.damage,
            vec![
                DamageRoll::new(1, 8, profile.ability_modifier, DamageKind::Piercing),
                DamageRoll::new(2, 6, 0, DamageKind::Radiant),
            ]
        );
    }

    #[test]
    fn applying_the_plugin_without_spellcasting_declared_is_a_prerequisite_failure() {
        let mut builder = CreatureBuilder::new("Not Yet A Caster", 15, 30);
        let plugin = TrueStrikePlugin::base_tier(1, 8, DamageKind::Piercing, true);
        assert!(matches!(
            plugin.apply(&mut builder),
            Err(FeatureError::PrerequisiteNotMet(_))
        ));
        assert!(builder.creature.actions.is_empty());
    }

    /// The same agreement check every other rule in this codebase is held
    /// to: the sampled path must match the exact `damage_pmf` it is supposed
    /// to be distributed as, across both the base attack and the
    /// Sneak-Attack-composed one.
    #[test]
    fn sampled_true_strike_agrees_with_the_exact_path() {
        let profile = wis_profile(4, 3);
        let defense = Defense::new(14, 60);
        let base = rapier_true_strike()
            .attack(profile)
            .with_mode(RollMode::Advantage);

        let sneak_rider = Rider::ConditionalExtraDamage {
            dice_count: 3,
            dice_sides: 6,
            once_per_turn: true,
        };
        let extra = sneak_rider
            .extra_damage_for(&base, false)
            .expect("advantage plus a finesse weapon qualifies");
        let composed = base.clone().with_damage_rider(extra);

        for (seed, (name, attack)) in [
            ("true strike alone", base),
            ("true strike + sneak attack", composed),
        ]
        .into_iter()
        .enumerate()
        {
            let exact = damage_pmf(&attack, &defense);
            let (lo, hi) = (exact.min(), exact.max());
            let mut rng = Rng::new(seed as u64 + 4_200);
            let n = 100_000;
            let mut counts = vec![0usize; (hi - lo + 1) as usize];
            for _ in 0..n {
                let d = sample_damage(&mut rng, &attack, &defense);
                assert!(
                    d >= lo && d <= hi,
                    "{name}: sampled {d} outside {lo}..={hi}"
                );
                counts[(d - lo) as usize] += 1;
            }
            for (i, &c) in counts.iter().enumerate() {
                let value = lo + i as i32;
                let want = exact.prob(value);
                let got = c as f64 / n as f64;
                let tol = 5.0 * (want * (1.0 - want) / n as f64).sqrt() + 1e-4;
                assert!(
                    (got - want).abs() < tol,
                    "{name}: P(damage = {value}) sampled {got:.5}, exact {want:.5}, tol {tol:.5}"
                );
            }
        }
    }

    /// `weapon_dice_count`/`weapon_dice_sides` and `radiant_dice_count` are
    /// the whole point of making True Strike a plugin rather than a
    /// hardcoded 1d8 rapier at the base 2d6 tier - a different weapon or a
    /// higher cantrip-scaling tier is just different TOML, not a code change.
    #[test]
    fn true_strike_reads_its_parameters_from_toml_and_applies() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "true_strike"
                weapon_dice_count = 1
                weapon_dice_sides = 8
                weapon_damage_kind = "piercing"
                finesse_or_ranged = true
                radiant_dice_count = 2
            "#,
        )
        .unwrap();
        let plugin = registry
            .build_plugin("true_strike", &params)
            .expect("true_strike builds from toml");
        assert_eq!(plugin.id(), "true_strike");
        assert_eq!(plugin.name(), "True Strike");

        let mut builder = crate::features::CreatureBuilder::new("Caster", 15, 30);
        builder.set_spellcasting(crate::rules::SpellCastingProfile::new(Ability::Wis, 4, 3));
        plugin
            .apply(&mut builder)
            .expect("spellcasting is declared");

        let action = builder.creature.actions.last().expect("action added");
        assert_eq!(action.name, "True Strike");
    }

    /// The weapon a True Strike is made with brings its own magic bonus and
    /// its own riders: a +3 bow of slaying hits and damages 3 better, and
    /// carries its bonus dice onto the spell's attack.
    #[test]
    fn true_strike_carries_the_wielded_weapons_bonus_and_riders() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "true_strike"
                weapon_dice_count = 1
                weapon_dice_sides = 8
                weapon_damage_kind = "piercing"
                ranged = true
                weapon_bonus = 3
                weapon_traits = ["bonus 3d6 piercing vs dragon"]
                radiant_dice_count = 2
            "#,
        )
        .unwrap();
        let plugin = registry.build_plugin("true_strike", &params).unwrap();
        let mut builder = crate::features::CreatureBuilder::new("Caster", 15, 30);
        builder.set_spellcasting(crate::rules::SpellCastingProfile::new(Ability::Wis, 5, 4));
        plugin.apply(&mut builder).unwrap();
        let action = &builder.creature.actions[0];
        let Effect::Strikes { strike, .. } = &action.effect else {
            panic!("expected strikes");
        };
        assert_eq!(strike.to_hit, 12, "5 + 4 + the weapon's 3");
        assert_eq!(strike.damage[0].bonus, 8, "5 + the weapon's 3");
        assert!(strike.kind.ranged && strike.kind.weapon && strike.kind.spell);
        assert!(matches!(
            action.riders[..],
            [crate::creature::Rider::BonusDamageVsCreatureType { dice_count: 3, .. }]
        ));

        let flat: toml::Value = toml::from_str(
            r#"
                weapon_dice_count = 1
                weapon_dice_sides = 8
                weapon_damage_kind = "piercing"
                radiant_dice_count = 2
                weapon_traits = ["ac 2"]
            "#,
        )
        .unwrap();
        assert!(matches!(
            registry.build_plugin("true_strike", &flat),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn true_strike_requires_its_weapon_and_radiant_dice() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                plugin = "true_strike"
                weapon_damage_kind = "piercing"
            "#,
        )
        .unwrap();
        assert!(matches!(
            registry.build_plugin("true_strike", &params),
            Err(FeatureError::InvalidConfiguration(_))
        ));
    }

    #[test]
    fn true_strike_requires_spellcasting_to_already_be_declared_on_the_creature() {
        let registry = FeatureRegistry::new();
        let params: toml::Value = toml::from_str(
            r#"
                weapon_dice_count = 1
                weapon_dice_sides = 8
                weapon_damage_kind = "piercing"
                radiant_dice_count = 2
            "#,
        )
        .unwrap();
        let plugin = registry.build_plugin("true_strike", &params).unwrap();
        let mut builder = crate::features::CreatureBuilder::new("Not Yet A Caster", 15, 30);
        assert!(matches!(
            plugin.apply(&mut builder),
            Err(FeatureError::PrerequisiteNotMet(_))
        ));
    }
}
