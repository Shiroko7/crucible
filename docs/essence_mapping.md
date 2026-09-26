# V2 Cultivation Paths System Mapping in Crucible

This document details how the **Essence Talent System V2 (Cultivation Paths)** maps into **Crucible**. It covers all 16 cultivation paths across the 3 traditions/families (**Primordial**, **Divine**, and **Immortal**), including all 202 custom talents (136 active, 66 passive), 39 cantrips, and 336 leveled spells (577 entries total).

---

## 1. Architectural Principles & V2 Overhaul

The V2 overhaul transforms the original nine standalone elemental pools into a unified, campaign-focused cultivation model:

- **Three Families with Shared Essence Pools:**
  - **Primordial Family** (Wood, Fire, Earth, Metal, Water, Sky) shares the `primordial` Essence pool.
  - **Divine Family** (Lunar, Love, Ruin, Pestilence, Shadow, Tempest, Providence) shares the `divine` Essence pool.
  - **Immortal Family** (Alchemy, Heart, Mysteries) shares the `immortal` (or `human`) Essence pool.
  - Every learned path within a family contributes to its shared pool, and any learned ability in the family spends from it (`cost primordial N`, `cost divine N`, `cost immortal N`).
- **Combatants are Structured Data:** Abilities are expressed cleanly through Crucible's declarative layers:
  1. **Move DSL Clauses:** `strikes N`, `hit +X`, `spell`, `slot N`, `save <stat> dc <DC> | half`, `autohit`, `heal`, `temp`, `on fail <cond>`, `targets N`, `cost <pool> N`.
  2. **Passive Trait Phrases:** `ignore resistance <types>`, `downgrade immunity <types> damage`, `push on <types> up to <size>`, `once per turn NdM <type>`, `reduce NdM+B <types>`, `reaction ac +N [vs <trigger>] [until next turn]`, `saves N`, `resistance <types>`, `evasion <stat>`, `quarry NdM <type>`.
  3. **Composable Feature Plugins:** `lasting_boon` (buffs, coatings, stances), `lasting_aura` (persistent hazard zones), `retaliation` (reactive defenses), `maximised_damage` (essence-fueled damage spikes), `commanded_double` (homunculi, clones, and summons).
- **Scope:** Purely non-combat utility features (languages, artisan tools, out-of-combat navigation) operate outside combat simulation; all attacks, martial techniques, spells, auras, reactions, buffs, and summon mechanics are fully represented.

---

## 2. Mechanic Category & Plugin Mapping Patterns

