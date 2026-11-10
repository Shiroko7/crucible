<script lang="ts">
	// One die, cut like what it is, turning over for a moment before it shows
	// what it rolled.
	import { onMount } from 'svelte';

	let {
		sides,
		value,
		size = 46,
		dropped = false,
		maxed = false,
		tone = 'plain'
	}: {
		sides: number;
		value: number;
		size?: number;
		/** Rolled but not the one that counted: the low die of advantage. */
		dropped?: boolean;
		/** Forced onto its highest face rather than rolled: it lands, and locks. */
		maxed?: boolean;
		tone?: 'plain' | 'best' | 'worst';
	} = $props();

	let shown = $state(0);
	let rolling = $state(true);

	// Each die seen face on: its outline, and the edges between its faces.
	const HEX = '50,3 91,26.5 91,73.5 50,97 9,73.5 9,26.5';
	const CUTS: Record<number, { outline: string; edges: string; lift: number }> = {
		4: { outline: '50,6 95,90 5,90', edges: 'M50 6 50 62M95 90 50 62M5 90 50 62', lift: 8 },
		6: { outline: '12,12 88,12 88,88 12,88', edges: 'M19 19H81V81H19Z', lift: 0 },
		8: { outline: HEX, edges: 'M50 3 91 73.5H9Z', lift: 3 },
		10: {
			outline: '50,3 95,44 50,97 5,44',
			edges: 'M50 14 75 51 50 66 25 51ZM75 51 95 44M25 51 5 44M50 66V97',
			lift: -8
		},
		12: {
			outline: '50,3 79,12 97,36 97,64 79,88 50,97 21,88 3,64 3,36 21,12',
			edges: 'M50 22 77 41 67 73H33L23 41ZM50 22V3M77 41 97 36M67 73 79 88M33 73 21 88M23 41 3 36',
			lift: 0
		},
		20: {
			outline: HEX,
			edges:
				'M50 24 76 68H24ZM50 24V3M50 24 91 26.5M50 24 9 26.5M76 68 91 26.5M76 68 91 73.5M76 68 50 97M24 68 9 26.5M24 68 9 73.5M24 68 50 97',
			lift: 3
		}
	};
	const cut = $derived(CUTS[sides] ?? CUTS[6]);

	// Turn through a few faces, then land.
	onMount(() => {
		shown = 1 + Math.floor(Math.random() * sides);
		const spin = setInterval(() => (shown = 1 + Math.floor(Math.random() * sides)), 60);
		const land = setTimeout(() => {
			clearInterval(spin);
			shown = value;
			rolling = false;
		}, 360);
		return () => {
			clearInterval(spin);
			clearTimeout(land);
		};
	});
</script>

<span
	class="die {maxed ? 'best' : tone}"
	class:rolling
	class:dropped
	class:maxed
	style:width="{size}px"
	style:height="{size}px"
	style:font-size="{Math.round(size * (sides === 20 || sides === 10 ? 0.3 : 0.36))}px"
	title="d{sides}"
>
	<svg viewBox="0 0 100 100" aria-hidden="true">
		<polygon class="body" points={cut.outline} />
		<path class="edges" d={cut.edges} />
		<polygon class="rim" points={cut.outline} />
	</svg>
	<b style:top="{cut.lift}%">{rolling ? shown : value}</b>
	<small>{maxed && !rolling ? 'max' : `d${sides}`}</small>
</span>

<style>
	.die {
		position: relative;
		flex: none;
		display: inline-grid;
		place-items: center;
		margin-bottom: 13px;
		--from: #3a342c;
		--to: #14110e;
		--rim: #7d705c;
		--ink: #ece3d0;
		filter: drop-shadow(0 3px 4px rgba(0, 0, 0, 0.7));
		transition:
			opacity 0.25s,
			transform 0.25s;
	}
	svg {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		overflow: visible;
	}
	.body {
		fill: var(--to);
		stroke: none;
	}
	.edges {
		fill: var(--from);
		fill-opacity: 0.75;
		stroke: var(--rim);
		stroke-opacity: 0.45;
		stroke-width: 1.4;
		stroke-linejoin: round;
	}
	.rim {
		fill: none;
		stroke: var(--rim);
		stroke-width: 2.6;
		stroke-linejoin: round;
	}
	.die b {
		position: relative;
		color: var(--ink);
		font-family: var(--display);
		font-weight: 700;
		font-variant-numeric: tabular-nums;
		text-shadow: 0 1px 2px #000;
	}
	.die small {
		position: absolute;
		top: 100%;
		margin-top: 1px;
		font-size: 9px;
		line-height: 12px;
		color: var(--faint);
		letter-spacing: 0.08em;
		text-transform: uppercase;
	}
	.die.best {
		--from: #6b5220;
		--to: #2a1f0a;
		--rim: #d9b566;
		--ink: #ffe6a8;
	}
	.die.worst {
		--from: #5a1c17;
		--to: #1f0907;
		--rim: #b8483d;
		--ink: #ffb9ad;
	}
	.die.maxed:not(.rolling) {
		filter: drop-shadow(0 0 5px rgba(217, 181, 102, 0.75));
		animation: lock 0.25s ease-out;
	}
	.die.maxed small {
		color: var(--accent);
	}
	@keyframes lock {
		from {
			transform: scale(1.25);
		}
	}
	.die.dropped {
		opacity: 0.35;
		transform: scale(0.82);
	}
	.die.dropped b {
		text-decoration: line-through;
	}
	.die.rolling {
		animation: tumble 0.36s ease-out;
	}
	@keyframes tumble {
		0% {
			transform: translateY(-14px) rotate(-120deg);
			opacity: 0.2;
		}
		100% {
			transform: none;
		}
	}
</style>
