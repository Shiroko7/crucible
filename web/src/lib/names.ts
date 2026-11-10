import type { Member } from './types';

/**
 * Two characters to stand for a creature: its initial, and the number its
 * name ends in or the initial of its second word or its second letter.
 */
export function initials(name: string): string {
	const words = name.split(/[^a-z0-9]+/i).filter(Boolean);
	const first = (words[0]?.[0] ?? '?').toUpperCase();
	const last = words.at(-1) ?? '';
	if (words.length > 1 && /^\d+$/.test(last)) return first + last.slice(-1);
	if (words.length > 1) return first + last[0].toUpperCase();
	return first + (words[0]?.[1] ?? '').toLowerCase();
}

/**
 * A colour for a creature: every party member a blue of its own, everything
 * it faces a red of its own, so two creatures on a side can be told apart.
 */
export function hue(name: string, team: number): number {
	// Copies of one creature share a colour: "Ogre 3" is an ogre.
	const stem = name.replace(/\s*\d+$/, '');
	let hash = 0;
	for (const c of stem) hash = (hash * 31 + c.charCodeAt(0)) >>> 0;
	return team === 0 ? 204 + (hash % 22) : (356 + (hash % 20)) % 360;
}

/** A token's paint: a dark face lit from above, inside a ring of its side's metal. */
export function colours(name: string, team: number): { light: string; dark: string; ring: string } {
	const h = hue(name, team);
	return {
		light: `hsl(${h} 30% 33%)`,
		dark: `hsl(${h} 38% 12%)`,
		ring: `hsl(${h} 46% 58%)`
	};
}

function plural(name: string): string {
	if (/(s|x|ch|sh)$/i.test(name)) return `${name}es`;
	return `${name}s`;
}

/** A side in words: "Knight & Cleric", "5 Brutes". */
export function sideName(members: Member[]): string {
	const parts = members.map((m) => (m.count > 1 ? `${m.count} ${plural(m.name)}` : m.name));
	if (parts.length === 0) return 'nobody';
	if (parts.length === 1) return parts[0];
	return `${parts.slice(0, -1).join(', ')} & ${parts.at(-1)}`;
}

/** Kept on this machine only: anything in a `local` folder is out of git. */
export function isPrivate(path: string): boolean {
	return path.split('/').slice(0, -1).includes('local');
}

/**
 * The roster by what a creature is - characters, then monsters, then whatever
 * does not load - rather than by where its file happens to be.
 */
export function byKind<T extends { path: string; card: { kind: string; name: string } | null }>(
	creatures: T[]
): { label: string; creatures: T[] }[] {
	const of = (label: string, kind: string | null) => ({
		label,
		creatures: creatures
			.filter((c) => (c.card?.kind ?? null) === kind)
			.sort((a, b) => (a.card?.name ?? a.path).localeCompare(b.card?.name ?? b.path))
	});
	return [
		of('Characters', 'character'),
		of('Monsters', 'monster'),
		of('Does not load', null)
	].filter((shelf) => shelf.creatures.length > 0);
}

/** A name as a file is called: `The Bridge!` is `the-bridge`. */
export function slug(name: string): string {
	return name
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '');
}

export function capitalised(text: string): string {
	return text.charAt(0).toUpperCase() + text.slice(1);
}

export function percent(share: number): string {
	const value = share * 100;
	if (value > 0 && value < 1) return '<1%';
	if (value < 100 && value > 99) return '>99%';
	return `${Math.round(value)}%`;
}