| Mechanic Category | Cultivation Path Examples | Crucible Representation |
| :--- | :--- | :--- |
| **Direct & Multi-Attacks** | *Lightning Javelin*, *Pebble Barrage*, *Steel Tornado*, *Erosion of Being* | Move DSL: `strikes N \| hit +X \| <dice> <kind>` or `save ...` |
| **Martial Weapon Replacements** | *Radiant Vein Blade I–III*, *Concussive Smash*, *Pommel Strike*, *Lacerating Edge* | Move DSL: `cost immortal N \| strikes N \| weapon <type> \| hit +X \| <dice> <kind> \| on fail <cond>` |
| **Unarmed Strike Progressions** | *Stone Fist I–V* (1d4 $\to$ 1d6 $\to$ 1d8 $\to$ 1d10 $\to$ 1d12) | Move DSL: `strikes 2 \| hit +X \| NdS+STR bludgeoning` (magical) |
| **Spells & Cantrips** | All 39 cantrips & 336 leveled spells across all 16 paths | Move DSL: `spell \| slot N \| save <stat> dc <DC> \| half \| <dice> <kind>` or spell plugins |
| **Autohit Retaliation** | *Thorny Defence*, *Supreme Heart-Reflection Sword* | Reaction DSL: `when hit in melee \| [cost <res> N \|] autohit \| <dice> <kind>` |
| **Damage Soaking / Reduction**| *Magnetic Shield*, *Iron Resilience*, *Endurance of Ilmater*, *Stone Skin* | Trait DSL: `reduce NdM+B <types>` or `lasting_boon` |
| **Elemental Coatings & Stances**| *Acidic Embrace*, *Stone Armor*, *Ice Form*, *Verdant Armor*, *Keen Edge* | `lasting_boon` plugin (AC bonuses, weapon damage riders, resistances) |
| **Persistent Hazards & Auras** | *Martyr's Flame Aura*, *Miasmic Cloud*, *Scorched Ground*, *Sandstorm*, *Pestilent Cloud* | `lasting_aura` plugin (start-of-turn or end-of-turn save + damage) |
| **Summons & Clones** | *Apex Toxinator* Snake Horror, Thalassios *Water Clones* | `commanded_double` plugin (custom HP, AC, attacks, and commands) |
| **Spacetime Folding & Banish** | *Mouth of the Well*, *Fishing the Moon from the Well*, *Static Slipstream* | Move DSL: `save cha/dex dc DC \| ... \| on fail banished for 1 minute` / `pulled` |
| **Assassination & Quarry Marks**| *Mark for Death*, *Blood Tithe*, *Crown of Agony* | Trait DSL: `quarry 1d6 necrotic`, `retaliation` plugin, Move DSL `temp NdM` |
| **Elemental Penetration** | *Elemental Adept*, *Armor Dissolution*, *Toxic Penetration* | Trait DSL: `ignore resistance <types>` & `downgrade immunity <types> damage` |
| **Reactive Parry / Ward** | *Wind Barrier*, *Illusory and Real*, *Glacial Shield* | Trait DSL: `reaction ac +N [until next turn]` or Move DSL `temp NdM` |
| **Damage Maximization** | *Lightning Overload*, *Destructive Wrath* | `maximised_damage` plugin (spending from shared family pool) |
| **Shove / Forced Movement** | *Thunderous Roar*, *Thunderous Strike*, *Arc Chain* | Trait DSL: `push on <types> up to <size>` or Move DSL `on fail pushed` |

---

## 3. Path-by-Path Cultivation Mapping

### A. Primordial Family (Shared Pool: `primordial`)

#### 1. Wood Path (44 entries: 10 talents [5 act / 5 pass], 4 cantrips, 30 spells)
- **Concept:** Plant growth, wild land spirits, thorny wards, and nature's endurance.
- **Key Talents:**
  - *Thorny Defence:* Reaction Move DSL: `when hit in melee | autohit | 4d4 piercing`.
  - *Verdant Armor:* `lasting_boon` granting AC bonus and start-of-turn temporary HP.
  - *Needle Barrage:* Move DSL: `save dex dc DC | 3d8 piercing | half`.
  - *Vine Manipulation:* Move DSL: `save str dc DC | on fail restrained`.
  - *Tree Form:* `lasting_boon` with AC bonus, bludgeoning/piercing resistance, and slam strikes.
- **Cantrips & Spells:** *Thorn Whip, Shillelagh, Root Grab, Druidcraft*; *Entangle, Barkskin, Spike Growth, Plant Growth, Grasping Vine, Wrath of Nature, Wall of Thorns, Shapechange* $\to$ Move DSL and spell plugins.

#### 2. Fire Path (34 entries: 9 talents [5 act / 4 pass], 5 cantrips, 20 spells)
- **Concept:** Aggression, combustion, explosive heat, and incinerating momentum.
- **Key Talents:**
  - *Inferno Disengage:* Action Move DSL: `autohit | 2d6 fire`.
  - *Flame Lash:* Move DSL: `spell | hit +X | 2d8 fire`.
  - *Scorched Ground:* `lasting_aura` dealing 3d6 fire damage at turn start.
  - *Ember Strike:* Trait `once per turn 1d12 fire with a weapon` or slot-empowered rider.
  - *Ember's Resilience:* Reaction rerolling failed saves at the cost of taking self-damage.
  - *Burning Fury:* Advantage on attack rolls after taking fire damage.
