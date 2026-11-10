<script lang="ts">
	// The roster: every creature file on disk, by the folder it is in.
	import { onMount } from 'svelte';
	import Token from '../../lib/Token.svelte';
	import * as api from '../../lib/api';
	import { byKind, isPrivate } from '../../lib/names';
	import type { CreatureInfo } from '../../lib/types';

	let roster = $state<CreatureInfo[] | null>(null);
	let failure = $state('');
	let search = $state('');

	const wanted = $derived(search.trim().toLowerCase());
	const shown = $derived(
		(roster ?? []).filter(
			(c) =>
				!wanted ||
				c.path.toLowerCase().includes(wanted) ||
				(c.card?.name.toLowerCase().includes(wanted) ?? false)
		)
	);
	const shelves = $derived(byKind(shown));

	function file(path: string): string {
		return path.split('/').at(-1) ?? path;
	}

	onMount(async () => {
		try {
			roster = await api.creatures();
		} catch (e) {
			failure = e instanceof Error ? e.message : String(e);
		}
	});
</script>

<svelte:head><title>Creatures · Crucible</title></svelte:head>

<main>
	<div class="intro">
		<div>
			<h1>Creatures</h1>
			<p>
				Every character and monster a scenario can field. Paste a stat block or a sheet to add one.
				Yours are private - kept on this machine, out of git.
			</p>
		</div>
		<div class="tools">
			<input
				class="input"
				type="search"
				placeholder="Search creatures"
				bind:value={search}
				aria-label="Search"
			/>
			<a class="button primary" href="/creature?new=import" data-test="new-creature">
				New creature
			</a>
		</div>
	</div>

	{#if failure}
		<div class="notice bad" data-test="failure">
			<b>The server is not answering.</b>
			<span>{failure}</span>
		</div>
	{:else if !roster}
		<p class="waiting">Reading the roster…</p>
	{:else if shown.length === 0}
		<div class="notice">
			<b>{roster.length === 0 ? 'No creatures yet.' : `Nothing matches “${search}”.`}</b>
		</div>
	{:else}
		{#each shelves as { label, creatures } (label)}
			<section data-test="kind">
				<h2>{label} <span class="count">{creatures.length}</span></h2>
				<div class="grid">
					{#each creatures as creature (creature.path)}
						{@const card = creature.card}
						<a
							class="card"
							class:broken={!card}
							data-test="creature"
							href="/creature?path={encodeURIComponent(creature.path)}"
						>
							{#if card}
								<Token
									square
									name={card.name}
									team={card.kind === 'character' ? 0 : 1}
									size={46}
								/>
								<div class="who">
									<h3>{card.name}</h3>
									<p class="line">
										{[card.size, card.creature_type ?? card.kind].join(' ').toLowerCase()}
									</p>
									<p class="where" title={creature.path}>
										{#if isPrivate(creature.path)}
											<span class="private" data-test="private">Private</span>
										{:else}
											<span class="example">Example</span>
										{/if}
										{creature.group}
									</p>
								</div>
								<div class="numbers">
									<span><em>AC</em>{card.ac}</span>
									<span><em>HP</em>{card.hp}</span>
								</div>
							{:else}
								<div class="who">
									<h3>{file(creature.path)}</h3>
									<p class="error">Does not load: {creature.error}</p>
								</div>
							{/if}
						</a>
					{/each}
				</div>
			</section>
		{/each}
	{/if}
</main>

<style>
	main {
		max-width: 1240px;
		margin: 0 auto;
		padding: 36px 28px 80px;
	}
	.intro {
		display: flex;
		justify-content: space-between;
		align-items: end;
		gap: 24px;
		flex-wrap: wrap;
		margin-bottom: 30px;
	}
	.intro h1 {
		font-size: 30px;
		line-height: 1.1;
		letter-spacing: 0.06em;
	}
	.intro p {
		margin: 8px 0 0;
		color: var(--muted);
	}
	.tools {
		display: flex;
		gap: 10px;
		flex-wrap: wrap;
	}
	.tools .input {
		width: 240px;
	}
	section {
		margin-bottom: 34px;
	}
	h2 {
		display: flex;
		gap: 10px;
		font: 600 12px var(--body);
		letter-spacing: 0.18em;
		text-transform: uppercase;
		color: var(--accent);
		padding-bottom: 8px;
		margin-bottom: 14px;
		border-bottom: 1px solid var(--border);
	}
	.count {
		color: var(--faint);
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(290px, 1fr));
		gap: 10px;
	}
	.card {
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 13px 16px;
		border-radius: var(--radius);
		background: var(--panel);
		border: 1px solid var(--border-strong);
		box-shadow: var(--edge);
		transition: border-color 0.15s;
	}
	.card:hover {
		border-color: var(--accent);
	}
	.card.broken {
		border-style: dashed;
	}
	.who {
		flex: 1;
		min-width: 0;
	}
	h3 {
		font-size: 17px;
		line-height: 1.2;
	}
	.line {
		margin: 1px 0 0;
		font: italic 13px var(--display);
		color: var(--muted);
	}
	.where {
		margin: 5px 0 0;
		font-size: 11px;
		color: var(--faint);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.where span {
		margin-right: 6px;
		padding: 0 5px;
		font-size: 10px;
		letter-spacing: 0.1em;
		text-transform: uppercase;
		border: 1px solid var(--border-strong);
		color: var(--muted);
	}
	.where .example {
		border-color: var(--accent);
		color: var(--accent);
	}
	.numbers {
		display: flex;
		gap: 12px;
		font: 600 17px var(--display);
	}
	.numbers em {
		font: 10px var(--body);
		letter-spacing: 0.1em;
		color: var(--faint);
		margin-right: 4px;
	}
	.error {
		margin: 4px 0 0;
		font-size: 12px;
		color: var(--bad);
	}
	.waiting {
		color: var(--muted);
	}
	.notice {
		display: grid;
		gap: 4px;
		padding: 28px;
		border: 1px dashed var(--border-strong);
		color: var(--muted);
		text-align: center;
	}
	.notice b {
		font: 600 20px var(--display);
		color: var(--text);
	}
	.notice.bad b {
		color: var(--bad);
	}
</style>
