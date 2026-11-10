import type { Replay } from './types';

/** How many feet a square of the grid is across. */
export const FEET = 5;

/**
 * Where a unit is drawn `t` of the way through a frame: walking its path if
 * it is the one moving, standing where the frame leaves it otherwise.
 */
export function where(replay: Replay, frame: number, seat: number, t: number): [number, number] {
	const beat = replay.frames[frame];
	const now = beat.units[seat];
	const before = replay.frames[frame - 1]?.units[seat];
	if (beat.kind !== 'move' || beat.actor !== seat || !before || beat.path.length === 0) {
		return [now.x, now.y];
	}
	const stops: [number, number][] = [[before.x, before.y], ...beat.path];
	const along = t * (stops.length - 1);
	const i = Math.min(Math.floor(along), stops.length - 2);
	const f = along - i;
	return [
		stops[i][0] + (stops[i + 1][0] - stops[i][0]) * f,
		stops[i][1] + (stops[i + 1][1] - stops[i][1]) * f
	];
}
