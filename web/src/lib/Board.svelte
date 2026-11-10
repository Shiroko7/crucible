<script lang="ts">
	// The battlefield at one beat of a recorded fight. Each kind of beat is
	// drawn as what it is: a spell is cast in a circle of light and flies as a
	// bolt, a blade lunges and cuts, a potion bubbles; a miss is called out,
	// damage bursts in the colour of its type, healing rises green.
	import { FEET, where } from './board';
	import { colours, initials } from './names';
	import { CONCENTRATING, badge, statuses } from './status';
	import type { Frame, Replay } from './types';

	let {
		replay,
		frame,
		hold,
		still = null,
		pad = [0, 0, 0, 0],
		whole = false
	}: {
		replay: Replay;
		frame: number;
		/** Milliseconds a frame is held while playing: what the animation fits inside. */
		hold: number;
		/** Hold every beat's animation this far through, 0 to 1: for a picture of one. */
		still?: number | null;
		/** How much of each edge the interface covers: top, right, bottom, left. */
		pad?: [number, number, number, number];
		/** Show the whole map rather than following the fight across it. */
		whole?: boolean;
	} = $props();

	let canvas = $state<HTMLCanvasElement>();
	const paints = $derived(replay.units.map((u) => colours(u.name, u.team)));
	// When the frame on show came up, which is what its animation runs from.
	let since = 0;
	// What the map is looking at: the square at the middle of the view, and
	// how big a square is drawn. It follows the fight rather than jumping.
	let camera: { x: number; y: number; cell: number } | null = null;
	let drawn = 0;

	$effect(() => {
		void replay;
		camera = null;
	});

	/** The part of the grid the fight is on at one beat: left, top, right, bottom. */
	function action(beat: Frame, before: Frame | undefined): [number, number, number, number] {
		let [l, t, r, b] = [Infinity, Infinity, -Infinity, -Infinity];
		const take = (x: number, y: number, size: number) => {
			l = Math.min(l, x);
			t = Math.min(t, y);
			r = Math.max(r, x + size);
			b = Math.max(b, y + size);
		};
		beat.units.forEach((state, seat) => {
			const size = replay.units[seat].span;
			const part = seat === beat.actor || seat === beat.target;
			if (state.hidden || !(state.alive || part)) return;
			take(state.x, state.y, size);
			// Where it came from, so a walk is seen from its first step.
			const was = before?.units[seat];
			if (was && part) take(was.x, was.y, size);
		});
		if (l > r) return [0, 0, replay.width, replay.height];
		// Room around it for nameplates and for what flies off the tokens.
		return [l - 1.5, t - 2, r + 1.5, b + 1.5];
	}

	$effect(() => {
		void frame;
		void replay;
		since = performance.now();
	});

	$effect(() => {
		let id = requestAnimationFrame(function tick() {
			draw();
			id = requestAnimationFrame(tick);
		});
		return () => cancelAnimationFrame(id);
	});

	/** What each type of damage looks like. */
	const KINDS: Record<string, string> = {
		fire: '#ff8a3d',
		cold: '#8fd4ff',
		lightning: '#ffe45c',
		thunder: '#9db4ff',
		radiant: '#ffe9a8',
		necrotic: '#b08be0',
		poison: '#8fd66a',
		acid: '#c8e64a',
		psychic: '#ff8fcf',
		force: '#e68fff'
	};
	const STEEL = '#e6e2d8';
	const ARCANE = '#b9a2ff';
	const GOLD = '#c9a45c';
	const BONE = '#efe6d2';
	/** Temporary hit points, and anything else held up for now. */
	const TEMP = '#7fd0e8';
	const SERIF = "'Palatino Linotype', Palatino, 'Book Antiqua', Georgia, serif";
	const SANS = "'Segoe UI', system-ui, sans-serif";

	/** The colour of the damage a beat is about - or is about to do. */
	function tint(beat: Frame, next: Frame | undefined): string {
		const from = beat.detail?.type === 'damage' ? beat : next;
		const kinds = from?.detail?.type === 'damage' ? from.detail.kinds : [];
		const named = kinds.find((k) => KINDS[k]);
		if (named) return KINDS[named];
		const school = beat.detail && 'school' in beat.detail ? beat.detail.school : 'weapon';
		return school === 'spell' ? ARCANE : STEEL;
	}

	const smooth = (p: number) => p * p * (3 - 2 * p);
	/** 0 before `from`, 1 after `to`, and how far between them otherwise. */
	const span = (p: number, from: number, to: number) =>
		Math.max(0, Math.min(1, (p - from) / (to - from)));

	function draw() {
		if (!canvas) return;
		const box = canvas.getBoundingClientRect();
		const scale = devicePixelRatio || 1;
		const [w, h] = [Math.round(box.width * scale), Math.round(box.height * scale)];
		if (canvas.width !== w || canvas.height !== h) {
			canvas.width = w;
			canvas.height = h;
		}
		const g = canvas.getContext('2d');
		if (!g) return;
		g.setTransform(scale, 0, 0, scale, 0, 0);
		g.clearRect(0, 0, box.width, box.height);
		g.textAlign = 'center';
		g.textBaseline = 'middle';
		g.lineCap = 'round';

		const beat = replay.frames[frame];
		if (!beat) return;
		const before = replay.frames[frame - 1];
		const next = replay.frames[frame + 1];
		if (!replay.width) {
			g.fillStyle = '#978e7c';
			g.font = `italic 16px ${SERIF}`;
			g.fillText('This fight was recorded without a grid.', box.width / 2, box.height / 2);
			return;
		}

		// The map is the grid the fight was fought on, square for square.
		const [cols, rows] = [replay.width, replay.height];
		// The fight is fitted into what the interface leaves clear.
		const [top, right, bottom, left] = pad;
		const clearW = Math.max(120, box.width - left - right - 24);
		const clearH = Math.max(120, box.height - top - bottom - 24);
		const [al, at, ar, ab] = whole ? [-1, -1.5, cols + 0.5, rows + 0.5] : action(beat, before);
		const aim = {
			x: (al + ar) / 2,
			y: (at + ab) / 2,
			cell: Math.max(8, Math.min(clearW / (ar - al), clearH / (ab - at), 72))
		};
		const tock = performance.now();
		if (!camera || still != null) camera = aim;
		else {
			const k = 1 - Math.exp(-(tock - drawn) / 260);
			camera = {
				x: camera.x + (aim.x - camera.x) * k,
				y: camera.y + (aim.y - camera.y) * k,
				cell: camera.cell + (aim.cell - camera.cell) * k
			};
		}
		drawn = tock;
		const cell = camera.cell;
		const ox = left + 12 + clearW / 2 - camera.x * cell;
		const oy = top + 12 + clearH / 2 - camera.y * cell;
		const now = performance.now();
		const p = still ?? Math.min(1, (now - since) / Math.min(hold * 0.85, 650));
		const ease = smooth(p);
		const detail = beat.detail;
		const colour = tint(beat, next);

		// The map: flagstones five feet across, each a little its own, ruled
		// off from one another and ending where the grid does.
		g.fillStyle = '#0a0908';
		g.fillRect(0, 0, box.width, box.height);
		const seen = (to: number, origin: number, room: number) => [
			Math.max(0, Math.floor(-origin / cell)),
			Math.min(to, Math.ceil((room - origin) / cell))
		];
		const [x0, x1] = seen(cols, ox, box.width);
		const [y0, y1] = seen(rows, oy, box.height);
		for (let y = y0; y < y1; y++) {
			for (let x = x0; x < x1; x++) {
				const n = (((x * 73856093) ^ (y * 19349663)) >>> 0) % 7;
				g.fillStyle = `rgb(${43 + n}, ${39 + n}, ${33 + n})`;
				g.fillRect(ox + x * cell, oy + y * cell, cell + 0.5, cell + 0.5);
			}
		}
		const [fx, fy, fw, fh] = [ox, oy, cols * cell, rows * cell];
		// Each line is cut dark with a lit edge beside it, as a joint is.
		for (const [shift, stroke] of [
			[0.5, 'rgba(0, 0, 0, 0.62)'],
			[1.5, 'rgba(255, 238, 200, 0.045)']
		] as const) {
			g.lineWidth = 1;
			g.strokeStyle = stroke;
			g.beginPath();
			for (let x = x0; x <= x1; x++) {
				const at = Math.round(ox + x * cell) + shift;
				g.moveTo(at, Math.max(0, fy));
				g.lineTo(at, Math.min(box.height, fy + fh));
			}
			for (let y = y0; y <= y1; y++) {
				const at = Math.round(oy + y * cell) + shift;
				g.moveTo(Math.max(0, fx), at);
				g.lineTo(Math.min(box.width, fx + fw), at);
			}
			g.stroke();
		}
		g.lineWidth = 2;
		g.strokeStyle = '#6b5c43';
		g.strokeRect(Math.round(fx) - 1, Math.round(fy) - 1, Math.round(fw) + 2, Math.round(fh) + 2);

		// Lit in the middle, dark at the edges.
		const [mx, my] = [left + 12 + clearW / 2, top + 12 + clearH / 2];
		const dusk = g.createRadialGradient(
			mx,
			my,
			Math.min(clearW, clearH) * 0.35,
			mx,
			my,
			Math.max(box.width, box.height) * 0.62
		);
		dusk.addColorStop(0, 'rgba(0, 0, 0, 0)');
		dusk.addColorStop(1, 'rgba(0, 0, 0, 0.78)');
		g.fillStyle = dusk;
		g.fillRect(0, 0, box.width, box.height);

		// The map measured: feet along its top and down its side, kept in
		// view when the map's own edge is not, and its size in a corner.
		const every = cell >= 26 ? 1 : cell >= 13 ? 2 : 5;
		g.font = `600 ${cell >= 26 ? 10 : 9}px ${SANS}`;
		g.lineWidth = 3;
		g.lineJoin = 'round';
		g.strokeStyle = 'rgba(0, 0, 0, 0.9)';
		g.fillStyle = '#8f8674';
		const mark = (text: string, x: number, y: number) => {
			g.strokeText(text, x, y);
			g.fillText(text, x, y);
		};
		const rulerY = Math.max(fy - 9, top + 12);
		const rulerX = Math.max(fx - 13, left + 14);
		for (let x = x0; x <= x1; x += 1) {
			if (x % every === 0 && ox + x * cell > rulerX + 10) mark(String(x * FEET), ox + x * cell, rulerY);
		}
		for (let y = y0; y <= y1; y += 1) {
			if (y % every === 0 && oy + y * cell > rulerY + 10) mark(String(y * FEET), rulerX, oy + y * cell);
		}
		g.textAlign = 'left';
		g.font = `600 11px ${SANS}`;
		mark(
			`${cols} × ${rows} squares  ·  ${cols * FEET} × ${rows * FEET} ft  ·  1 square = ${FEET} ft`,
			left + 22,
			top + 12 + clearH - 4
		);
		g.textAlign = 'center';

		/** Where a token stands, before anything this beat does to it. */
		const home = (seat: number): [number, number, number] => {
			const [x, y] = where(replay, frame, seat, ease);
			const size = replay.units[seat].span * cell;
			return [ox + x * cell + size / 2, oy + y * cell + size / 2, size];
		};
		const present = (seat: number | null): seat is number =>
			seat != null && !!beat.units[seat] && !beat.units[seat].hidden;
		const change = (seat: number) =>
			beat.units[seat].hp - (before?.units[seat]?.hp ?? beat.units[seat].hp);
		const melee = detail?.type === 'attack' && !detail.ranged && detail.school !== 'spell';

		/** Where a token is drawn: a blade lunges, whoever is hit flinches, a miss is dodged. */
		const centre = (seat: number): [number, number, number] => {
			let [cx, cy, size] = home(seat);
			if (melee && seat === beat.actor && present(beat.target)) {
				const [tx, ty] = home(beat.target);
				const reach = Math.sin(Math.PI * span(p, 0, 0.7)) * Math.min(cell * 0.55, 30);
				const far = Math.hypot(tx - cx, ty - cy) || 1;
				cx += ((tx - cx) / far) * reach;
				cy += ((ty - cy) / far) * reach;
			}
			if (detail?.type === 'damage' && seat === beat.target && detail.amount > 0) {
				cx += Math.sin(p * 30) * (1 - p) * cell * 0.1;
			}
			if (detail?.type === 'attack' && detail.outcome === 'miss' && seat === beat.target) {
				cx += Math.sin(Math.PI * span(p, 0.3, 0.9)) * cell * 0.22;
			}
			return [cx, cy, size];
		};

		const ring = (
			cx: number,
			cy: number,
			radius: number,
			stroke: string,
			width: number,
			alpha = 1
		) => {
			if (radius <= 0 || alpha <= 0) return;
			g.globalAlpha = alpha;
			g.strokeStyle = stroke;
			g.lineWidth = width;
			g.beginPath();
			g.arc(cx, cy, radius, 0, Math.PI * 2);
			g.stroke();
			g.globalAlpha = 1;
		};
		/** Words rising off a token and fading. */
		const call = (text: string, cx: number, cy: number, fill: string, size: number, from = 0) => {
			const q = span(p, from, 1);
			if (q <= 0 || q >= 1) return;
			g.globalAlpha = Math.min(1, (1 - q) * 2.2) * Math.min(1, q * 8);
			g.font = `700 ${Math.max(13, size)}px ${SERIF}`;
			g.lineWidth = 3.5;
			g.lineJoin = 'round';
			g.strokeStyle = 'rgba(0, 0, 0, 0.95)';
			g.fillStyle = fill;
			const y = cy - smooth(q) * cell * 0.6;
			g.strokeText(text, cx, y);
			g.fillText(text, cx, y);
			g.globalAlpha = 1;
		};

		// ---- under the tokens ----

		// The walk: footprints from where it started to where it stops.
		if (beat.kind === 'move' && present(beat.actor) && before?.units[beat.actor]) {
			const half = (replay.units[beat.actor].span * cell) / 2;
			const from = before.units[beat.actor];
			g.strokeStyle = 'rgba(201, 164, 92, 0.6)';
			g.lineWidth = Math.max(2, cell * 0.07);
			g.setLineDash([1, cell * 0.28]);
			g.beginPath();
			g.moveTo(ox + from.x * cell + half, oy + from.y * cell + half);
			for (const [x, y] of beat.path) g.lineTo(ox + x * cell + half, oy + y * cell + half);
			g.stroke();
			g.setLineDash([]);
		}

		// An aura that is up: a wash of light on the floor around its owner.
		beat.units.forEach((state, seat) => {
			if (!state.alive || state.hidden || !state.conditions.includes('aura')) return;
			const [cx, cy, size] = home(seat);
			const reach = size / 2 + cell * 2;
			const glow = g.createRadialGradient(cx, cy, size / 2, cx, cy, reach);
			glow.addColorStop(0, 'rgba(255, 233, 168, 0.20)');
			glow.addColorStop(1, 'rgba(255, 233, 168, 0)');
			g.fillStyle = glow;
			g.beginPath();
			g.arc(cx, cy, reach, 0, Math.PI * 2);
			g.fill();
			g.strokeStyle = 'rgba(255, 233, 168, 0.35)';
			g.lineWidth = 1.5;
			g.setLineDash([3, 7]);
			g.lineDashOffset = now / 90;
			g.beginPath();
			g.arc(cx, cy, reach, 0, Math.PI * 2);
			g.stroke();
			g.setLineDash([]);
		});

		// A turn starting: a ring of light opens under whoever's it is.
		if ((beat.kind === 'turn' || beat.kind === 'initiative') && present(beat.actor)) {
			const [cx, cy, size] = home(beat.actor);
			ring(cx, cy, size * (0.45 + 0.5 * ease), GOLD, 2, 1 - p);
		}

		// A move being taken, drawn as what it is.
		if (detail?.type === 'use' && present(beat.actor)) {
			const [cx, cy, size] = home(beat.actor);
			const fade = 1 - span(p, 0.6, 1);
			if (detail.school === 'spell') {
				// A casting circle: two rings of runes turning against each other.
				g.save();
				g.translate(cx, cy);
				g.shadowColor = ARCANE;
				g.shadowBlur = 14;
				for (const [radius, turn, dash] of [
					[0.62, 1, [10, 7]],
					[0.82, -1, [3, 9]]
				] as const) {
					g.rotate(turn * p * 1.6);
					g.globalAlpha = fade * Math.min(1, p * 5);
					g.strokeStyle = ARCANE;
					g.lineWidth = 2.5;
					g.setLineDash([...dash]);
					g.beginPath();
					g.arc(0, 0, size * radius * (0.7 + 0.3 * ease), 0, Math.PI * 2);
					g.stroke();
				}
				g.restore();
				g.setLineDash([]);
				g.globalAlpha = 1;
			} else if (detail.school === 'item') {
				// Something drunk or thrown open: bubbles rising off the user.
				for (let i = 0; i < 9; i++) {
					const drift = ((i * 37) % 100) / 100;
					const q = (p * 1.2 + drift * 0.5) % 1;
					g.globalAlpha = (1 - q) * fade;
					g.fillStyle = i % 3 ? '#7fe3a6' : '#ffd0f0';
					g.beginPath();
					g.arc(
						cx + (drift - 0.5) * size * 0.9 + Math.sin(q * 9 + i) * 3,
						cy + size * 0.3 - q * size * 0.95,
						2 + (i % 3) * 1.4,
						0,
						Math.PI * 2
					);
					g.fill();
				}
				g.globalAlpha = 1;
			} else if (detail.school === 'weapon') {
				// Steel drawn back: an arc swept round the wielder.
				g.globalAlpha = fade;
				g.strokeStyle = STEEL;
				g.lineWidth = 3;
				g.beginPath();
				g.arc(cx, cy, size * 0.62, -2.4 + ease * 2.2, -1.5 + ease * 2.2);
				g.stroke();
				g.globalAlpha = 1;
			} else {
				ring(cx, cy, size * (0.4 + 0.45 * ease), STEEL, 2.5, fade);
			}
			// And at whom, when it is aimed at somebody.
			if (present(beat.target) && beat.target !== beat.actor) {
				const [tx, ty] = home(beat.target);
				g.globalAlpha = 0.35 * fade;
				g.strokeStyle = detail.school === 'spell' ? ARCANE : STEEL;
				g.lineWidth = 1.5;
				g.setLineDash([4, 6]);
				g.beginPath();
				g.moveTo(cx, cy);
				g.lineTo(cx + (tx - cx) * ease, cy + (ty - cy) * ease);
				g.stroke();
				g.setLineDash([]);
				g.globalAlpha = 1;
			}
		}

		// The space each one takes up: its squares, marked in its side's colour.
		beat.units.forEach((state, seat) => {
			if (!state.alive || state.hidden) return;
			const [cx, cy, size] = home(seat);
			const foe = replay.units[seat].team === 1;
			g.fillStyle = foe ? 'rgba(194, 86, 74, 0.13)' : 'rgba(109, 155, 208, 0.13)';
			g.fillRect(cx - size / 2 + 1, cy - size / 2 + 1, size - 1, size - 1);
			g.lineWidth = 1;
			g.strokeStyle = foe ? 'rgba(194, 86, 74, 0.5)' : 'rgba(109, 155, 208, 0.5)';
			g.strokeRect(cx - size / 2 + 1.5, cy - size / 2 + 1.5, size - 2, size - 2);
		});

		// ---- the tokens: the fallen first, so the living are drawn over them ----

		const turnOf = (() => {
			for (let i = frame; i >= 0; i--) {
				const kind = replay.frames[i].kind;
				if (kind === 'turn' || kind === 'lost') return replay.frames[i].actor;
			}
			return null;
		})();
		const seats = beat.units
			.map((_, seat) => seat)
			.sort((i, j) => Number(beat.units[i].alive) - Number(beat.units[j].alive));
		for (const seat of seats) {
			const unit = replay.units[seat];
			const state = beat.units[seat];
			if (state.hidden) continue;
			const [cx, cy, size] = centre(seat);
			const edge = Math.max(2, cell * 0.06);
			const radius = size / 2 - edge / 2 - Math.max(2.5, cell * 0.05);
			const paint = paints[seat];
			// Dropping this beat: it greys out as it falls rather than at once.
			const falling = beat.kind === 'down' && beat.target === seat ? 1 - ease : 0;
			// And it is still on its feet for the blow that drops it.
			const dropping = next?.kind === 'down' && next.target === seat;
			const alive = state.alive || falling > 0 || dropping;

			// Whose turn it is stands in a circle of gold on the floor; whoever
			// is being aimed at, in a red one with a mark at each quarter.
			if (turnOf === seat && state.alive) {
				const around = radius + edge + Math.max(3, cell * 0.07);
				g.shadowColor = 'rgba(201, 164, 92, 0.9)';
				g.shadowBlur = cell * 0.25;
				ring(cx, cy, around, GOLD, Math.max(1.5, cell * 0.04), 0.75 + 0.25 * Math.sin(now / 320));
				g.shadowBlur = 0;
				g.shadowColor = 'transparent';
			}
			if (
				beat.target === seat &&
				beat.actor !== seat &&
				state.alive &&
				(detail?.type === 'use' || detail?.type === 'attack')
			) {
				const around = radius + edge + Math.max(3, cell * 0.07);
				ring(cx, cy, around, '#c2564a', Math.max(1.5, cell * 0.035), 0.9);
				g.strokeStyle = '#c2564a';
				g.lineWidth = Math.max(1.5, cell * 0.035);
				g.beginPath();
				for (let i = 0; i < 4; i++) {
					const a = (i * Math.PI) / 2 + Math.PI / 4;
					g.moveTo(cx + Math.cos(a) * (around - 4), cy + Math.sin(a) * (around - 4));
					g.lineTo(cx + Math.cos(a) * (around + 5), cy + Math.sin(a) * (around + 5));
				}
				g.stroke();
			}

			const solid = state.alive || dropping ? 1 : 0.28 + 0.72 * falling;
			g.globalAlpha = solid;
			g.shadowColor = 'rgba(0, 0, 0, 0.7)';
			g.shadowBlur = cell * 0.14;
			g.shadowOffsetY = cell * 0.05;
			const under = state.alive ? state.conditions : [];
			// Knocked down: it lies flat until it gets up.
			const flat = under.includes('prone') ? 0.68 : 1;
			// A dark face lit from above, in a ring of its side's metal.
			const body = g.createRadialGradient(cx, cy - radius * 0.55, radius * 0.1, cx, cy, radius);
			body.addColorStop(0, alive ? paint.light : '#3b3936');
			body.addColorStop(1, alive ? paint.dark : '#191816');
			g.fillStyle = body;
			g.beginPath();
			g.ellipse(cx, cy + radius * (1 - flat), radius, radius * flat, 0, 0, Math.PI * 2);
			g.fill();
			g.shadowColor = 'transparent';
			g.shadowBlur = 0;
			g.shadowOffsetY = 0;
			g.lineWidth = edge + 2;
			g.strokeStyle = '#000';
			g.stroke();
			g.lineWidth = edge;
			g.strokeStyle = alive ? paint.ring : '#55514b';
			g.stroke();

			// Struck: a flash in the colour of what struck it.
			const delta = change(seat);
			if (delta < 0 && p < 0.6) {
				g.globalAlpha = 0.7 * (1 - p / 0.6);
				g.fillStyle = colour;
				g.beginPath();
				g.arc(cx, cy, radius, 0, Math.PI * 2);
				g.fill();
			}

			g.globalAlpha = solid;
			g.fillStyle = alive ? BONE : '#8a857c';
			g.font = `600 ${Math.max(9, radius * 0.82)}px ${SERIF}`;
			g.fillText(alive ? initials(unit.name) : '✕', cx, cy + radius * (1 - flat) + 1);
			g.globalAlpha = 1;

			if (!state.alive) continue;

			// Everything about it is kept inside its own squares, so two
			// standing side by side do not write over each other. Along the
			// top: what it has left, with temporary hit points over it in blue.
			const corner = Math.max(7, Math.min(11, cell * 0.2));
			const badged = cell >= 18;
			// The bar starts clear of the shield in the corner beside it.
			const plateX = cx - size / 2 + (badged ? corner * 1.5 : 3);
			const plateW = cx + size / 2 - 3 - plateX;
			const plateH = Math.max(4, Math.min(7, cell * 0.1));
			const plateY = cy - size / 2 + 3;
			if (unit.max_hp > 0) {
				const share = Math.max(0, Math.min(1, state.hp / unit.max_hp));
				g.fillStyle = '#000';
				g.fillRect(plateX - 1, plateY - 1, plateW + 2, plateH + 2);
				g.fillStyle = '#241412';
				g.fillRect(plateX, plateY, plateW, plateH);
				g.fillStyle = share > 0.5 ? '#5f9c53' : share > 0.25 ? '#b8902f' : '#b3423a';
				g.fillRect(plateX, plateY, plateW * share, plateH);
				g.fillStyle = 'rgba(255, 255, 255, 0.16)';
				g.fillRect(plateX, plateY, plateW * share, plateH * 0.4);
				if (state.temp_hp > 0) {
					const extra = Math.min(1, state.temp_hp / unit.max_hp);
					g.fillStyle = TEMP;
					g.fillRect(plateX, plateY, plateW * extra, Math.max(2, plateH * 0.45));
				}
			}
			// Along the bottom: its name, where there is room to read one.
			if (cell >= 34) {
				const font = Math.max(9, Math.min(12, cell * 0.19));
				g.font = `600 ${font}px ${SANS}`;
				let name = unit.name;
				while (name.length > 3 && g.measureText(name).width > size - 2) {
					name = name.slice(0, -2) + '…';
				}
				const y = cy + size / 2 - font * 0.62;
				g.lineWidth = 3;
				g.lineJoin = 'round';
				g.strokeStyle = 'rgba(0, 0, 0, 0.92)';
				g.strokeText(name, cx, y);
				g.fillStyle = unit.team === 0 ? '#bcd2ea' : '#eab9b0';
				g.fillText(name, cx, y);
			}
			// On its upper corners, where a round token leaves its square
			// empty: its Armor Class on a shield - blue while something is
			// raising it, red while something has lowered it - and the
			// temporary hit points it is carrying.
			if (badged) {
				const [sx, sy] = [cx - size / 2 + corner * 0.55, cy - size / 2 + corner * 0.6];
				g.beginPath();
				g.moveTo(sx - corner, sy - corner);
				g.lineTo(sx + corner, sy - corner);
				g.lineTo(sx + corner, sy);
				g.quadraticCurveTo(sx + corner * 0.9, sy + corner * 0.85, sx, sy + corner * 1.2);
				g.quadraticCurveTo(sx - corner * 0.9, sy + corner * 0.85, sx - corner, sy);
				g.closePath();
				g.fillStyle = '#12100d';
				g.fill();
				g.lineWidth = 1.5;
				g.strokeStyle = '#000';
				g.stroke();
				g.lineWidth = 1;
				g.strokeStyle = '#a79f8f';
				g.stroke();
				g.font = `700 ${corner * 1.1}px ${SANS}`;
				g.fillStyle = state.ac > unit.ac ? TEMP : state.ac < unit.ac ? '#e08a7c' : BONE;
				g.fillText(String(state.ac), sx, sy + corner * 0.05);
				if (state.temp_hp > 0) {
					const text = `+${state.temp_hp}`;
					g.font = `700 ${corner * 1.1}px ${SANS}`;
					const wide = g.measureText(text).width + 6;
					const [tx, ty] = [cx + size / 2 - wide / 2 + 2, sy + plateH + corner * 0.9];
					g.fillStyle = '#0d1418';
					g.fillRect(tx - wide / 2, ty - corner, wide, corner * 2);
					g.lineWidth = 1;
					g.strokeStyle = TEMP;
					g.strokeRect(tx - wide / 2 + 0.5, ty - corner + 0.5, wide - 1, corner * 2 - 1);
					g.fillStyle = TEMP;
					g.fillText(text, tx, ty + 0.5);
				}
			}

			// What it is under, on the token itself. Poison sickens its
			// colour; whatever holds it still drains it.
			const wash = under.includes('poisoned')
				? 'rgba(110, 200, 70, 0.32)'
				: under.some((c) => ['paralyzed', 'petrified', 'incapacitated'].includes(c))
					? 'rgba(160, 190, 215, 0.38)'
					: null;
			if (wash) {
				g.fillStyle = wash;
				g.beginPath();
				g.ellipse(cx, cy + radius * (1 - flat), radius, radius * flat, 0, 0, Math.PI * 2);
				g.fill();
			}
			// Stunned: stars going round its head.
			if (under.includes('stunned')) {
				g.fillStyle = '#ffd84d';
				for (let i = 0; i < 3; i++) {
					const a = now / 260 + (i * Math.PI * 2) / 3;
					const [sx, sy] = [cx + Math.cos(a) * radius * 0.9, cy - radius * 0.95 + Math.sin(a) * radius * 0.22];
					g.beginPath();
					for (let k = 0; k < 10; k++) {
						const b = -Math.PI / 2 + (k * Math.PI) / 5;
						const d = (k % 2 ? 0.45 : 1) * Math.max(3, cell * 0.09);
						g.lineTo(sx + Math.cos(b) * d, sy + Math.sin(b) * d);
					}
					g.closePath();
					g.fill();
				}
			}
			// Held in place: a band around it.
			if (under.includes('restrained')) {
				g.strokeStyle = '#c3ccd9';
				g.lineWidth = Math.max(2, cell * 0.06);
				g.setLineDash([cell * 0.12, cell * 0.08]);
				g.beginPath();
				g.arc(cx, cy, radius * 0.72, 0, Math.PI * 2);
				g.stroke();
				g.setLineDash([]);
			}
			// Concentrating: a slow ring of runes turning round it.
			if (state.concentrating) {
				g.strokeStyle = CONCENTRATING.colour;
				g.globalAlpha = 0.85;
				g.lineWidth = Math.max(1.5, cell * 0.045);
				g.setLineDash([2, cell * 0.14]);
				g.lineDashOffset = -now / 70;
				g.beginPath();
				g.arc(cx, cy, radius + Math.max(7, cell * 0.2), 0, Math.PI * 2);
				g.stroke();
				g.setLineDash([]);
				g.globalAlpha = 1;
			}
			// And each of them as a badge along its lower edge.
			const marks = statuses(under);
			if (state.concentrating) marks.push(CONCENTRATING);
			const r = Math.max(5, Math.min(cell * 0.14, 9));
			const fit = Math.max(1, Math.min(4, Math.floor(size / (r * 2 + 3)) - 1));
			const shownMarks = marks.slice(0, marks.length > fit ? Math.max(1, fit - 1) : fit);
			const markY = cy - size / 2 - r + 2;
			shownMarks.forEach((mark, i) => {
				const x = cx + (i - (shownMarks.length - 1) / 2) * (r * 2 + 3);
				badge(g, mark, x, markY, r);
			});
			if (marks.length > shownMarks.length) {
				g.fillStyle = BONE;
				g.font = `600 ${r * 1.4}px ${SANS}`;
				g.fillText(
					`+${marks.length - shownMarks.length}`,
					cx + ((shownMarks.length + 1) / 2) * (r * 2 + 3),
					markY
				);
			}
		}

		// ---- over the tokens ----

		// An attack on its way: a cut for a blade, a shaft for a bow, a bolt
		// of light for a spell - then whether it landed.
		if (detail?.type === 'attack' && present(beat.actor) && present(beat.target)) {
			const [ax, ay] = home(beat.actor);
			const [tx, ty, tsize] = centre(beat.target);
			const angle = Math.atan2(ty - ay, tx - ax);
			const flight = smooth(span(p, 0, 0.5));
			const landed = detail.outcome !== 'miss';
			if (melee) {
				// The cut: a crescent across the target as the lunge arrives.
				const q = span(p, 0.25, 0.7);
				if (q > 0 && q < 1) {
					g.save();
					g.translate(tx, ty);
					g.rotate(angle);
					g.globalAlpha = Math.sin(Math.PI * q);
					g.strokeStyle = colour;
					g.shadowColor = colour;
					g.shadowBlur = 10;
					g.lineWidth = Math.max(3, cell * 0.1);
					g.beginPath();
					g.arc(-tsize * 0.15, 0, tsize * 0.55, -1.1 + q * 0.5, 1.1 - (1 - q) * 0.5);
					g.stroke();
					g.restore();
					g.globalAlpha = 1;
				}
			} else if (flight < 1) {
				// Overshoots a little when it is going to miss.
				const reach = landed ? flight : flight * 1.25;
				const [hx, hy] = [ax + (tx - ax) * reach, ay + (ty - ay) * reach];
				const tail = detail.school === 'spell' ? 0.22 : 0.12;
				const back = Math.max(0, reach - tail);
				const trail = g.createLinearGradient(
					ax + (tx - ax) * back,
					ay + (ty - ay) * back,
					hx,
					hy
				);
				trail.addColorStop(0, 'rgba(255,255,255,0)');
				trail.addColorStop(1, colour);
				g.strokeStyle = trail;
				g.lineWidth = detail.school === 'spell' ? Math.max(4, cell * 0.13) : 2.5;
				g.beginPath();
				g.moveTo(ax + (tx - ax) * back, ay + (ty - ay) * back);
				g.lineTo(hx, hy);
				g.stroke();
				if (detail.school === 'spell') {
					g.shadowColor = colour;
					g.shadowBlur = 18;
					g.fillStyle = '#fff';
					g.beginPath();
					g.arc(hx, hy, Math.max(4, cell * 0.11), 0, Math.PI * 2);
					g.fill();
					g.shadowBlur = 0;
				}
			}
			const text = Math.max(15, cell * 0.36);
			if (!landed) call('Miss', tx, ty - tsize * 0.55, '#bdb6a8', text, 0.4);
			else if (detail.outcome === 'crit') {
				ring(tx, ty, tsize * (0.3 + 0.9 * span(p, 0.45, 1)), GOLD, 3, 1 - span(p, 0.45, 1));
				call('Critical!', tx, ty - tsize * 0.55, '#f0cf7a', text * 1.15, 0.4);
			} else {
				ring(tx, ty, tsize * (0.3 + 0.5 * span(p, 0.45, 1)), colour, 3, 1 - span(p, 0.45, 1));
			}
		}

		// Damage: a burst in the colour of its type, and the number.
		if (detail?.type === 'damage' && present(beat.target)) {
			const [cx, cy, size] = centre(beat.target);
			const q = span(p, 0, 0.7);
			if (detail.amount > 0 && q < 1) {
				g.strokeStyle = colour;
				g.lineWidth = Math.max(2, cell * 0.06);
				g.globalAlpha = 1 - q;
				for (let i = 0; i < 10; i++) {
					const a = (i / 10) * Math.PI * 2 + (beat.target ?? 0);
					const near = size * (0.3 + 0.35 * q);
					const far = size * (0.42 + 0.6 * q);
					g.beginPath();
					g.moveTo(cx + Math.cos(a) * near, cy + Math.sin(a) * near);
					g.lineTo(cx + Math.cos(a) * far, cy + Math.sin(a) * far);
					g.stroke();
				}
				g.globalAlpha = 1;
			}
			// A critical hit's number is the bigger for it.
			const crit = before?.detail?.type === 'attack' && before.detail.outcome === 'crit';
			call(
				detail.amount > 0 ? String(detail.amount) : 'No damage',
				cx,
				cy - size * 0.55,
				detail.amount > 0 ? colour : '#bdb6a8',
				Math.max(17, cell * (crit ? 0.62 : 0.46))
			);
		}

		// Healing: green light drawing in, crosses rising.
		if (detail?.type === 'heal' && present(beat.target)) {
			const [cx, cy, size] = centre(beat.target);
			ring(cx, cy, size * (1.0 - 0.5 * ease), '#7fe3a6', 3, 1 - p);
			g.strokeStyle = '#7fe3a6';
			g.lineWidth = 2.5;
			for (let i = 0; i < 5; i++) {
				const q = (p + i * 0.17) % 1;
				const x = cx + (i - 2) * size * 0.2;
				const y = cy + size * 0.25 - q * size * 0.9;
				g.globalAlpha = (1 - q) * (1 - span(p, 0.7, 1));
				g.beginPath();
				g.moveTo(x - 4, y);
				g.lineTo(x + 4, y);
				g.moveTo(x, y - 4);
				g.lineTo(x, y + 4);
				g.stroke();
			}
			g.globalAlpha = 1;
			call(`+${detail.amount}`, cx, cy - size * 0.55, '#8fd67f', Math.max(17, cell * 0.46));
		}

		// A saving throw: a ward that holds, or breaks.
		if (detail?.type === 'save' && present(beat.actor)) {
			const [cx, cy, size] = centre(beat.actor);
			const resisted = detail.how === 'legendary resistance';
			const hue = resisted ? GOLD : detail.passed ? '#8fcdf2' : '#c2564a';
			if (detail.passed) {
				ring(cx, cy, size * (0.5 + 0.25 * Math.sin(Math.PI * p)), hue, 4, 1 - span(p, 0.6, 1));
			} else {
				// Broken: the ward in pieces, falling away.
				g.strokeStyle = hue;
				g.lineWidth = 3;
				g.globalAlpha = 1 - span(p, 0.4, 1);
				for (let i = 0; i < 6; i++) {
					const a = (i / 6) * Math.PI * 2;
					g.beginPath();
					g.arc(cx, cy + ease * size * 0.15, size * (0.62 + 0.2 * ease), a, a + 0.7);
					g.stroke();
				}
				g.globalAlpha = 1;
			}
			call(
				resisted ? 'Resisted' : detail.passed ? 'Saved' : 'Failed',
				cx,
				cy - size * 0.55,
				hue,
				Math.max(14, cell * 0.32),
				0.35
			);
		}

		// A condition taking hold: it is named, and circles its victim.
		if (detail?.type === 'condition' && present(beat.target)) {
			const [cx, cy, size] = centre(beat.target);
			g.fillStyle = GOLD;
			for (let i = 0; i < 3; i++) {
				const a = p * 5 + (i / 3) * Math.PI * 2;
				g.globalAlpha = 1 - span(p, 0.6, 1);
				g.beginPath();
				g.arc(cx + Math.cos(a) * size * 0.6, cy + Math.sin(a) * size * 0.3 - size * 0.25, 3.5, 0, Math.PI * 2);
				g.fill();
			}
			g.globalAlpha = 1;
			call(
				detail.name.charAt(0).toUpperCase() + detail.name.slice(1).replaceAll('_', ' '),
				cx,
				cy - size * 0.6,
				'#f0cf7a',
				Math.max(13, cell * 0.3)
			);
		}

		if (beat.kind === 'down' && present(beat.target)) {
			const [cx, cy, size] = centre(beat.target);
			ring(cx, cy, size * (0.4 + 0.6 * ease), '#c2564a', 3, 1 - p);
			call('Down', cx, cy - size * 0.55, '#e08a7c', Math.max(15, cell * 0.38));
		}
		if (beat.kind === 'lost' && present(beat.actor)) {
			const [cx, cy, size] = centre(beat.actor);
			call('Loses its turn', cx, cy - size * 0.55, '#bdb6a8', Math.max(13, cell * 0.28));
		}
		if (detail?.type === 'initiative' && present(beat.actor)) {
			const [cx, cy, size] = centre(beat.actor);
			call(String(detail.total), cx, cy - size * 0.55, '#f0cf7a', Math.max(16, cell * 0.44), 0.3);
		}
		// What is being used, named over whoever uses it.
		if (detail?.type === 'use' && present(beat.actor)) {
			const [cx, cy, size] = centre(beat.actor);
			call(
				detail.name,
				cx,
				cy - size * 0.62,
				detail.school === 'spell' ? '#d0c2f5' : detail.school === 'item' ? '#a5dcb4' : BONE,
				Math.max(13, cell * 0.3)
			);
		}
	}
</script>

<canvas bind:this={canvas}></canvas>

<style>
	canvas {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
	}
</style>
