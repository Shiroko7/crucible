// The shapes crucible-server sends, mirroring `crucible_core::sim::showcase`,
// `crucible_core::sim::Replay` and the server's own library listing.

export type FrameKind =
	| 'start'
	| 'initiative'
	| 'turn'
	| 'move'
	| 'use'
	| 'attack'
	| 'damage'
	| 'save'
	| 'heal'
	| 'condition'
	| 'down'
	| 'lost'
	| 'recover'
	| 'other'
	| 'end';

/** How a move is drawn: a weapon swung, a spell cast, an item used. */
export type School = 'weapon' | 'spell' | 'item' | 'ability';

/** One die as it fell. */
export interface DieRoll {
	sides: number;
	value: number;
	/** Not rolled: a die of a pool taken at its maximum, on its highest face. */
	maxed?: boolean;
}

/** The numbers behind a beat, for the kinds of beat that have them. */
export type Detail =
	| { type: 'initiative'; die: number; modifier: number; total: number }
	| { type: 'use'; name: string; school: School; slot: string; ranged: boolean }
	| {
			type: 'attack';
			name: string;
			school: School;
			ranged: boolean;
			/** The d20s rolled: two under advantage or disadvantage. */
			dice: number[];
			kept: number;
			modifier: number;
			total: number;
			ac: number;
			outcome: 'miss' | 'hit' | 'crit';
	  }
	| {
			type: 'damage';
			amount: number;
			kinds: string[];
			dice: DieRoll[];
			note: string;
			school: School;
	  }
	| {
			type: 'save';
			ability: string;
			dice: number[];
			kept: number;
			modifier: number;
			total: number;
			dc: number;
			passed: boolean;
			/** `rolled`, `legendary resistance` or `automatic failure`. */
			how: string;
	  }
	| { type: 'heal'; amount: number; school: School }
	| { type: 'condition'; name: string };

/** One creature as a frame leaves it. */
export interface UnitState {
	hp: number;
	temp_hp: number;
	/** Its Armor Class as it stands, with whatever is raising it. */
	ac: number;
	x: number;
	y: number;
	alive: boolean;
	/** Off the grid for now: swallowed. */
	hidden: boolean;
	conditions: string[];
	/** The spell it is concentrating on, if it is holding one. */
	concentrating?: string | null;
}

export interface Frame {
	round: number;
	kind: FrameKind;
	/** Roster seats: who did it, and to whom. */
	actor: number | null;
	target: number | null;
	text: string;
	/** The squares the actor walked through, for a `move`. */
	path: [number, number][];
	/** The roll or the amount behind it, when it has one. */
	detail: Detail | null;
	units: UnitState[];
}

/** What does not change about a creature over a fight. */
export interface Unit {
	name: string;
	team: number;
	/** Squares along each side of its space. */
	span: number;
	max_hp: number;
	/** Its Armor Class with nothing raising it. */
	ac: number;
	summon: boolean;
}

export interface Replay {
	/** The grid in squares, or 0 by 0 for a fight without one. */
	width: number;
	height: number;
	units: Unit[];
	/** Roster seats in initiative order. */
	order: number[];
	frames: Frame[];
	lines: string[];
	/** 0 or 1, or null for a fight nobody won. */
	winner: number | null;
	rounds: number;
}

export interface Entry {
	policy: string;
	/** `worst`, `median` or `best`, for the first side. */
	pick: string;
	seed: number;
	stream: number;
	replay: Replay;
}

/** How the first side did under one playstyle, over the probe run. */
export interface Style {
	policy: string;
	blurb: string;
	win: number;
	loss: number;
	draw: number;
	rounds: number;
	deaths: number;
}

export interface Bundle {
	title: string;
	probed: number;
	/** One per playstyle, in the order their fights appear in `entries`. */
	styles: Style[];
	entries: Entry[];
}

/** One kind of creature on a side, and how many of it. */
export interface Member {
	name: string;
	count: number;
	span: number;
	hp: number;
}

export interface ScenarioInfo {
	path: string;
	name: string;
	group: string;
	/** Who is on each side: the party, then what it faces. */
	sides: [Member[], Member[]];
	/** Why the file does not parse, when it does not. */
	error: string | null;
}

export interface RunOptions {
	seed: number;
	samples: number;
	apart: number;
	/** The map in squares, across and down: never less than 40 by 30. */
	map: [number, number];
}

/** One thing a creature can do, and what it takes to do it. */
export interface Listed {
	slot: 'action' | 'bonus' | 'reaction' | 'legendary' | 'aura';
	name: string;
}

/** A creature as its file loads. */
export interface Card {
	name: string;
	kind: 'character' | 'monster';
	ac: number;
	hp: number;
	size: string;
	span: number;
	creature_type: string | null;
	initiative: number;
	speed: number;
	/** Strength through Charisma. */
	saves: number[];
	resources: { name: string; max: number }[];
	moves: Listed[];
}

/** A creature file in the roster. */
export interface CreatureInfo {
	/** What a scenario names it by. */
	path: string;
	group: string;
	card: Card | null;
	/** Why the file does not load, when it does not. */
	error: string | null;
}

/** A creature file opened for editing. */
export interface CreatureSheet {
	path: string;
	text: string;
	card: Card | null;
	error: string | null;
	/** The scenarios that field it. */
	used_by: string[];
}

/** One creature file on a side of a scenario, and how many copies. */
export interface Fielded {
	team: number;
	source: string;
	count: number;
}

/** A scenario opened for editing. */
export interface ScenarioSheet {
	path: string;
	text: string;
	notes: string;
	/** Null for a file written by hand, which is edited as text. */
	entries: Fielded[] | null;
}

/** A creature file as a sheet: the one table it holds, and its opening note. */
export interface CreatureForm {
	kind: 'character' | 'monster';
	notes: string;
	// Whatever the file says: the sheet has fields for most of it.
	// eslint-disable-next-line @typescript-eslint/no-explicit-any
	table: Record<string, any>;
}

/** What a draft of a creature file loads as. */
export interface Checked {
	card: Card | null;
	error: string | null;
	/** Null for text that is not one creature's table at all. */
	form: CreatureForm | null;
	/** Comments inside the file that saving from the sheet would drop. */
	comments: number;
}

/** A sheet written out as its file. */
export interface Rendered {
	text: string;
	card: Card | null;
	error: string | null;
}

/** A creature file drafted by the model. */
export interface Drafted {
	text: string;
	card: Card | null;
	error: string | null;
	/** What the model says it left out or guessed. */
	notes: string[];
	attempts: number;
}
