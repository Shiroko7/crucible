# Essence Talent System Mapping in Crucible

This document details how the **Essence Talent System** (covering all 9 essence paths: Acid, Air, Earth, Fire, Lightning, Metal, Poison, Water, Wood, including talent abilities and essence spell progressions) maps into **Crucible**.

---

## 1. Architectural Principles

In accordance with Crucible's core philosophy (see [`DESIGN.md`](../DESIGN.md)):
- **Combatants are structured data**, not hardcoded Rust types or bespoke branches.
- **Composition over duplication**: abilities are expressed via:
  1. **Move DSL clauses** (`strikes`, `hit +N`, `spell`, `slot N`, `save <stat> dc <DC> | half`, `autohit`, `heal`, `temp`, `on fail <cond>`).
  2. **Passive trait phrases** (`ignore resistance <types>`, `downgrade immunity <types> damage`, `push on <types> up to <size>`, `once per turn NdM <type>`, `reduce N <types>`, `reaction ac +N`).
  3. **Composable feature plugins** (`lasting_boon`, `lasting_aura`, `retaliation`, `maximised_damage`, `commanded_double`).
- **Scope**: In alignment with Crucible's engine design, purely non-combat utilities (such as languages, artisan tool proficiencies, crafting, out-of-combat navigation) are out of scope for the combat simulator. All active combat attacks, spells, auras, reactions, buffs, and summon mechanics are fully represented.

---

## 2. Engine Capabilities Added

To achieve complete coverage across all 9 essence paths, two mechanical gaps were closed in `crucible-core`:

