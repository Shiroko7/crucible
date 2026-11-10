<script lang="ts">
	// The roll window: whatever was just rolled, the sum, what it had to beat,
	// and whether it did.
	import Die from './Die.svelte';
	import { capitalised } from './names';
	import type { Detail } from './types';

	let {
		detail,
		at
	}: {
		detail: Detail | null;
		/** The frame on show: a new one is a new throw, even of the same numbers. */
		at: number;
	} = $props();

	function signed(n: number): string {
		return n < 0 ? `− ${-n}` : `+ ${n}`;
	}

	/** Gold for a natural 20, red for a natural 1. */
	function natural(value: number): 'plain' | 'best' | 'worst' {
		return value === 20 ? 'best' : value === 1 ? 'worst' : 'plain';
	}
</script>

<div class="tray panel" data-test="tray">
	{#key at}
		{#if detail?.type === 'attack'}
			<span class="label">Attack roll <i>{detail.name}</i></span>
			<div class="roll">
				{#each detail.dice as die, i (i)}
					<Die
						sides={20}
						value={die}
						tone={natural(die)}
						dropped={detail.dice.length > 1 &&
							i !== detail.dice.indexOf(detail.kept)}
					/>
				{/each}
				<span class="sum">{signed(detail.modifier)} = <b>{detail.total}</b></span>
				<span class="versus">vs</span>
				<span class="bar" title="Armor class">AC <b>{detail.ac}</b></span>
			</div>
			<span class="stamp {detail.outcome}" data-test="stamp">
				{detail.outcome === 'crit' ? 'Critical hit!' : detail.outcome === 'hit' ? 'Hit' : 'Miss'}
			</span>
		{:else if detail?.type === 'save'}
			<span class="label">{capitalised(detail.ability)} saving throw</span>
			<div class="roll">
				{#each detail.dice as die, i (i)}
					<Die
						sides={20}
						value={die}
						dropped={detail.dice.length > 1 &&
							i !== detail.dice.indexOf(detail.kept)}
					/>
				{/each}
				{#if detail.dice.length > 0}
					<span class="sum">{signed(detail.modifier)} = <b>{detail.total}</b></span>
					<span class="versus">vs</span>
				{/if}
				<span class="bar" title="Difficulty class">DC <b>{detail.dc}</b></span>
			</div>
			<span class="stamp {detail.passed ? 'saved' : 'miss'}" data-test="stamp">
				{detail.how === 'legendary resistance'
					? 'Legendary Resistance'
					: detail.how === 'automatic failure'
						? 'Fails automatically'
						: detail.passed
							? 'Saved'
							: 'Failed'}
			</span>
		{:else if detail?.type === 'initiative'}
			<span class="label">Initiative</span>
			<div class="roll">
				<Die sides={20} value={detail.die} tone={natural(detail.die)} />
				<span class="sum">{signed(detail.modifier)} = <b>{detail.total}</b></span>
			</div>
		{:else if detail?.type === 'damage'}
			<span class="label">
				Damage{detail.dice.some((d) => d.maxed) ? ' · maximised' : ''}
				{#if detail.note}<i>{detail.note}</i>{/if}
			</span>
			<div class="roll">
				{#each detail.dice.slice(0, 8) as die, i (i)}
					<Die sides={die.sides} value={die.value} size={36} maxed={die.maxed} />
				{/each}
				{#if detail.dice.length > 8}
					<span class="more">+{detail.dice.length - 8}</span>
				{/if}
				<span class="sum">= <b>{detail.amount}</b></span>
			</div>
			<span class="kinds">
				{#each detail.kinds as kind (kind)}
					<i class="kind {kind}">{kind}</i>
				{/each}
			</span>
		{:else if detail?.type === 'heal'}
			<span class="label">Healing</span>
			<div class="roll"><span class="sum heal">+ <b>{detail.amount}</b> hit points</span></div>
		{:else}
			<span class="label idle">Rolls</span>
		{/if}
	{/key}
</div>

<style>
	.tray {
		display: grid;
		grid-template-rows: auto 1fr auto;
		justify-items: center;
		align-items: center;
		gap: 2px;
		padding: 7px 12px 8px;
		min-width: 0;
		overflow: hidden;
	}
	.label {
		font-size: 10.5px;
		letter-spacing: 0.16em;
		text-transform: uppercase;
		color: var(--muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		max-width: 100%;
	}
	.label i {
		font-style: normal;
		color: var(--accent);
		margin-left: 6px;
	}
	.label.idle {
		color: var(--faint);
		grid-row: 1 / -1;
	}
	.roll {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		min-width: 0;
	}
	.sum {
		font-size: 15px;
		font-variant-numeric: tabular-nums;
		color: var(--muted);
		white-space: nowrap;
	}
	.sum b {
		font: 700 24px var(--display);
		color: var(--text);
	}
	.sum.heal,
	.sum.heal b {
		color: var(--good);
	}
	.versus {
		font: italic 13px var(--display);
		color: var(--faint);
	}
	.bar {
		padding: 2px 9px;
		font-size: 11px;
		letter-spacing: 0.1em;
		color: var(--muted);
		border: 1px solid var(--border-strong);
		background: #0d0b09;
		white-space: nowrap;
	}
	.bar b {
		font: 700 19px var(--display);
		color: var(--text);
		margin-left: 3px;
	}
	.more {
		color: var(--muted);
		font-size: 12px;
	}

	/* How it went: a word between two rules. */
	.stamp {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
		font: 600 13px var(--display);
		letter-spacing: 0.22em;
		text-transform: uppercase;
		white-space: nowrap;
		animation: settle 0.25s 0.36s both ease-out;
	}
	.stamp::before,
	.stamp::after {
		content: '';
		flex: 1;
		height: 1px;
		background: linear-gradient(90deg, transparent, currentColor);
		opacity: 0.5;
	}
	.stamp::after {
		transform: scaleX(-1);
	}
	.stamp.hit,
	.stamp.saved {
		color: var(--good);
	}
	.stamp.crit {
		color: #e6c476;
		text-shadow: 0 0 10px rgba(217, 181, 102, 0.55);
	}
	.stamp.miss {
		color: var(--bad);
	}
	@keyframes settle {
		from {
			opacity: 0;
			letter-spacing: 0.5em;
		}
	}

	.kinds {
		display: flex;
		gap: 10px;
		min-height: 16px;
	}
	.kind {
		font-style: normal;
		font-size: 11px;
		letter-spacing: 0.14em;
		text-transform: uppercase;
		color: #c9c2b2;
	}
	.kind.fire {
		color: #f08a45;
	}
	.kind.cold {
		color: #8fcdf2;
	}
	.kind.lightning {
		color: #f0dc62;
	}
	.kind.thunder {
		color: #9db0f0;
	}
	.kind.radiant {
		color: #f2e0a4;
	}
	.kind.necrotic {
		color: #a98ad6;
	}
	.kind.poison {
		color: #8fca6c;
	}
	.kind.acid {
		color: #bfd94f;
	}
	.kind.psychic {
		color: #e88fc4;
	}
	.kind.force {
		color: #d68fe8;
	}
</style>
