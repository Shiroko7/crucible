<script lang="ts">
	// The library: every scenario on disk as a card showing who fights whom.
	import { onMount } from 'svelte';
	import Token from '../lib/Token.svelte';
	import { capitalised, sideName } from '../lib/names';
	import { loadLibrary } from '../lib/store.svelte';
	import type { Member, ScenarioInfo } from '../lib/types';

	let library = $state<ScenarioInfo[] | null>(null);
	let failure = $state('');
	let search = $state('');

	const wanted = $derived(search.trim().toLowerCase());
	const shown = $derived(
		(library ?? []).filter(
			(s) =>
				!wanted ||
				s.name.includes(wanted) ||
				s.sides.some((side) => side.some((m) => m.name.toLowerCase().includes(wanted)))
		)
	);
	const groups = $derived(
		[...new Set(shown.map((s) => s.group))]
			// Each collection by name, then the examples the repo comes with.
			.sort((a, b) => Number(a === 'scenarios') - Number(b === 'scenarios') || a.localeCompare(b))
			.map((group) => ({ group, scenarios: shown.filter((s) => s.group === group) }))
	);

	function heading(group: string): string {
		return group === 'scenarios' ? 'Examples' : capitalised(group.replaceAll('/', ' / '));
	}

	function headcount(members: Member[]): number {
		return members.reduce((sum, m) => sum + m.count, 0);
	}

	onMount(async () => {
		try {
			library = await loadLibrary();
		} catch (e) {
			failure = e instanceof Error ? e.message : String(e);
		}
	});
</script>

<svelte:head><title>Crucible</title></svelte:head>

<main>
	<div class="intro">
		<div>
			<h1>Library</h1>
			<p>Pick a fight to see how it goes under each way of playing it, then watch it happen.</p>
		</div>
		<div class="tools">
			<input
				type="search"
				placeholder="Search scenarios or creatures"
				bind:value={search}
				aria-label="Search"
			/>
			<a class="button primary" href="/build" data-test="new-scenario">New scenario</a>
		</div>
	</div>

	{#if failure}
		<div class="notice bad" data-test="failure">
			<b>The server is not answering.</b>
			<span>{failure}</span>
		</div>
	{:else if !library}
		<div class="grid">
			{#each Array(6) as _, i (i)}
				<div class="card ghost"></div>
			{/each}
		</div>
	{:else if library.length === 0}
		<div class="notice">
			<b>No scenarios yet.</b>
			<span><a class="link" href="/build">Build one</a> out of the creatures in the roster.</span>
		</div>
	{:else if shown.length === 0}
		<div class="notice"><b>Nothing matches “{search}”.</b></div>
	{:else}
		{#each groups as { group, scenarios } (group)}
			<section data-test="group">
				<h2>
					{heading(group)}
					<span class="count">{scenarios.length}</span>
				</h2>
				<div class="grid">
					{#each scenarios as scenario (scenario.path)}
						{#if scenario.error}
							<div class="card broken" data-test="card" title={scenario.error}>
								<h3>{capitalised(scenario.name)}</h3>
								<p class="error">This file does not load: {scenario.error}</p>
								<a class="edit" href="/build?path={encodeURIComponent(scenario.path)}">Edit</a>
							</div>
						{:else}
							<div class="slot">
							<a
								class="card"
								data-test="card"
								href="/scenario?path={encodeURIComponent(scenario.path)}"
							>
								<div class="matchup">
									<div class="tokens">
										{#each scenario.sides[0] as member (member.name)}
											<Token square name={member.name} team={0} size={40} count={member.count} />
										{/each}
									</div>
									<span class="vs">vs</span>
									<div class="tokens foes">
										{#each scenario.sides[1] as member (member.name)}
											<Token square name={member.name} team={1} size={40} count={member.count} />
										{/each}
									</div>
								</div>
								<h3>{sideName(scenario.sides[0])}</h3>
								<p class="against">against {sideName(scenario.sides[1])}</p>
								<p class="meta">
									{headcount(scenario.sides[0])} in the party · {headcount(scenario.sides[1])} against
								</p>
							</a>
							<a
								class="edit"
								data-test="edit"
								href="/build?path={encodeURIComponent(scenario.path)}"
							>
								Edit
							</a>
							</div>
						{/if}
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
	.slot,
	.card.broken {
		position: relative;
	}
	.slot .card {
		height: 100%;
	}
	.edit {
		position: absolute;
		right: 12px;
		bottom: 11px;
		font-size: 11px;
		letter-spacing: 0.12em;
		text-transform: uppercase;
		color: var(--faint);
	}
	.edit:hover,
	.link {
		color: var(--accent);
	}
	input[type='search'] {
		width: min(340px, 100%);
		padding: 8px 12px;
		border-radius: 2px;
		border: 1px solid var(--border-strong);
		background: #0d0b09;
	}
	input[type='search']::placeholder {
		color: var(--faint);
	}

	section {
		margin-bottom: 38px;
	}
	h2 {
		display: flex;
		align-items: center;
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
		display: block;
		padding: 16px 18px;
		border-radius: var(--radius);
		background: var(--panel);
		border: 1px solid var(--border-strong);
		box-shadow: var(--edge);
		transition: border-color 0.15s;
	}
	a.card:hover {
		border-color: var(--accent);
	}
	.card.ghost {
		height: 168px;
		opacity: 0.5;
		animation: pulse 1.4s ease-in-out infinite;
	}
	.card.broken {
		border-style: dashed;
	}
	@keyframes pulse {
		50% {
			opacity: 0.25;
		}
	}

	.matchup {
		display: flex;
		align-items: center;
		gap: 12px;
		margin-bottom: 16px;
	}
	.tokens {
		display: flex;
		flex-wrap: wrap;
		gap: 12px;
		flex: 1;
		padding: 3px;
	}
	.tokens.foes {
		justify-content: flex-end;
	}
	.vs {
		font: italic 16px var(--display);
		color: var(--faint);
	}
	h3 {
		font-size: 17px;
		line-height: 1.25;
	}
	.against {
		margin: 2px 0 0;
		font: italic 14px var(--display);
		color: var(--foe);
	}
	.meta {
		margin: 10px 0 0;
		font-size: 11px;
		letter-spacing: 0.1em;
		text-transform: uppercase;
		color: var(--faint);
	}
	.error {
		margin: 8px 0 0;
		font-size: 13px;
		color: var(--bad);
	}

	.notice {
		display: grid;
		gap: 4px;
		padding: 28px;
		border-radius: var(--radius);
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
