//! A text format for combatants, and a parser for it.
//!
//! Two reasons this exists rather than a pair of hardcoded constants.
//!
//! The licensing one: non-SRD stat blocks cannot be committed, so the tool has
//! to be able to read a creature it has never seen from a file the user owns.
//! The design one is the forcing function `DESIGN.md` describes - if a creature
//! cannot be written down as data, something is missing, and finding that out
//! now is cheaper than finding it out after the ability DSL is built. Writing
//! a level-8 monk in this format is what turned up the need for shared resource
//! pools, on-hit riders and conditions with lifetimes.
//!
//! This is still not that DSL: no triggers of its own, no arithmetic, no
//! references between abilities. It will be replaced.
//!
//! ```text
//! creature: Ogre
//! ac: 11
//! hp: 68
//! initiative: -1
//! saves: str +4, dex -1, con +3, int -3, wis -2, cha -2
//! type: giant
//! size: large
//! immune: fire
//! resource: focus 8
//! trait: evasion dex
//! trait: legendary resistance 3
//! trait: deflect 1d10+7 bludgeoning, piercing, slashing
//! trait: halve attack damage
//! trait: extra damage applies to spell attacks
//! trait: reaction ac 5 vs ranged weapon
//! trait: bonus 3d6 piercing vs dragon
//! trait: ac 2
//! trait: saves 1
//! trait: spell 1
//! trait: resistance fire, cold
//! trait: downgrade immunity poison damage, poisoned condition
//! trait: empower weapon 2d6 poison on poisoned
//! trait: injury poison con dc 13 poisoned disadvantage str for 1 hour
//! trait: once per turn 1d6 piercing
//! trait: once per turn 1d8 thunder with a weapon
//! trait: push on lightning, thunder up to large
//! trait: quarry 1d6 force
//! trait: always succeed dex 3 reaction
//! trait: reaction ac 4 vs melee until next turn
//! trait: reactions 3
//! condition immune: poisoned
//! action: Greatclub | strikes 1 | hit +6 | 2d8+4 bludgeoning
//! action: Longbow | ranged | hit +8 | 1d8+4 piercing | bonus 3d6 piercing vs dragon
//! action: Shortsword | weapon shortsword | finesse | hit +10 | 1d6+6 slashing
//!                    | on hit vexed until end
//! action: Icebrand | weapon icebrand | hit +11 | 1d6+7 slashing or cold
//! action: Guiding Bolt | spell | slot 1 | ranged | hit +7 | 4d6 radiant
//! action: Hold | spell | slot 2 | concentration | save wis dc 15 | on fail paralyzed until save
//! bonus: Potion of Healing | object | cost potions 1 | heal 2d4+2
//! action: Staff | strikes 2 | hit +9 | 1d8+6 bludgeoning
//!              | on hit save con dc 16 stunned once cost focus 1
//! bonus: Patient Defense | cost focus 1 | stance dodging
//! action: Breath | uses 3 | save dex dc 16 | 2d8 cold | half on success
//!               && strikes 1 | hit +9 | 1d8+6 bludgeoning
//! trait: damage threshold 30 cracks weak spot resists bludgeoning, piercing
//! trait: digest 6d6 acid
//! trait: regurgitate 30 con dc 21
//! condition immune: prone, frightened
//! action: Bite | hit +9 | 3d8+5 piercing | on hit swallow large
//!              && stance exposed
//! bonus: Clamp Shut | stance sealed
//! legendary: Squeeze | points 2 | swallowed | 4d10 bludgeoning
//! reaction: Snap | when enemy pulled | hit +9 | 3d8+5 piercing
//! reaction: Spray | when breached | save dex dc 18 | 4d10 piercing | half on success
//! reaction: Wrath | when hit in melee | uses 5 | save dex dc 16 | 2d8 lightning
//!                 | half on success
//! action: Turn | save wis dc 16 | only undead | 5d8 radiant
//!             | on fail frightened and incapacitated until damaged
//! aura: Undertow | save str dc 15 | on fail pulled
//! trait: mouth
//! trait: difficult terrain
//! tactic: hit and run
//! action: Maw | reach mouth | hit +9 | 3d8+5 piercing
//!             && reach near | strikes 2 | hit +9 | 2d8+5 bludgeoning
//!             | on hit save str dc 17 prone and pushed until victim
//! action: Gush | recharge 5 | reach front | save con dc 17 | 8d8 cold | on fail pushed
//! legendary: Chill | reach near | save con dc 17 | 4d6 cold | on fail slowed for 1 round
//! trait: magic resistance
//! trait: advantage against frightened
//! trait: bonus 1d8 necrotic vs quarry
//! trait: redirect attacks wis dc 20
//! bonus: Mark | targets 1 | save wis dc 18 | on fail quarry for 1 minute
//!             | on fail unhealable for 1 minute
//! action: Bloom | save wis dc 18 | 5d10 psychic | on fail reeling until victim end
//! legendary: Whisper | targets 1 | save wis dc 18 | on fail frightened until victim end
//!                    | bonus 3d8 psychic vs quarry
//! reaction: Unravel | when enemy casts | spell | slot 3 | save con dc 18
//! reaction: Drink | when targeted by a spell | item | counter
//! reaction: Rebuke | when damaged | save dex dc 15 | 3d10 fire | half on success
//! trait: reflect psychic
//! trait: truesight 30
//! trait: refuses healing
//! trait: first hit 2d6 necrotic
//! resource: rod 10
//! trait: spell store rod up to 5 holds 50
//! action: Unmake | spell | uses 1 | save con dc 18 | 7d8+30 necrotic | half on success
//!                | on kill raise Zombie
//! action: Prison | concentration | targets 1 | save int dc 18
//!                | on fail restrained for 1 minute | escape 10d10 psychic
//! action: Static | save int dc 18 | 8d6 psychic | on fail muddled until save
//! action: Hold | targets 1 | worth 30 | save wis dc 18 | on fail paralyzed until save
//! action: Vanish | spell | concentration | stance invisible
//! legendary: Step | teleport 30
//! trait: resist nonmagical bludgeoning, piercing, slashing
//! trait: magic weapons
//! trait: advantage on initiative
//! trait: passive perception 15
//! trait: in dim light
//! trait: no opportunity attacks
//! trait: phase through 1d10 psychic
//! trait: mirror psychic 10
//! trait: swap when hit 60 gnoll
//! action: Claws | strikes 2 | hit +7 | 1d8+4 slashing | bonus 2d6 psychic with advantage
//! action: Slams | range 10 | strikes 2 | hit +9 | 2d8+5 bludgeoning
//!               | all hit 2d8 psychic save con dc 17 stunned until victim end
//! action: Reap | recharge 5 | range 10 | strikes each | hit +9 | 2d8+5 bludgeoning
//!              | on hit save dex dc 17 prone until victim
//! action: Rush | recharge 5 | strikes 6 | hit +7 | 1d8+4 slashing | no opportunity attacks
//! action: Fold | recharge 6 | range 30 | save wis dc 19 | on fail thrown 60
//!              | blast 10 6d12 psychic | on success 3d12 psychic
//! action: Sword | hit +9 | 1d8+5 slashing | magical
//! bonus: Fade | in dim light | hide +7
//! trait: advantage on dex saves against lightning
//! action: Arc | cost essence 2 | range 120 | targets 1 | save dex dc 17 | 3d8 lightning
//!             | half on success | chains 30
//! action: Ride | range 60 | line 60 | save dex dc 17 | 4d6 lightning | half on success
//! action: Arrow | spell | weapon dagger | ranged | range 20 | takes bonus action | hit +6
//!               | 4d8 lightning | half on miss
//!               && area 10 | spares target | save dex dc 17 | 2d8 lightning | half on success
//! action: Grasp | spell | hit +9 | 3d8 lightning | on hit shocked until victim
//! action: Wrath | cost essence 3 | also cost charges 1 | save con dc 17 | 5d6 thunder
//! ```
//!
//! `&&` joins a move out of several effects, for a Multiattack that is not all
//! the same attack. Move order is meaningful: it is what
//! [`crate::sim::Policy::InOrder`] reads.
//!
//! A strike is a melee weapon attack unless its move says otherwise: `ranged`,
//! `finesse`, `spell` (a spell attack; also marks the move as casting a spell),
//! and `weapon` (a weapon attack made as part of a spell, alongside `spell`).
//! `weapon <name>` also names the blade the swing is made with, which is what
//! an enchantment laid on one weapon rides (see [`crate::creature::Boon`]).
//! `item` and `object` mark a move as activating a magic item or using an
//! object. `components v`, `components verbal, somatic` or `components none`
//! say which of the three a cast needs: a silenced creature can still cast
//! what it does not have to speak, and a spell that never says is read as
//! speaking. Damage written `1d6+7 slashing or cold` is dealt as whichever of
//! the two its target reduces least, chosen per attack.
//!
//! A condition's lifetime, after `on fail` or an `on hit save`, is
//! `until victim`, `until applier` (the default), `until end` (the end of the
//! applier's next turn), `until save` (repeat the save at the end of each of
//! the victim's turns), `for N rounds|minutes|hours`, or `until damaged` - a
//! minute, ended early by any damage its holder takes, which `for N minutes
//! or damaged` writes out in full. `on hit <condition>
//! [lifetime]`, with no `save`, lands one with nothing to resist it - what a
//! weapon mastery like Vex does (`on hit vexed until end`: advantage on that
//! attacker's own next roll against the target).
//!
//! Three more conditions a save or a hit can land: `unhealable` (it cannot
//! regain hit points or gain temporary ones), `reeling` (its speed is 0 and
//! it cannot take reactions) and `addled` (it cannot cast a spell or use a
//! magic item). `until victim end` lasts through the end of the victim's
//! next turn, where `until victim` ends at the start of it. A save aimed at
//! `targets 1` is made by whoever the move was aimed at.
//!
//! A creature that turns its enemies' strength on them: `magic resistance`
//! is advantage on saves against anything tagged `spell` or `item`;
//! `advantage against <condition>` is advantage on its own attack rolls
//! against anything under that condition; `bonus NdM <type> vs <condition>`
//! is extra damage against such a target - as a trait on every hit, as a
//! clause on a move joining that move's damage; and `redirect attacks <ability>
//! dc N` makes the first creature to attack it each turn save or land that
//! attack, and its others that turn, on one of its own side in reach. A
//! reaction `when enemy casts` stops a spell as it is cast - a save in it is
//! the caster's, to keep the spell, and `counter` stops it with no save -
//! and `when targeted by a spell` answers only one aimed at this creature
//! alone. One that costs a slot or a charge is kept for a spell that cost
//! its caster a slot. A stopped spell costs its caster the action, not the
//! slot.
//!
//! What else a failed save or a hit can leave behind: `exhaustion` is a
//! level of it each time it lands - 2 off every attack roll and save and 5
//! feet off its speed a level, and dead at the sixth; `doomed` is
//! disadvantage on death saving throws; `muddled` is 1d6 off every attack
//! roll and off saves to keep concentrating; `sapped` is 1d4 off the next
//! save, which uses it up; `fleeing` spends the victim's reaction, if it has
//! one, running its speed away; and `invisible`, as a `stance` on the user,
//! is disadvantage to attack it and advantage on its own attacks, until it
//! attacks, casts, or loses the concentration holding it. On a grid `pushed`
//! and `pulled` move their victim ten feet away from, or toward, whoever did
//! it. A player character that is down makes death saving throws.
//!
//! More of a creature's own: `reflect <types>` throws damage of those types
//! back at whoever dealt it; `truesight N` sees the invisible within N feet;
//! `refuses healing` is healed by nobody but itself; `first hit NdM <type>`
//! is extra damage on its first hit of the fight; and `spell store <pool> up
//! to N holds M` keeps spell levels in a resource pool, which pay for slots
//! up to level N it has run out of and are fed by the level of any spell it
//! stops with `counter`. A reaction `when damaged` answers damage dealt by
//! any means. On a move, `one at a time` lifts what it lands from whoever
//! else was under it; `escape NdM <type>` lets whoever it holds break out
//! at the start of its turn by taking that, ending the concentration;
//! `on kill raise <name>` raises a humanoid it kills outright as one of the
//! creature's `minions` (stat blocks of their own, under `[[monster.minions]]`
//! in a file), which then takes turns; `teleport N` puts its user up to N
//! feet away from where its enemies are; and `worth N` tells whatever ranks
//! moves by damage what this one is worth besides, in hit points of damage -
//! how a creature is played rather than a rule, and the only way one that
//! deals no damage gets chosen.
//!
//! Creatures that are not quite where they seem: `resist nonmagical <types>`
//! resists those types from an attack that is not magical - a spell attack
//! is, and so is a move tagged `spell` or `item`, a move with the clause
//! `magical`, a swing a boon is riding, and anything from a creature with
//! the trait `magic weapons`; damage that was never an attack roll is not
//! resisted at all. `advantage on initiative` rolls two dice for its place
//! in the order. `no opportunity attacks` as a trait means its movement
//! never provokes one; as a clause on a move, the turn that move is taken
//! its user covers its speed a second time and provokes nothing - "it can
//! move up to its speed without provoking opportunity attacks". `phase
//! through NdM <type>` walks through other creatures, each one it passes
//! through taking that once a turn. `mirror <types> N` takes no damage of
//! those types: every creature within N feet of it takes it instead,
//! whichever side it is on. `swap when hit N <words>` is a reaction: hit by
//! an attack roll, it changes places with one of its own side within N feet
//! whose name has those words in it, and that one is hit instead. On a
//! move, `bonus NdM <type> with advantage` is extra damage on a hit whose
//! attack roll had advantage; `all hit NdM <type> [save <ability> dc N
//! <condition> [lifetime]]` is what every strike of the move landing on one
//! creature adds; and `strikes each` makes one strike at each creature in
//! the move's `range`, the user's own side included. `on fail thrown N`
//! turns a save into one creature's (the one aimed at): failing, it is put
//! down somewhere else within N feet and everything within the `blast R NdM
//! <type>` radius of where it stood takes that - itself too, unless it is
//! `thrown N away`, clear of it - and saving, it takes `on success NdM
//! <type>` and stays. `hide +N` is a Stealth check against the best passive
//! Perception among its user's enemies (`passive perception N`, or 10 and
//! its Wisdom modifier): on a success it is `hidden` - advantage on its
//! attacks and disadvantage to attack it - until its first attack roll, hit
//! or miss. The clause `in dim light` allows a move only while its user
//! stands in dim light or darkness, which is the trait `in dim light` or
//! `in darkness`; a creature that says nothing stands in bright light, and
//! the last light said holds, so a scenario can add `trait: in bright
//! light` under a `source:` line.
//!
//! A storm thrown by hand: on a save aimed at one creature, `chains N`
//! passes it from each creature that fails to the nearest enemy within N
//! feet of it that it has not reached, until one saves. `line N` makes the
//! user the area - the straight line of N feet that crosses the most
//! enemies and the fewest of its own side, everyone on it saving, and the
//! user reappearing at its far end. On an attack, `half on miss` still
//! lands half its damage. On a save around a target (`area N`), `spares
//! target` leaves the target itself out - the burst that follows a blow.
//! `takes bonus action` on an action uses up the bonus action too, for a
//! spell that is a bonus action and the attack it rides. `also cost <pool>
//! N` is a second pool beside `cost`, and a move needs both. `on hit
//! shocked` leaves a target unable to make opportunity attacks. The trait
//! `advantage on <ability> saves [against <types>]` is advantage on that
//! save - against damage of those types, or always. An aura raised with
//! the `lasting_aura` feature can carry `placed N`: it is put down around
//! whoever it is aimed at, up to N feet away, and stays there - reaching
//! `range` feet from that spot, catching what starts a turn inside it
//! rather than what walks in, and pulling toward its middle; with
//! `difficult terrain`, whoever starts a turn in it covers half its speed.
//!
//! Where a creature stands to do it: a `heal` reaches as far as its move's
//! `range` says and a touch - five feet - when it says nothing, so its user
//! walks to whoever it means to heal first; and the trait `fights up close`
//! makes a creature walk up to its target every turn, even to cast from
//! there, where anything else aiming from range keeps its distance.
//!
//! What nobody has to write down. On a grid, a ranged attack is at
//! disadvantage with an enemy that can act within five feet of its maker,
//! and beyond its `range` out to its `long N` if it has one; a prone target
//! is hit with advantage from within five feet and at disadvantage from
//! further off; and a creature standing between a ranged attacker and its
//! target is half cover, +2 to the target's Armor Class. `on fail grappled`
//! leaves a creature's speed 0 until it breaks free. Every creature loaded
//! from a file can also Dodge, Disengage, Help, Shove (a Strength save
//! against 8 + its Strength modifier + its proficiency bonus, or prone) and
//! Grapple (the same save, or grappled until it is made again at the end
//! of a turn) without any of them being in its file.
//!
//! A shell and a gullet: `damage threshold N` ignores any single instance of
//! damage below N, and a weak spot skips it - reached from inside, or by an
//! attack roll while the creature is `exposed` and not `sealed`, or while a
//! breach has left it `cracked`. `on hit swallow <size>` swallows a target
//! that size or smaller; `digest` is what it takes each turn inside, a
//! `swallowed` move hurts everything inside on demand, and `regurgitate` is
//! the damage from inside in one turn that forces a save to keep it down.
//! `points N` is what a legendary action costs. A `reaction:` is a move with a
//! `when enemy <condition>`, `when breached` or `when hit` clause, aimed at
//! whoever set it off - `when hit in melee` and `when hit by a ranged weapon`
//! narrow it to the blows it answers; an `aura:` is a move every enemy is subject to at the start of its
//! turns, aimed at that enemy alone. A creature gets one reaction a round
//! unless `reactions N` says otherwise, and never more than one per turn - so
//! a bigger budget is what lets it answer several different creatures in the
//! same round.
//!
//! A creature with a `mouth` has its enemies stand around it - at its mouth,
//! beside its body, at range or far off in front (see
//! [`crate::creature::Zone`]). Its moves reach only what `reach mouth`,
//! `reach near` or `reach front` names (anywhere, without one), its mouth is
//! its weak spot, open unless `sealed`, and it swims by its `tactic` - `hold`,
//! `charge` or `hit and run`. `difficult terrain` slows its enemies to one
//! place a turn; `pulled` and `pushed` move them a place in or out, and
//! `slowed` halves their moves. An on-hit save can land several conditions:
//! `prone and pushed`. A creature says what it is with `type:` and how big it
//! is with `size:`, which is what a save only one kind of creature is caught
//! by (`only undead`) and a shove gated on size (`push on lightning up to
//! large`) read.
//!
//! On a grid (see [`crate::sim::arena`]) a creature covers `speed:` feet a
//! turn - 30 unless it says - and starts on the square `at: x, y` names, or
//! wherever its side lines up when it names none. A move reaches `range N`
//! feet: 5 for a melee attack and 60 for anything else unless it says, so a
//! longbow is `ranged | range 150` and a ten-foot reach is `range 10`.
//! `area N` is an area's radius around whoever it is aimed at; without it an
//! area catches every enemy in range.
//!
//! Some mechanisms are only reachable through a feature plugin, because they
//! need more than a phrase: a mark maintained by concentration that a rider
//! pays out against (`hunters_mark`), a lasting boon a creature puts on
//! itself (`lasting_boon` - extra damage on its hits, resistances, or both,
//! for a number of rounds), an aura it raises and then holds up
//! (`lasting_aura` - an `aura:` that had to be cast and lasts a stated time,
//! rather than one it simply has), and a double it calls up and then commands
//! (`commanded_double`).
//!
//! So is anything whose numbers have to come from the caster's own profile
//! rather than being written down twice: a reaction answering whoever hit it
//! (`retaliation`), a cast taken at its maximum for a charge
//! (`maximised_damage`), and every spell and class feature with a save DC in
//! it. See [`crate::features`].