- **Cantrips & Spells:** *Fire Bolt, Green-Flame Blade, Create Bonfire, Control Flames, Produce Flame*; *Burning Hands, Hellish Rebuke, Scorching Ray, Fireball, Wall of Fire, Immolation, Fire Storm, Delayed Blast Fireball* $\to$ Move DSL.

#### 3. Earth Path (39 entries: 15 talents [6 act / 9 pass], 2 cantrips, 22 spells)
- **Concept:** Seismic density, stone resilience, tremors, and unyielding mass.
- **Key Talents:**
  - *Pebble Barrage:* Move DSL: `save dex dc DC | 3d8 bludgeoning | half`.
  - *Earthen Ward:* Reaction Move DSL: `temp 1d10+CON`.
  - *Stone Armor:* `lasting_boon` providing AC boost and physical damage resistance.
  - *Earthen Grasp:* Move DSL: `save str dc DC | on fail restrained for 1 minute`.
  - *Sandstorm:* `lasting_aura` dealing 2d8 slashing/bludgeoning damage in a 20ft radius.
  - *Immovable Mountain:* Immunity to prone, grappled, restrained, paralyzed, and stunned.
  - *Earthen Resilience:* Con save proficiency (`pc.saves.con = ...`).
- **Cantrips & Spells:** *Magic Stone, Mold Earth*; *Earth Tremor, Maximilian's Earthen Grasp, Erupting Earth, Stoneskin, Wall of Stone, Move Earth, Earthquake* $\to$ Move DSL.

#### 4. Metal Path (29 entries: 12 talents [11 act / 1 pass], 2 cantrips, 15 spells)
- **Concept:** Forged blades, magnetism, density, and karmic retribution.
- **Key Talents:**
  - *Magnetic Shield:* Trait `reduce 1d10+CON acid, bludgeoning, cold, fire, force, lightning, necrotic, piercing, poison, psychic, radiant, slashing, thunder`.
  - *Steel Tornado:* Move DSL: `cost primordial 2 | save dex dc DC | 4d8 slashing | half`.
  - *Weight of Lives:* Move DSL: `cost primordial 1 | save cha dc DC | 4d8 force | half | on fail restrained for 2 rounds`.
  - *Keen Edge:* `lasting_boon` adding +1 to attack and damage rolls.
  - *Gem Cocoon:* Move DSL: `temp 40` (soaking incoming attacks).
- **Cantrips & Spells:** *Blade Ward, Sword Burst*; *Cloud of Daggers, Heat Metal, Conjure Barrage, Blade Barrier, Steel Wind Strike* $\to$ Move DSL.

#### 5. Water Path (43 entries: 12 talents [8 act / 4 pass], 3 cantrips, 28 spells)
- **Concept:** Fluid adaptability, tidal force, glacial protection, and water duplicates.
- **Key Talents:**
  - *Tide's Reflection Art I–III (Thalassios):* `commanded_double` plugin summoning Water Clones with slam attacks, reactive 3d8 cold detonations, and position swaps.
  - *Ice Form:* `lasting_boon` granting cold and fire damage resistance.
  - *Glacial Shield:* Reaction Move DSL: `slot 1 | temp 10`.
  - *Restorative Rain & Tidal Surge:* Move DSL `heal 2d6` / `heal 3d8+WIS`.
  - *Aura of the Deep Bulwark:* Defensive aura granting fire resistance and ranged attack protection.
- **Cantrips & Spells:** *Ray of Frost, Frostbite, Shape Water*; *Fog Cloud, Rime's Binding Ice, Tidal Wave, Watery Sphere, Ice Storm, Cone of Cold, Tsunami* $\to$ Move DSL.

