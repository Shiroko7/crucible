<script lang="ts">
	// A creature as its file loads: the top of a stat block, and what it can do.
	import Token from './Token.svelte';
	import type { Card, Listed } from './types';

	let { card }: { card: Card } = $props();

	const ABILITIES = ['Str', 'Dex', 'Con', 'Int', 'Wis', 'Cha'];
	const SLOTS: { slot: Listed['slot']; label: string }[] = [
		{ slot: 'action', label: 'Actions' },
		{ slot: 'bonus', label: 'Bonus actions' },
		{ slot: 'reaction', label: 'Reactions' },
		{ slot: 'legendary', label: 'Legendary actions' },
		{ slot: 'aura', label: 'Auras' }
	];

	const signed = (n: number) => (n >= 0 ? `+${n}` : `${n}`);
	const line = $derived(
		[card.size, card.creature_type, card.kind === 'character' ? 'character' : null]
			.filter(Boolean)
			.join(' ')
	);
	const groups = $derived(
		SLOTS.map(({ slot, label }) => ({
			label,
			moves: card.moves.filter((m) => m.slot === slot)
		})).filter((g) => g.moves.length > 0)
	);
</script>

<div class="stat" data-test="stat-card">
	<div class="head">
		<Token square name={card.name} team={card.kind === 'character' ? 0 : 1} size={52} />
		<div>
			<h2 data-test="stat-name">{card.name}</h2>
			<p class="line">{line}</p>
		</div>
	</div>

	<dl class="numbers">
		<div><dt>Armor Class</dt><dd data-test="stat-ac">{card.ac}</dd></div>
		<div><dt>Hit Points</dt><dd data-test="stat-hp">{card.hp}</dd></div>
		<div><dt>Speed</dt><dd>{card.speed} ft</dd></div>
		<div><dt>Initiative</dt><dd>{signed(card.initiative)}</dd></div>
		<div>
			<dt>Space</dt>
			<dd>{card.span * 5} ft</dd>
		</div>
	</dl>

	<dl class="saves">
		{#each ABILITIES as ability, i (ability)}
			<div><dt>{ability}</dt><dd>{signed(card.saves[i] ?? 0)}</dd></div>
		{/each}
	</dl>
	<p class="caption">Saving throws</p>

	{#if card.resources.length > 0}
		<h3>Resources</h3>
		<ul>
			{#each card.resources as pool (pool.name)}
				<li>{pool.name.replaceAll('_', ' ')} <span>{pool.max}</span></li>
			{/each}
		</ul>
	{/if}

	{#each groups as group (group.label)}
		<h3>{group.label}</h3>
		<ul data-test="stat-moves">
			{#each group.moves as move, i (i)}
				<li>{move.name}</li>
			{/each}
		</ul>
	{:else}
		<p class="nothing">It has nothing it can do: give it an action.</p>
	{/each}
</div>

<style>
	.stat {
		display: grid;
		gap: 12px;
	}
	.head {
		display: flex;
		gap: 14px;
		align-items: center;
	}
	h2 {
		font-size: 22px;
		line-height: 1.15;
	}
	.line {
		margin: 2px 0 0;
		font: italic 14px var(--display);
		color: var(--muted);
		text-transform: lowercase;
	}
	dl {
		margin: 0;
		display: grid;
		gap: 1px;
		background: var(--border);
		border: 1px solid var(--border);
	}
	.numbers {
		grid-template-columns: repeat(5, 1fr);
	}
	.saves {
		grid-template-columns: repeat(6, 1fr);
	}
	dl div {
		padding: 7px 4px;
		background: #110f0d;
		text-align: center;
	}
	dt {
		font-size: 10px;
		letter-spacing: 0.1em;
		text-transform: uppercase;
		color: var(--faint);
	}
	dd {
		margin: 0;
		font: 600 17px var(--display);
	}
	.caption {
		margin: -8px 0 0;
		font-size: 10px;
		letter-spacing: 0.14em;
		text-transform: uppercase;
		color: var(--faint);
		text-align: center;
	}
	h3 {
		font: 600 11px var(--body);
		letter-spacing: 0.16em;
		text-transform: uppercase;
		color: var(--accent);
		padding-bottom: 4px;
		border-bottom: 1px solid var(--border);
	}
	ul {
		margin: -4px 0 0;
		padding: 0;
		list-style: none;
		display: flex;
		flex-wrap: wrap;
		gap: 4px 6px;
	}
	li {
		padding: 2px 8px;
		border: 1px solid var(--border);
		background: #110f0d;
		font-size: 13px;
	}
	li span {
		color: var(--accent);
		margin-left: 4px;
	}
	.nothing {
		margin: 0;
		color: var(--warn);
		font-size: 13px;
	}
</style>
