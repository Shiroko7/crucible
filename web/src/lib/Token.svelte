<script lang="ts">
	// A creature's face: a round token on the map, a square portrait in the
	// interface. Its initials for now, and the one place a picture goes once
	// creatures have them.
	import { colours, initials } from './names';

	let {
		name,
		team,
		size = 40,
		square = false,
		down = false,
		active = false,
		count = 1
	}: {
		name: string;
		team: number;
		size?: number;
		/** A portrait in a frame rather than a token on a map. */
		square?: boolean;
		down?: boolean;
		active?: boolean;
		/** How many of it this one token stands for. */
		count?: number;
	} = $props();

	const paint = $derived(colours(name, team));
</script>

<span
	class="token"
	class:square
	class:down
	class:active
	title={count > 1 ? `${count} × ${name}` : name}
	style:width="{size}px"
	style:height="{size}px"
	style:font-size="{Math.round(size * 0.4)}px"
	style:--light={paint.light}
	style:--dark={paint.dark}
	style:--ring={paint.ring}
>
	{initials(name)}
	{#if count > 1}
		<span class="count">×{count}</span>
	{/if}
</span>

<style>
	.token {
		position: relative;
		flex: none;
		display: inline-grid;
		place-items: center;
		border-radius: 50%;
		font-family: var(--display);
		font-weight: 600;
		line-height: 1;
		color: #efe6d2;
		background: radial-gradient(circle at 50% 22%, var(--light), var(--dark) 78%);
		box-shadow:
			inset 0 0 0 1px rgba(0, 0, 0, 0.75),
			inset 0 0 8px rgba(0, 0, 0, 0.5),
			0 0 0 2px var(--ring),
			0 0 0 3px #000,
			0 2px 6px rgba(0, 0, 0, 0.6);
		text-shadow: 0 1px 2px #000;
		transition:
			box-shadow 0.2s,
			filter 0.2s;
	}
	.token.square {
		border-radius: 2px;
	}
	.token.active {
		box-shadow:
			inset 0 0 0 1px rgba(0, 0, 0, 0.75),
			inset 0 0 8px rgba(0, 0, 0, 0.5),
			0 0 0 2px var(--accent),
			0 0 0 3px #000,
			0 0 12px 1px rgba(201, 164, 92, 0.5);
	}
	.token.down {
		filter: grayscale(1) brightness(0.5);
	}
	.count {
		position: absolute;
		right: -7px;
		bottom: -6px;
		padding: 0 4px;
		font: 600 11px/16px var(--body);
		text-shadow: none;
		color: var(--text);
		background: #0d0b09;
		border: 1px solid var(--border-strong);
	}
</style>