#### 6. Sky Path (49 entries: 17 talents [12 act / 5 pass], 4 cantrips, 28 spells)
- **Concept:** Atmospheric pressure, flight, gale speed, sound, and direct lightning discharge.
- **Key Talents:**
  - *Lightning Javelin:* Move DSL: `spell | ranged | hit +X | 5d12 lightning`.
  - *Arc Chain:* Move DSL: `cost primordial 2 | save dex dc DC | 3d8 lightning | half | targets 3`.
  - *Thunderous Roar:* Move DSL: `save con dc DC | 2d8 thunder | half | on fail pushed`.
  - *Conductive Touch:* Trait: `once per turn 2d12 lightning with a weapon`.
  - *Wind Barrier:* Trait: `reaction ac 2 until next turn`.
  - *Lightning Shove:* Trait: `push on lightning, thunder up to large`.
  - *Just Passing By:* Reaction dissolving into wind, avoiding weapon attacks.
- **Cantrips & Spells:** *Shocking Grasp, Gust, Thunderclap, Message*; *Feather Fall, Witch Bolt, Gust of Wind, Shatter, Lightning Bolt, Fly, Storm Sphere, Chain Lightning* $\to$ Move DSL.

---

### B. Divine Family (Shared Pool: `divine`)

#### 7. Lunar Path (25 entries: 7 talents [6 act / 1 pass], 1 cantrip, 17 spells)
- **Concept:** Selûne’s moon, stars, navigation, dreams, celestial foresight, and toxic moonlight.
- **Key Talents:**
  - *Moonlit Verdant Beam:* Move DSL: `spell | ranged | hit +X | 4d8 radiant, 2d6 poison`.
  - *Moonfall Condemnation:* Move DSL: `cost divine 4 | save dex dc DC | 6d10 radiant | half`.
  - *Lunar Wind Spiral:* Move DSL: `save dex dc DC | 5d8 radiant, 3d8 bludgeoning | half`.
  - *Waning Moon Sabers:* Move DSL: `spell | strikes 3 | hit +X | 2d8 radiant`.
  - *Lunar Tide:* Reaction granting save advantage / damage reduction against area effects.
- **Cantrips & Spells:** *Dancing Lights*; *Guiding Bolt, Moonbeam, Crown of Stars, Foresight, Dream of the Blue Veil* $\to$ Move DSL and spell plugins.

#### 8. Love Path (23 entries: 2 talents [2 act / 0 pass], 2 cantrips, 19 spells)
- **Concept:** Affection, emotional bonds, kinship with beasts, beauty, and emotional manipulation.
- **Key Talents:**
  - *Attraction / Disdain:* Move DSL: `spell | save wis dc DC | on fail charmed`.
  - *Tide of Emotions:* Move DSL: `spell | save wis dc DC | on fail frightened`.
- **Cantrips & Spells:** *Friends, Vicious Mockery*; *Charm Person, Animal Friendship, Speak with Animals, Calm Emotions, Enthrall, Dominate Beast, Dominate Person, Otto's Irresistible Dance* $\to$ Move DSL.

#### 9. Ruin Path (41 entries: 14 talents [11 act / 3 pass], 4 cantrips, 23 spells)
- **Concept:** Assassination, murder, pain, thievery, karmic debt, and death curses.
- **Key Talents:**
  - *Mark for Death:* Trait `quarry 1d6 necrotic` (or `Condition::Quarry`).
  - *Whip's Kiss:* Move DSL: `strikes 1 | hit +X | 1d4+DEX slashing, 2d6 necrotic | heal 1d6`.
  - *Heart Crusher Grip (Nilo):* Move DSL: `cost divine 1 | save con dc DC | 4d8 necrotic | half | on fail incapacitated`.
  - *Crown of Agony:* `retaliation` plugin (`trigger = "melee"`, `damage = "2d8 psychic"`).
  - *Blood Tithe:* Move DSL: `temp 1d6` gained upon damaging enemies.
  - *Frightful Pursuit:* Advantage on attack rolls against frightened creatures.
  - *Mass Inflict Wounds:* Move DSL: `spell | slot 5 | save con dc DC | 5d10 necrotic | half`.
- **Cantrips & Spells:** *Toll the Dead, Chill Touch, Mind Sliver, Mage Hand*; *Inflict Wounds, Ray of Sickness, Hellish Rebuke, Blindness/Deafness, Bestow Curse, Blight, Finger of Death, Power Word Kill* $\to$ Move DSL.

