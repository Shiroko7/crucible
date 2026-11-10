// What a creature is under - stunned, poisoned, concentrating - as something
// to look at: a colour, a small icon for its token, and words for its plate.

type Icon =
	| 'star'
	| 'drop'
	| 'down'
	| 'up'
	| 'bolt'
	| 'heart'
	| 'bang'
	| 'shield'
	| 'hourglass'
	| 'links'
	| 'sight'
	| 'eye'
	| 'mute'
	| 'plus'
	| 'block'
	| 'ring'
	| 'sleep';

export interface Status {
	label: string;
	colour: string;
	icon: Icon;
}

const KNOWN: Record<string, Status> = {
	stunned: { label: 'stunned', colour: '#ffd84d', icon: 'star' },
	poisoned: { label: 'poisoned', colour: '#7fd65a', icon: 'drop' },
	prone: { label: 'prone', colour: '#d6b48a', icon: 'down' },
	blinded: { label: 'blinded', colour: '#a9b2c1', icon: 'eye' },
	paralyzed: { label: 'paralyzed', colour: '#8fd4ff', icon: 'bolt' },
	incapacitated: { label: 'incapacitated', colour: '#a9b2c1', icon: 'sleep' },
	deafened: { label: 'deafened', colour: '#a9b2c1', icon: 'mute' },
	silenced: { label: 'silenced', colour: '#a9b2c1', icon: 'mute' },
	compelled: { label: 'compelled', colour: '#ff8fcf', icon: 'ring' },
	charmed: { label: 'charmed', colour: '#ff8fcf', icon: 'heart' },
	frightened: { label: 'frightened', colour: '#ff9a4d', icon: 'bang' },
	petrified: { label: 'petrified', colour: '#9aa3a8', icon: 'block' },
	exhaustion: { label: 'exhausted', colour: '#b89a7a', icon: 'down' },
	slowed: { label: 'slowed', colour: '#7fa8ff', icon: 'hourglass' },
	restrained: { label: 'restrained', colour: '#c3ccd9', icon: 'links' },
	banished: { label: 'banished', colour: '#b9a2ff', icon: 'ring' },
	suppressed: { label: 'suppressed', colour: '#b08be0', icon: 'down' },
	dodging: { label: 'dodging', colour: '#6fe0e0', icon: 'shield' },
	steady_aim: { label: 'steady aim', colour: '#6fe0e0', icon: 'sight' },
	marked: { label: 'marked', colour: '#ff7a6b', icon: 'sight' },
	quarry: { label: 'quarry', colour: '#ff7a6b', icon: 'sight' },
	outlined: { label: 'outlined', colour: '#b9a2ff', icon: 'sight' },
	vexed: { label: 'vexed', colour: '#ffd84d', icon: 'up' },
	boon: { label: 'boon', colour: '#ffd84d', icon: 'plus' },
	exposed: { label: 'mouth open', colour: '#ff9a4d', icon: 'bang' },
	sealed: { label: 'mouth sealed', colour: '#c3ccd9', icon: 'block' },
	cracked: { label: 'shell cracked', colour: '#ff9a4d', icon: 'bolt' },
	swallowed: { label: 'swallowed', colour: '#b08be0', icon: 'ring' },
	unhealable: { label: 'cannot be healed', colour: '#c2564a', icon: 'heart' },
	reeling: { label: 'reeling', colour: '#ff9a4d', icon: 'hourglass' },
	addled: { label: 'addled', colour: '#b08be0', icon: 'mute' },
	doomed: { label: 'doomed', colour: '#c2564a', icon: 'down' },
	invisible: { label: 'invisible', colour: '#a9b2c1', icon: 'eye' },
	muddled: { label: 'muddled', colour: '#b08be0', icon: 'down' },
	sapped: { label: 'sapped', colour: '#b08be0', icon: 'drop' }
};

/** Held for an instant and gone, or shown some other way: no badge. */
const SILENT = new Set(['pulled', 'pushed', 'aura']);

export const CONCENTRATING: Status = {
	label: 'concentrating',
	colour: '#b9a2ff',
	icon: 'ring'
};

/** How a condition is shown, or `null` for one that is not. */
export function status(name: string): Status | null {
	if (SILENT.has(name)) return null;
	const saves = name.match(/^disadvantage_(\w+)_saves$/);
	if (saves) {
		return {
			label: `disadvantage on ${saves[1]} saves`,
			colour: '#b08be0',
			icon: 'down'
		};
	}
	return (
		KNOWN[name] ?? {
			label: name.replaceAll('_', ' '),
			colour: '#c3ccd9',
			icon: 'ring'
		}
	);
}

/** A creature's conditions as badges, one of each. */
export function statuses(conditions: string[]): Status[] {
	const seen = new Set<string>();
	const out: Status[] = [];
	for (const name of conditions) {
		const s = status(name);
		if (s && !seen.has(s.label)) {
			seen.add(s.label);
			out.push(s);
		}
	}
	return out;
}

