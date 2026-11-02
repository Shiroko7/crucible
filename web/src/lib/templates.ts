// What a new creature starts as: a small one that loads, to be written over.

export const CHARACTER = `[pc]
name = "New Character"
class = "Fighter"
level = 5
ac = 18
hp = 44
initiative = 1
speed = 30

[pc.saves]
str = 7
dex = 1
con = 5
int = 0
wis = 1
cha = 0

[[pc.actions]]
name = "Longsword"
effect = "strikes 2 | hit +7 | 1d8+4 slashing"
`;

export const MONSTER = `[monster]
name = "New Monster"
size = "Medium"
creature_type = "Humanoid"
ac = 13
hp = 30
initiative = 1
speed = 30

[monster.saves]
str = 1
dex = 1
con = 1
int = 0
wis = 0
cha = 0

[[monster.actions]]
name = "Scimitar"
effect = "strikes 2 | hit +4 | 1d6+2 slashing"
`;

/** The phrases the effect of a move is written in, by example. */
export const PHRASES: { say: string; means: string }[] = [
	{ say: 'strikes 2 | hit +7 | 1d8+4 slashing', means: 'Two melee attacks at +7.' },
	{ say: 'ranged | hit +6 | 1d8+3 piercing', means: 'An attack from range.' },
	{
		say: 'save dex dc 15 | 6d6 fire | half on success',
		means: 'A save, for half damage on a success.'
	},
	{
		say: 'spell | slot 2 | concentration | save wis dc 15 | on fail paralyzed until save',
		means: 'A spell that spends a slot and lands a condition.'
	},
	{ say: 'recharge 5 | save con dc 17 | 8d8 cold', means: 'Back on a 5 or 6.' },
	{ say: 'uses 3 | heal 2d4+2', means: 'Three times a fight.' },
	{ say: 'cost focus 1 | stance dodging', means: 'Paid from a pool under [*.resources].' },
	{
		say: 'strikes 1 | hit +9 | 2d6+5 slashing && strikes 2 | hit +9 | 1d8+5 piercing',
		means: 'A multiattack of different attacks.'
	}
];