#### 10. Pestilence Path (38 entries: 13 talents [9 act / 4 pass], 2 cantrips, 23 spells)
- **Concept:** Biological toxins, contagion, miasmic disease, and pestilent decay.
- **Key Talents:**
  - *Toxic Skin Secretion I–III:* Reaction `when hit in melee | save con dc DC | 2d6 poison | on fail poisoned` or `retaliation`.
  - *Venomous Strike:* `lasting_boon` coating weapon for +2d6 poison damage.
  - *Pestilent Cloud & Creeping Pestilence:* `lasting_aura` dealing poison damage each round.
  - *Venomous Precision:* Advantage on attack rolls against poisoned creatures.
  - *Toxic Penetration:* Trait `ignore resistance poison` and `downgrade immunity poison damage, poisoned condition`.
  - *Blessed Immunity:* `pc.immune = ["poison"]`, `pc.condition_immune = ["poisoned"]`.
- **Cantrips & Spells:** *Poison Spray, Infestation*; *Ray of Enfeeblement, Stinking Cloud, Contagion, Cloudkill, Insect Plague, Harm* $\to$ Move DSL.

#### 11. Shadow Path (44 entries: 12 talents [6 act / 6 pass], 1 cantrip, 31 spells)
- **Concept:** Shar’s darkness, concealment, void gravity, and Netherdark martial arts.
- **Key Talents:**
  - *Netherdark Fist I–V:* Scaled force martial strikes:
    - *I:* `cost divine 1 | strikes 1 | hit +X | 2d10 force`.
    - *II (Voidstride):* `cost divine 2 | strikes 1 | hit +X | 4d10 force`.
    - *III (Gravitic Collapse):* `cost divine 3 | strikes 1 | hit +X | 6d10 force | on fail prone`.
    - *IV (Reality Rend):* `cost divine 4 | strikes 1 | hit +X | 8d10 force`.
    - *V (Absolute Oblivion):* `cost divine 5 | strikes 1 | hit +X | 10d10 force | on fail incapacitated`.
  - *Shadowflame Dream:* Move DSL: `save cha dc DC | 6d8 necrotic | half`.
  - *Darkbolt:* Move DSL: `spell | ranged | hit +X | 3d8 necrotic`.
  - *Armor of Darkness & Umbral Form:* `lasting_boon` providing AC boost and physical damage resistance.
- **Cantrips & Spells:** *Minor Illusion*; *Darkness, Invisibility, Shadow Blade, Pass Without Trace, Phantasmal Killer, Creation, Maddening Darkness, Weird* $\to$ Move DSL.

#### 12. Tempest Path (25 entries: 7 talents [5 act / 2 pass], 1 cantrip, 17 spells)
- **Concept:** Macro weather systems, storm fronts, heavy thunder, and storm strikes.
- **Key Talents:**
  - *Thunderous Strike:* Move DSL: `strikes 1 | hit +X | 1d8+STR bludgeoning, 2d8 thunder | on fail pushed`.
  - *Extinguishing Lightning:* Move DSL: `cost divine 3 | save dex dc DC | 3d12 lightning, 3d12 necrotic | half`.
  - *Thunderous Push:* Trait: `push on thunder up to large`.
  - *Thunderous Retaliation:* `retaliation` plugin (`trigger = "melee"`, `damage = "2d8 thunder"`).
- **Cantrips & Spells:** *Lightning Lure*; *Thunderwave, Fog Cloud, Call Lightning, Sleet Storm, Lightning Arrow, Control Water, Destructive Wave, Storm of Vengeance* $\to$ Move DSL & plugins.

#### 13. Providence Path (34 entries: 9 talents [7 act / 2 pass], 3 cantrips, 22 spells)
- **Concept:** Healing, enduring protection, hope, self-sacrifice, and freedom from restraint.
- **Key Talents:**
  - *Martyr’s Flame Aura:* `lasting_aura` dealing 3d8 radiant damage to enemies on turn start.
  - *Touch of Respite & Gift of Enduring Faith:* Move DSL `heal 2d8+WIS` / `heal 4d8+WIS`.
  - *Boon of Fortitude:* `lasting_boon` granting temporary hit points and resistance.
  - *Endurance of Ilmater:* Trait `reduce 3 bludgeoning, piercing, slashing`.
  - *Unbroken Spirit:* Advantage on saving throws against frightened and charmed conditions.