/** Draw a status as a square badge, `r` from its centre at (`x`, `y`) to each side. */
export function badge(g: CanvasRenderingContext2D, s: Status, x: number, y: number, r: number) {
	g.save();
	g.translate(x, y);
	// A square of dark glass edged in its colour, as a game marks an effect.
	g.fillStyle = '#000';
	g.fillRect(-r - 1, -r - 1, r * 2 + 2, r * 2 + 2);
	g.fillStyle = '#14110e';
	g.fillRect(-r, -r, r * 2, r * 2);
	g.strokeStyle = s.colour;
	g.lineWidth = 1;
	g.strokeRect(-r + 0.5, -r + 0.5, r * 2 - 1, r * 2 - 1);

	// The icon, in a box from -1 to 1.
	const k = r * 0.58;
	g.scale(k, k);
	g.lineWidth = 0.34;
	g.lineCap = 'round';
	g.lineJoin = 'round';
	g.fillStyle = s.colour;
	g.beginPath();
	switch (s.icon) {
		case 'star':
			for (let i = 0; i < 10; i++) {
				const a = -Math.PI / 2 + (i * Math.PI) / 5;
				const d = i % 2 ? 0.45 : 1;
				g.lineTo(Math.cos(a) * d, Math.sin(a) * d);
			}
			g.closePath();
			g.fill();
			break;
		case 'drop':
			g.moveTo(0, -1);
			g.bezierCurveTo(0.9, 0, 0.8, 1, 0, 1);
			g.bezierCurveTo(-0.8, 1, -0.9, 0, 0, -1);
			g.fill();
			break;
		case 'down':
			g.moveTo(-0.8, -0.5);
			g.lineTo(0, 0.6);
			g.lineTo(0.8, -0.5);
			g.stroke();
			break;
		case 'up':
			g.moveTo(-0.8, 0.5);
			g.lineTo(0, -0.6);
			g.lineTo(0.8, 0.5);
			g.stroke();
			break;
		case 'bolt':
			g.moveTo(0.3, -1);
			g.lineTo(-0.6, 0.15);
			g.lineTo(0, 0.15);
			g.lineTo(-0.3, 1);
			g.lineTo(0.6, -0.2);
			g.lineTo(0, -0.2);
			g.closePath();
			g.fill();
			break;
		case 'heart':
			g.moveTo(0, 0.9);
			g.bezierCurveTo(-1.4, -0.1, -0.7, -1.1, 0, -0.35);
			g.bezierCurveTo(0.7, -1.1, 1.4, -0.1, 0, 0.9);
			g.fill();
			break;
		case 'bang':
			g.moveTo(0, -0.9);
			g.lineTo(0, 0.2);
			g.stroke();
			g.beginPath();
			g.arc(0, 0.8, 0.2, 0, Math.PI * 2);
			g.fill();
			break;
		case 'shield':
			g.moveTo(-0.8, -0.8);
			g.lineTo(0.8, -0.8);
			g.lineTo(0.8, 0);
			g.quadraticCurveTo(0.7, 0.8, 0, 1);
			g.quadraticCurveTo(-0.7, 0.8, -0.8, 0);
			g.closePath();
			g.fill();
			break;
		case 'hourglass':
			g.moveTo(-0.7, -0.9);
			g.lineTo(0.7, -0.9);
			g.lineTo(-0.7, 0.9);
			g.lineTo(0.7, 0.9);
			g.closePath();
			g.fill();
			break;
		case 'links':
			g.arc(-0.4, 0, 0.5, 0, Math.PI * 2);
			g.moveTo(0.9, 0);
			g.arc(0.4, 0, 0.5, 0, Math.PI * 2);
			g.stroke();
			break;
		case 'sight':
			g.arc(0, 0, 0.6, 0, Math.PI * 2);
			g.moveTo(0, -1);
			g.lineTo(0, 1);
			g.moveTo(-1, 0);
			g.lineTo(1, 0);
			g.stroke();
			break;
		case 'eye':
			g.moveTo(-1, 0);
			g.quadraticCurveTo(0, -0.9, 1, 0);
			g.quadraticCurveTo(0, 0.9, -1, 0);
			g.moveTo(-0.8, 0.8);
			g.lineTo(0.8, -0.8);
			g.stroke();
			break;
		case 'mute':
			g.arc(0, 0, 0.8, 0, Math.PI * 2);
			g.moveTo(-0.55, 0.55);
			g.lineTo(0.55, -0.55);
			g.stroke();
			break;
		case 'plus':
			g.moveTo(0, -0.8);
			g.lineTo(0, 0.8);
			g.moveTo(-0.8, 0);
			g.lineTo(0.8, 0);
			g.stroke();
			break;
		case 'block':
			g.rect(-0.7, -0.7, 1.4, 1.4);
			g.fill();
			break;
		case 'sleep':
			g.moveTo(-0.6, -0.6);
			g.lineTo(0.6, -0.6);
			g.lineTo(-0.6, 0.6);
			g.lineTo(0.6, 0.6);
			g.stroke();
			break;
		case 'ring':
			g.arc(0, 0, 0.7, 0, Math.PI * 2);
			g.stroke();
			g.beginPath();
			g.arc(0, 0, 0.2, 0, Math.PI * 2);
			g.fill();
			break;
	}
	g.restore();
}