use crate::creature::{Creature, Resource, Tactic};
use crate::dsl::grammar::{count, number, parse_move, parse_reaction, parse_trait};
use crate::rules::{Ability, Condition, CreatureType, DamageKind, Reduction, Size};
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parse a whole scenario. Creatures come back in the order they were written.
///
/// A line ending in `|` or `&&` continues onto the next, so a monk's action can
/// be written over several lines without becoming unreadable.
pub fn parse(text: &str) -> Result<Vec<Creature>, ParseError> {
    parse_under(text, Path::new(""))
}

/// [`parse`], with every `source:` line read relative to `root` rather than
/// to wherever the process happens to be standing.
pub fn parse_under(text: &str, root: &Path) -> Result<Vec<Creature>, ParseError> {
    let mut creatures: Vec<Creature> = Vec::new();
    let mut pending_team: Option<u8> = None;

    for (line_no, line) in logical_lines(text) {
        let fail = |message: String| ParseError {
            line: line_no,
            message,
        };

        let Some((key, value)) = line.split_once(':') else {
            return Err(fail(format!("expected `key: value`, got `{line}`")));
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_string();

        if key == "team" && creatures.is_empty()
            || (key == "team"
                && pending_team.is_none()
                && creatures.last().is_some_and(|c| c.hp > 0))
        {
            let team_val = match value.to_ascii_lowercase().as_str() {
                "a" | "0" | "party" => 0,
                "b" | "1" | "monsters" => 1,
                other => return Err(fail(format!("`{other}` is not a team - use a or b"))),
            };
            pending_team = Some(team_val);
            continue;
        }

        if key == "source" || key == "import" || key == "file" {
            let registry = crate::features::FeatureRegistry::new();
            let mut creature =
                crate::dsl::config::load_creature_from_file(root.join(&value), &registry)
                    .map_err(|e| fail(format!("failed to load creature from '{value}': {e}")))?;
            if let Some(team) = pending_team.take() {
                creature.team = team;
            }
            creatures.push(creature);
            continue;
        }

        if key == "creature" {
            if value.is_empty() {
                return Err(fail("a creature needs a name".into()));
            }
            let mut c = Creature::new(value, 0, 0);
            if let Some(team) = pending_team.take() {
                c.team = team;
            }
            creatures.push(c);
            continue;
        }

        let Some(current) = creatures.last_mut() else {
            return Err(fail(format!(
                "`{key}` before any `creature:` or `source:` line"
            )));
        };

        match key.as_str() {
            "ac" => current.ac = number(&value).map_err(fail)?,
            // What it is, for anything that asks: a save only one kind of
            // creature is caught by (turning the Undead, Hold Person's
            // humanoid), bonus damage against a type.
            "type" | "creature type" => {
                current.creature_type = Some(
                    CreatureType::parse(&value)
                        .ok_or_else(|| fail(format!("`{value}` is not a creature type")))?,
                );
            }
            // How big it is, for anything gated on size: what a shove can
            // move, what a mouth can swallow.
            "size" => {
                current.size =
                    Size::parse(&value).ok_or_else(|| fail(format!("`{value}` is not a size")))?;
            }
            "hp" => current.hp = number(&value).map_err(fail)?,
            "initiative" | "init" => current.initiative = number(&value).map_err(fail)?,
            "legendary uses" => current.legendary_uses = count(&value).map_err(fail)?,
            "team" => {
                current.team = match value.to_ascii_lowercase().as_str() {
                    "a" | "0" | "party" => 0,
                    "b" | "1" | "monsters" => 1,
                    other => return Err(fail(format!("`{other}` is not a team - use a or b"))),
                }
            }
            "count" => {
                let n = count(&value).map_err(fail)?;
                if n == 0 {
                    return Err(fail("a count of zero leaves nothing to fight".into()));
                }
                current.count = n;
            }
            "saves" => {
                for entry in value.split(',') {
                    let mut words = entry.split_whitespace();
                    let (Some(name), Some(bonus)) = (words.next(), words.next()) else {
                        return Err(fail(format!("expected `str +3` in saves, got `{entry}`")));
                    };
                    let ability = Ability::parse(name)
                        .ok_or_else(|| fail(format!("unknown ability `{name}`")))?;
                    current.saves[ability.index()] = number(bonus).map_err(fail)?;
                }
            }
            "immune" | "resist" | "vulnerable" => {
                let reduction = match key.as_str() {
                    "immune" => Reduction::Immune,
                    "resist" => Reduction::Resistant,
                    _ => Reduction::Vulnerable,
                };
                for word in value.split(',') {
                    let word = word.trim();
                    let kind = DamageKind::parse(word)
                        .ok_or_else(|| fail(format!("unknown damage type `{word}`")))?;
                    current.reductions.push((kind, reduction));
                }
            }
            "condition immune" => {
                for word in value.split(',') {
                    let word = word.trim();
                    let condition = Condition::parse(word)
                        .ok_or_else(|| fail(format!("unknown condition `{word}`")))?;
                    current.condition_immunities.push(condition);
                }
            }
            "resource" => {
                let mut words = value.split_whitespace();
                let (Some(name), Some(max)) = (words.next(), words.next()) else {
                    return Err(fail(format!("expected `resource: focus 8`, got `{value}`")));
                };
                let max = count(max).map_err(fail)?;
                current.resources.push(Resource {
                    name: name.to_string(),
                    max,
                });
            }
            "trait" => {
                let effect = parse_trait(&value).map_err(fail)?;
                effect.apply(current).map_err(fail)?;
            }
            "action" | "bonus" | "legendary" | "aura" => {
                let m = parse_move(&value, current).map_err(fail)?;
                match key.as_str() {
                    "action" => current.actions.push(m),
                    "bonus" => current.bonus_actions.push(m),
                    "aura" => current.auras.push(m),
                    _ => current.legendary.push(m),
                }
            }
            "reaction" => {
                let r = parse_reaction(&value, current).map_err(fail)?;
                current.reactions.push(r);
            }
            // Feet a turn on the grid, and the square it starts on.
            "speed" => current.speed = count(&value).map_err(fail)?,
            "at" => {
                let cell: Vec<&str> = value.split(',').map(str::trim).collect();
                let [x, y] = cell[..] else {
                    return Err(fail(format!("expected `at: 4, 7`, got `{value}`")));
                };
                current.at = Some((number(x).map_err(fail)?, number(y).map_err(fail)?));
            }
            "tactic" => {
                current.tactic = Tactic::parse(&value).ok_or_else(|| {
                    fail(format!(
                        "`{value}` is not a tactic - use hold, charge or hit and run"
                    ))
                })?;
            }
            other => return Err(fail(format!("unknown key `{other}`"))),
        }
    }

    for (i, c) in creatures.iter().enumerate() {
        if c.hp <= 0 {
            return Err(ParseError {
                line: 0,
                message: format!("creature {} (`{}`) needs a positive `hp`", i + 1, c.name),
            });
        }
    }

    // A file that never mentions teams reads as the first creature against
    // everything else, which is what every one-against-one scenario meant before
    // teams existed.
    if creatures.iter().all(|c| c.team == 0) && creatures.len() > 1 {
        for c in creatures.iter_mut().skip(1) {
            c.team = 1;
        }
    }
    Ok(expand(creatures))
}

/// Turn `count: 4` into four separately numbered creatures.
///
/// Numbered because the narration is unreadable otherwise, and because
/// [`crate::sim::Policy::Scattered`] needs them to be distinguishable.
fn expand(creatures: Vec<Creature>) -> Vec<Creature> {
    let mut out = Vec::new();
    for c in creatures {
        let n = c.count.max(1);
        for i in 0..n {
            let mut copy = c.clone();
            copy.count = 1;
            if n > 1 {
                copy.name = format!("{} {}", c.name, i + 1);
            }
            out.push(copy);
        }
    }
    out
}

/// Strip comments, drop blanks, and glue continuation lines together. The line
/// number reported is the one the logical line started on.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        // Either end of the join may carry the marker: a line may trail off
        // with `|`, or the next may pick up with `&&`. Both read naturally, so
        // both work.
        let picks_up = line.starts_with("&&") || line.starts_with('|');
        let trails_off = out
            .last()
            .is_some_and(|(_, acc)| acc.ends_with('|') || acc.ends_with("&&"));
        match out.last_mut() {
            Some((_, acc)) if picks_up || trails_off => {
                acc.push(' ');
                acc.push_str(line);
            }
            _ => out.push((n + 1, line.to_string())),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::Effect;
    use crate::rules::DamageRoll;

    /// What a creature is and how big it is, both read off a line of their
    /// own: the two things a type-restricted save and a size-gated shove ask
    /// about.
    #[test]
    fn a_creature_can_say_what_it_is_and_how_big() {
        let text = "
creature: Skeleton
ac: 14
hp: 13
type: undead
size: medium
";
        let c = &parse(text).expect("parses")[0];
        assert!(c.is_creature_type("Undead"));
        assert!(!c.is_creature_type("Humanoid"));
        assert_eq!(c.size, Size::Medium);

        // Unsaid, a creature is Medium and matches any type asked about -
        // see `Creature::is_creature_type`, which keeps a restriction off
        // creatures nobody labelled rather than excluding them.
        let plain = &parse(
            "creature: x
ac: 1
hp: 1
",
        )
        .expect("parses")[0];
        assert_eq!(plain.creature_type, None);
        assert_eq!(plain.size, Size::Medium);

        assert!(parse(
            "creature: x
hp: 1
type: eldritch
"
        )
        .is_err());
        assert!(parse(
            "creature: x
hp: 1
size: colossal
"
        )
        .is_err());
    }

    #[test]
    fn a_whole_creature_round_trips_into_the_right_numbers() {
        let text = "
# a comment, and a blank line above
creature: Ogre
ac: 11
hp: 68
initiative: -1
saves: str +4, dex -1, con +3
action: Greatclub | strikes 1 | hit +6 | 2d8+4 bludgeoning
";
        let c = &parse(text).expect("parses")[0];
        assert_eq!(c.name, "Ogre");
        assert_eq!((c.ac, c.hp, c.initiative), (11, 68, -1));
        assert_eq!(c.save(Ability::Str), 4);
        assert_eq!(c.save(Ability::Cha), 0, "unset saves stay at zero");
        match &c.actions[0].effect {
            Effect::Strikes { strike, count } => {
                assert_eq!(*count, 1);
                assert_eq!(strike.to_hit, 6);
                assert_eq!(
                    strike.damage,
                    vec![DamageRoll::new(2, 8, 4, DamageKind::Bludgeoning)]
                );
            }
            other => panic!("expected strikes, got {other:?}"),
        }
    }

    /// Errors carry a line number, because a scenario file is hand-written and
    /// "parse error" with no location is the least useful message there is.
    #[test]
    fn mistakes_are_reported_where_they_happen() {
        let missing_creature = parse("ac: 10\n").unwrap_err();
        assert_eq!(missing_creature.line, 1);

        let bad_type =
            parse("creature: x\nhp: 4\naction: Hit | hit +2 | 1d6 sparkly\n").unwrap_err();
        assert_eq!(bad_type.line, 3);
        assert!(bad_type.message.contains("sparkly"), "{bad_type}");

        let no_hit = parse("creature: x\nhp: 4\naction: Hit | 1d6 fire\n").unwrap_err();
        assert!(no_hit.message.contains("hit +N"), "{no_hit}");

        let no_hp = parse("creature: x\nac: 4\n").unwrap_err();
        assert!(no_hp.message.contains("positive `hp`"), "{no_hp}");

        // Spending from a pool nobody declared is the easy mistake to make.
        let no_pool = parse("creature: x\nhp: 4\nbonus: Flurry | cost focus 1 | stance dodging\n")
            .unwrap_err();
        assert!(
            no_pool.message.contains("not a declared resource"),
            "{no_pool}"
        );

        let empty_move = parse("creature: x\nhp: 4\naction: Ponder | uses 2\n").unwrap_err();
        assert!(empty_move.message.contains("does nothing"), "{empty_move}");
    }

    #[test]
    fn scenarios_can_reference_external_config_sources() {
        let text = "
team: a
source: content/characters/cleric.toml

team: b
source: content/monsters/brute.toml
";
        let combatants = parse(text).expect("parses external config sources");
        assert_eq!(combatants.len(), 2);
        assert_eq!(combatants[0].name, "Cleric");
        assert_eq!(combatants[0].team, 0);
        assert_eq!(combatants[1].name, "Brute");
        assert_eq!(combatants[1].team, 1);
    }
}