- **Cantrips & Spells:** *Spare the Dying, Guidance, Resistance*; *Cure Wounds, Healing Word, Bless, Lesser Restoration, Beacon of Hope, Mass Cure Wounds, Heal, Mass Heal* $\to$ Move DSL & plugins.

---

### C. Immortal Family (Shared Pool: `immortal` / `human`)

#### 14. Alchemy Path (48 entries: 27 talents [16 act / 11 pass], 3 cantrips, 18 spells)
- **Concept:** Refinement, biochemical transformation, mutagens, acids, and homunculi.
- **Key Talents:**
  - *Caustic Bomb:* Action Move DSL: `cost immortal 1 | save dex dc DC | 4d6 acid | half`.
  - *Mutagen Formulas (STR, DEX, CON, INT, WIS, CHA):* `lasting_boon` granting stat increases and corresponding trade-offs.
  - *Apex Toxinator:* `commanded_double` plugin summoning a monstrous Snake Horror with venomous bite attacks.
  - *Acidic Embrace:* `lasting_boon` adding 2d4 acid damage to weapon strikes.
  - *Miasmic Cloud:* `lasting_aura` dealing acid damage in a 10ft radius.
  - *Erosion of Being:* Move DSL: `cost immortal 2 | save con dc DC | 6d8 acid | half`.
  - *Seven-Color Elixir:* Bonus action buff providing resistances, healing, or damage riders.
- **Cantrips & Spells:** *Acid Splash, Primal Savagery, Mending*; *Tasha's Caustic Brew, Chromatic Orb, Melf's Acid Arrow, Enlarge/Reduce, Vitriolic Sphere, Animal Shapes, True Polymorph* $\to$ Move DSL.

#### 15. Heart Path (33 entries: 32 talents [24 act / 8 pass], 1 cantrip, 0 spells)
- **Concept:** Embodied martial discipline, unarmed combat styles, and weapon technique mastery.
- **Key Talents:**
  - *Stone Fist I–V (Monk Progression):* Unarmed strike progression dealing 1d4, 1d6, 1d8, 1d10, and 1d12 magical bludgeoning damage.
  - *Radiant Vein Blade I–III (Kelemvor Forms):*
    - *I (Twin Strike):* `cost immortal 1 | strikes 2 | weapon <type> | hit +X | 1d8+STR slashing, 1d8 radiant`.
    - *II (Aegis):* `cost immortal 2 | strikes 1 | weapon <type> | hit +X | 1d8+STR slashing, 2d8 radiant` and trait `reaction ac 2 until next turn`.
    - *III (Cascading Judgment):* `cost immortal 3 | strikes 1 | weapon <type> | hit +X | 1d8+STR slashing, 3d8 radiant` plus 3d8 radiant area wave.
  - *Pommel Strike:* Move DSL: `cost immortal 1 | strikes 1 | hit +X | 1d4+STR bludgeoning | on fail stunned for 1 round`.
  - *Lacerating Edge:* Move DSL: `cost immortal 1 | strikes 1 | hit +X | 1d8+STR slashing, 1d8 slashing`.
  - *Supreme Heart-Reflection Sword:* Reaction Move DSL: `when hit in melee | cost immortal 4 | autohit | 4d8 force` and trait `reduce 1d10+DEX ...`.
  - *Heartstopper (Sternum Shatter):* Move DSL: `cost immortal 3 | strikes 1 | hit +X | 1d10+STR bludgeoning, 4d10 bludgeoning | on fail incapacitated for 1 round`.
  - *Divine Ocean-Splitting Record:* Move DSL: `cost immortal 5 | strikes 1 | hit +X | 2d6+STR slashing, 8d10 force`.
  - *Sky-Piercing Execution:* Reaction Move DSL: `when airborne | strikes 1 | hit +X | 1d8+STR piercing, 3d10 piercing`.
  - *Unwavering Belief:* Flat save bonus trait `saves 2` or `pc.saves` full proficiency.
  - *Venomous Bite (Gio):* Move DSL: `strikes 1 | hit +X | 1d6+STR piercing, 2d6 poison | on fail poisoned for 1 minute`.