### A. `autohit` in Move DSL Grammar
- **Syntax:** `autohit | <damage>`
- **Description:** Damaging effects that strike unconditionally without requiring an attack roll or saving throw (e.g. *Thorny Defence*, *The Usurper's Eye*).
- **Example:**
  ```toml
  [[pc.reactions]]
  name = "Thorny Defence"
  effect = "when hit in melee | autohit | 4d4 piercing"
  ```

### B. `ignore resistance <types>` Trait & Rider
- **Syntax:** `traits = ["ignore resistance fire", "ignore resistance acid, poison"]`
- **Description:** Elemental penetration (e.g., *Shatter Resistance*, *Elemental Adept*, *Exploit Vulnerability*) that treats damage resistance as normal damage.
- **Immunity Downgrade Composition:** When paired with `downgrade immunity <type> damage`, an immune creature is downgraded to resistant, which is then bypassed to normal damage.

---

## 3. Generic Plugin & DSL Mapping Patterns

| Mechanic Category | Essence Examples | Crucible Representation |
| :--- | :--- | :--- |
| **Direct Attack** | *Caustic Bomb*, *Pebble Barrage*, *Steel Tornado*, *Toxic Dart* | Move DSL: `strikes N \| hit +X \| <dice> <kind>` |
| **Spells & Cantrips** | All 161 essence spells (e.g. *Acid Arrow*, *Chain Lightning*, *Fireball*) | Move DSL: `spell \| slot N \| save <stat> dc <DC> \| half \| <dice> <kind>` |
| **Autohit Retaliation** | *Thorny Defence*, *The Usurper's Eye* | Reaction DSL: `when hit in melee \| autohit \| <dice> <kind>` |
| **Save Retaliation** | *Static Shock*, *Toxic Skin Secretion* | `retaliation` plugin or `[pc.reactions]` with `when hit in melee \| save ...` |
| **Elemental Coating / Stance** | *Acidic Embrace*, *Stone Armor*, *Ice Form*, *Verdant Armor* | `lasting_boon` plugin (providing AC, damage riders, and resistances) |
| **Persistent Hazards / Fields** | *Miasmic Cloud*, *Scorched Ground*, *Martyr's Flame*, *Sandstorm* | `lasting_aura` plugin (start-of-turn or end-of-turn saving throw + damage) |
| **Elemental Doubles / Summons**| *Apex Toxinator* Snake, Thalassios Clones | `commanded_double` plugin (custom HP, AC, attacks, and commands) |
| **Damage Maximization** | *Lightning Overload*, *Flame Burst* | `maximised_damage` plugin (spending essence resource pool) |
| **Shove / Forced Movement** | *Shock Wave*, *Gale Blast*, *Repulsion* | Trait DSL: `push on <kind> up to <size>` |
| **Damage Soaking / Reduction**| *Iron Resilience*, *Stone Skin*, *Barkskin* | Trait DSL: `reduce N <types>` or `lasting_boon` with resistances |
| **Reactive Parry / Ward** | *Deflecting Palm*, *Shield of Winds* | Trait DSL: `reaction ac +N vs <trigger>` |

---

## 4. Path-by-Path Essence Mapping

### 1. Acid Essence
*Mastery of dissolution, corrosion, and caustic hazards.*

- **Abilities:**
  - *Caustic Vials / Acid Splash*: Action Move DSL (`spell | hit +X | 1d6 acid`).
  - *Acidic Embrace*: `lasting_boon` adding extra acid damage (`dice_count = 1`, `dice_sides = 6`, `damage_kind = "acid"`, `weapon_only = true`).
  - *Corrosive Aura*: `lasting_aura` dealing acid damage on turn start (`effect = "save dex dc X | half | 2d6 acid"`).
  - *Dissolving Touch*: Action Move DSL (`strikes 1 | hit +X | 3d8 acid`).
  - *Armor Dissolution*: Trait `ignore resistance acid`.
  - *Acid Immunity*: `pc.immune = ["acid"]`.
- **Spells:** *Acid Splash, Tasha's Caustic Brew, Melf's Acid Arrow, Hunger of Hadar, Vitriolic Sphere* -> standard `spell` Move DSL with appropriate slot levels, saving throws, and damage dice.

### 2. Air Essence
*Control over winds, aerial agility, and pressure.*

- **Abilities:**
  - *Wind Ward / Zephyr Cloak*: Trait `reaction ac +3 vs ranged weapon` or `lasting_boon` with AC bonus.
  - *Gale Push / Gust Step*: Trait `push on bludgeoning, thunder up to large`.
  - *Suffocating Vacuum*: Move DSL with save and condition (`save con dc X | on fail incapacitated`).
  - *Evasion of Winds*: Trait `evasion dex`.
  - *Flight / Aerial Superiority*: Combat mobility (represented via reach and ranged targeting).
- **Spells:** *Gust, Feather Fall, Gust of Wind, Wind Wall, Fly, Whirlwind* -> Move DSL `spell | slot N | save str/dex dc X ...`.

### 3. Earth Essence
*Unyielding stone, physical density, and seismic disruption.*

- **Abilities:**
  - *Stone Armor*: `lasting_boon` granting `ac = 3` and temporary HP soaking.
  - *Iron Resilience*: Trait `reduce 3 bludgeoning, piercing, slashing`.
  - *Seismic Tremor / Ground Slam*: Move DSL `spell | save dex dc X | half | 3d8 bludgeoning | on fail prone`.
  - *Stone Ward*: `[pc.reactions]` or trait `reaction ac +4 vs melee`.
  - *Earthen Meld / Burrowing*: Tactically out of targeting reach / defensive stance.
- **Spells:** *Mold Earth, Earth Tremor, Spike Growth, Erupting Earth, Stoneskin, Wall of Stone, Earthquake* -> Move DSL `spell | slot N | save dex dc X ...`.

### 4. Fire Essence
*Aggression, explosive heat, and spreading combustion.*

- **Abilities:**
  - *Flame Burst / Pyromaniac*: `maximised_damage` plugin spending essence points.
  - *Elemental Adept (Fire)*: Trait `ignore resistance fire` and `downgrade immunity fire damage`.
  - *Scorched Ground / Flame Aura*: `lasting_aura` with start-of-turn fire damage.
  - *Blazing Retaliation*: `retaliation` plugin (`trigger = "melee"`, `damage = "2d8 fire"`).
  - *Ignite / Burning Dot*: Move DSL `spell | hit +X | 1d10 fire` with recursive rider or aura.
- **Spells:** *Fire Bolt, Burning Hands, Scorching Ray, Fireball, Wall of Fire, Immolation, Delayed Blast Fireball* -> Move DSL `spell | slot N | save dex dc X | half | Nd6 fire`.

### 5. Lightning Essence
*High-voltage discharges, speed, and magnetic propulsion.*

- **Abilities:**
  - *Lightning Shove*: Trait `push on lightning up to large`.
  - *Lightning Overload / Destructive Wrath*: `maximised_damage` plugin buying maximized lightning damage.
  - *Static Discharge*: `retaliation` plugin (`trigger = "melee"`, `damage = "2d8 lightning"`).
  - *Chain Reaction / Arc Strike*: Move DSL with multiple strikes or save with multiple targets (`strikes 3 | hit +X | 1d12 lightning`).
  - *Shatter Lightning Resistance*: Trait `ignore resistance lightning`.
- **Spells:** *Shocking Grasp, Witch Bolt, Lightning Bolt, Storm Sphere, Chain Lightning* -> Move DSL `spell | slot N | save dex dc X | half | Nd6 lightning`.

### 6. Metal Essence
*Forged precision, blade mastery, and metallic warding.*

- **Abilities:**
  - *Steel Tornado / Blade Flurry*: Move DSL `strikes 3 | hit +X | 1d8+4 slashing`.
  - *Adamantine Density*: Trait `reduce 3 bludgeoning, piercing, slashing` and immunity to critical hits.
  - *Weapon Enchantment / Keen Edge*: `lasting_boon` adding weapon damage bonus.
  - *Magnetic Deflection*: Trait `reaction ac +4 vs ranged weapon attacks`.
  - *Serrated Strikes*: Trait `once per turn 1d8 slashing with a weapon`.
- **Spells:** *Blade Ward, Cloud of Daggers, Heat Metal, Conjure Barrage, Blade Barrier* -> Move DSL `spell | slot N ...`.

### 7. Poison Essence
*Toxins, debilitation, and biological corruption.*

- **Abilities:**
  - *Apex Toxinator (Summon)*: `commanded_double` plugin summoning a Viper / Snake horror with bite attacks.
  - *Miasmic Cloud*: `lasting_aura` dealing poison damage (`save con dc X | half | 2d6 poison`).
  - *Toxic Skin Secretion*: Reaction `when hit in melee | save con dc X | on fail poisoned`.
  - *Toxic Penetration*: Trait `ignore resistance poison` and `downgrade immunity poison damage, poisoned condition`.
  - *Venomous Strike*: Move DSL `strikes 1 | hit +X | 1d8 piercing + 2d6 poison`.
- **Spells:** *Poison Spray, Ray of Sickness, Protection from Poison, Stinking Cloud, Cloudkill* -> Move DSL `spell | slot N | save con dc X ...`.

### 8. Water Essence
*Fluid adaptability, healing flow, and tidal force.*

- **Abilities:**
  - *Ice Form / Frozen Armor*: `lasting_boon` granting AC bonus and cold resistance.
  - *Tidal Wave / Water Surge*: Move DSL `spell | save str dc X | half | 4d8 bludgeoning | on fail prone` with push trait.
  - *Healing Surge / Restoration*: Move DSL `heal 3d8+4` or bonus action heal.
  - *Water Clones / Thalassios*: `commanded_double` plugin summoning water duplicates.
  - *Frost Retaliation*: `retaliation` plugin (`trigger = "melee"`, `damage = "2d6 cold"`).
- **Spells:** *Ray of Frost, Fog Cloud, Rime's Binding Ice, Tidal Wave, Ice Storm, Cone of Cold, Tsunami* -> Move DSL `spell | slot N ...`.

### 9. Wood Essence
*Regrowth, entangling roots, and thorny warding.*

- **Abilities:**
  - *Thorny Defence*: Reaction Move DSL with `autohit`: `when hit in melee | autohit | 4d4 piercing`.
  - *Verdant Armor*: `lasting_boon` with AC bonus and temporary HP.
  - *Entangling Roots*: Move DSL `spell | save str dc X | on fail restrained`.
  - *Moonlit Verdant Beam*: Move DSL `spell | slot 5 | save con dc X | half | 6d8 radiant + 4d6 piercing`.
  - *Photosynthetic Regrowth*: Move DSL `heal 2d8+3` or passive regeneration trait.
- **Spells:** *Thorn Whip, Entangle, Barkskin, Plant Growth, Grasping Vine, Wrath of Nature* -> Move DSL `spell | slot N ...`.

---

## 5. Verification & Testing

Every pattern and mechanism documented above is backed by automated tests:
- **`crates/crucible-core/tests/essence_paths.rs`**: End-to-end integration tests verifying TOML loading, simulation, `autohit` melee reactions, resistance ignoring, immunity downgrading, `lasting_boon` armor and coatings, `lasting_aura` hazardous clouds, and `commanded_double` summons.
- **`crates/crucible-core/src/creature/rider/immunity.rs`**: Unit tests verifying `Rider::IgnoreResistance` and its composition with `Rider::DowngradeImmunity`.
- **`crates/crucible-core/src/dsl/grammar/moves.rs`**: Unit tests verifying `autohit` parsing in actions and reactions.
- **`crates/crucible-core/src/dsl/grammar/traits.rs`**: Unit tests verifying `ignore resistance <types>` trait parsing.