- **Cantrip:** *True Strike* (exceptional martial strike cantrip; no leveled spells).

#### 16. Mysteries Path (28 entries: 4 talents [3 act / 1 pass], 1 cantrip, 23 spells)
- **Concept:** Spacetime folding, gravity inversion, extradimensional spaces, and illusory reality.
- **Key Talents:**
  - *Mouth of the Well:* Move DSL: `cost immortal 1 | save dex dc DC | on fail pulled and restrained`.
  - *Illusory and Real:* Trait `reaction ac 10 vs any attack` (swapping places with a reflection).
  - *Unshaken Conviction:* Trait `resistance psychic` and advantage vs charm/frighten.
  - *Fishing the Moon from the Well:* Grandmaster technique: `cost immortal 5 | save cha dc DC | 10d10 force | half | on fail banished for 1 minute`.
- **Cantrips & Spells:** *Prestidigitation*; *Rope Trick, Misty Step, Haste, Slow, Dimension Door, Teleport, Reverse Gravity, Time Stop, Paradox, Time Ravage, Gate* $\to$ Move DSL.

---

## 4. Family Pools and Resource Spending

Crucible parses and enforces resource costs through its resource pool indexing. Characters define their shared family pools under `[pc.resources]`:

```toml
[pc.resources]
primordial = 12   # Shared pool for Wood, Fire, Earth, Metal, Water, Sky
divine = 10       # Shared pool for Lunar, Love, Ruin, Pestilence, Shadow, Tempest, Providence
immortal = 8      # Shared pool for Alchemy, Heart, Mysteries
```

Individual abilities declare their expenditure using `cost <pool> <amount>`:
- `effect = "cost primordial 1 | strikes 1 | ..."`
- `effect = "cost immortal 4 | autohit | 4d8 force"`
- `effect = "cost divine 3 | save dex dc 15 | 6d10 force | half"`

Plugins such as `maximised_damage` and `commanded_double` similarly link directly to the family resource:
```toml
[[pc.features]]
plugin = "maximised_damage"
name = "Destructive Wrath"
effect = "spell | hit +8 | 5d12 lightning"
kinds = ["lightning"]
resource = "primordial"
cost = 2
```

---

## 5. Verification & Testing

The V2 Cultivation Paths support is backed by comprehensive integration testing in:
- **`crates/crucible-core/tests/essence_paths.rs`**: End-to-end integration test suite exercising:
  1. **Primordial Family Cultivator:** Combined Earth, Fire, Wood, and Sky abilities spending from `primordial` (Stone Armor boons, Thorny Defence autohit reactions, Fire Bolt, Lightning Javelin, and elemental penetration).
  2. **Divine Family Cultivator:** Ruin quarry mechanics (`Mark for Death`), Shadow void force strikes (*Netherdark Fist*), Crown of Agony retaliation, and Pestilence toxic secretions.
  3. **Immortal Family Cultivator:** Heart martial techniques (*Stone Fist* unarmed strikes, *Radiant Vein Blade* strike sequences, *Supreme Heart-Reflection Sword* reaction counters), Mysteries spacetime manipulation (*Mouth of the Well*, *Fishing the Moon* force/banish), and Alchemy *Caustic Bombs* with Mutagen boons.
  4. **Multi-element Penetration & Resistance Downgrades:** Treating immunity as resistance and ignoring resistance.
- **`crates/crucible-core/src/dsl/grammar/lex.rs`**: Unit tests verifying both comma-separated and plus-separated multi-damage rolls (e.g. `4d8 radiant + 2d6 poison`).
