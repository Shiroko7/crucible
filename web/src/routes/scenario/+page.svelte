<script lang="ts">
	// One scenario: how it goes under each way of playing it, and the worst,
	// median and best fight of each, which open on the stage.
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import Stage from '../../lib/Stage.svelte';
	import Token from '../../lib/Token.svelte';
	import { capitalised, percent, sideName } from '../../lib/names';
	import { loadBundle, loadLibrary, settings } from '../../lib/store.svelte';
	import type { Bundle, ScenarioInfo, Style } from '../../lib/types';

	const path = $derived(page.url.searchParams.get('path') ?? '');
	// Which fight is on the stage, if one is: `fight` in the address, from 1.
	const fightAt = $derived.by(() => {
		const asked = Number(page.url.searchParams.get('fight'));
		return Number.isInteger(asked) && asked >= 1 ? asked - 1 : null;
	});

	// And the frame it opens on: `frame`, from 1.
	const frameAt = $derived(Math.max(0, (Number(page.url.searchParams.get('frame')) || 1) - 1));

	// `still=0.5` holds each beat's animation half way: for a picture of one.
	const still = $derived(
		page.url.searchParams.has('still') ? Number(page.url.searchParams.get('still')) : null
	);
	let info = $state<ScenarioInfo | null>(null);
	let bundle = $state<Bundle | null>(null);
	let busy = $state(false);
	let failure = $state('');
	let took = $state(0);
	let tuning = $state(false);

	const party = $derived(info ? sideName(info.sides[0]) : '');
	const foes = $derived(info ? sideName(info.sides[1]) : '');
	const matchup = $derived(info ? `${party} vs ${foes}` : path);
	const entry = $derived(fightAt != null ? (bundle?.entries[fightAt] ?? null) : null);

	const verdict = $derived.by(() => {
		const styles = bundle?.styles ?? [];
		if (styles.length === 0) return null;
		const best = styles.reduce((a, b) => (b.win > a.win ? b : a));
		const worst = styles.reduce((a, b) => (b.win < a.win ? b : a));
		const mean = styles.reduce((sum, s) => sum + s.win, 0) / styles.length;
		const spread = `From ${percent(worst.win)} played ${worst.policy} to ${percent(best.win)} played ${best.policy}.`;
		if (worst.win >= 0.9) {
			return { word: 'Comfortable', tone: 'won', line: 'The party wins however it plays.' };
		}
		if (best.win <= 0.1) {
			return { word: 'Deadly', tone: 'lost', line: 'The party loses however it plays.' };
		}
		if (best.win - worst.win >= 0.3) return { word: 'Depends on play', tone: 'draw', line: spread };
		if (mean >= 0.6) return { word: 'Favoured', tone: 'won', line: spread };
		if (mean <= 0.4) return { word: 'Against the odds', tone: 'lost', line: spread };
		return { word: 'Coin flip', tone: 'draw', line: spread };
	});

	async function load(again = false) {
		if (!path) return;
		busy = true;
		failure = '';
		const started = performance.now();
		try {
			bundle = await loadBundle(path, again);
			took = Math.round(performance.now() - started);
		} catch (e) {
			bundle = null;
			failure = e instanceof Error ? e.message : String(e);
		} finally {
			busy = false;
		}
	}

	// A scenario in the address is looked up and run.
	$effect(() => {
		const wanted = path;
		bundle = null;
		info = null;
		loadLibrary()
			.then((library) => {
				if (wanted === path) info = library.find((s) => s.path === wanted) ?? null;
			})
			.catch(() => {});
		load();
	});

	function address(fight: number | null): string {
		const query = new URLSearchParams({ path });
		if (fight != null) query.set('fight', String(fight + 1));
		return `/scenario?${query}`;
	}

	/** Step to the fight before or after the one on stage; false at either end. */
	function step(by: number): boolean {
		const to = (fightAt ?? 0) + by;
		if (!bundle || to < 0 || to >= bundle.entries.length) return false;
		goto(address(to), { replace: true, reset: false });
		return true;
	}

	function fights(style: Style) {
		return (bundle?.entries ?? [])
			.map((entry, index) => ({ entry, index }))
			.filter(({ entry }) => entry.policy === style.policy);
	}

	function outcome(winner: number | null): { word: string; tone: string } {
		if (winner === 0) return { word: 'won', tone: 'won' };
		if (winner === 1) return { word: 'lost', tone: 'lost' };
		return { word: 'draw', tone: 'draw' };
	}
</script>

<svelte:head><title>{matchup} · Crucible</title></svelte:head>

{#if entry}
	<Stage {entry} {matchup} start={frameAt} {still} onclose={() => goto(address(null))} onstep={step} />
{:else}
	<main>
		<a class="back" href="/">
			<svg viewBox="0 0 16 16" width="14" height="14"><path d="M10 3 5 8l5 5" /></svg>
			Library
		</a>

		<section class="hero">
			<div class="banner">
				<div class="tokens">
					{#each info?.sides[0] ?? [] as member (member.name)}
						<Token square name={member.name} team={0} size={56} count={member.count} />
					{/each}
				</div>
				<span class="vs">vs</span>
				<div class="tokens">
					{#each info?.sides[1] ?? [] as member (member.name)}
						<Token square name={member.name} team={1} size={56} count={member.count} />
					{/each}
				</div>
			</div>
			<h1 data-test="matchup">
				{#if info}
					{party} <span class="against">against {foes}</span>
				{:else}
					{path}
				{/if}
			</h1>
		</section>

		{#if failure}
			<div class="notice" data-test="failure">
				<b>This scenario did not run.</b>
				<span>{failure}</span>
				<a class="button" href="/">Back to the library</a>
			</div>
		{:else if !bundle}
			<div class="notice" data-test="running">
				<span class="spinner"></span>
				<b>Fighting it {settings.samples} times a playstyle…</b>
			</div>
		{:else}
			<section class="prediction">
				<div class="verdict" data-test="verdict">
					<span class="eyebrow">Prediction</span>
					{#if verdict}
						<h2 class={verdict.tone}>{verdict.word}</h2>
						<p>{verdict.line}</p>
					{/if}
					<p class="basis" data-test="basis">
						{bundle.probed} fights a playstyle against a greedy opponent, seed {settings.seed},
						sides {settings.apart} ft apart · map {settings.across} × {settings.down} squares · {took} ms
					</p>
					<div class="tune">
						<button class="button" onclick={() => (tuning = !tuning)}>Run settings</button>
						<button class="button primary" disabled={busy} onclick={() => load(true)}>
							{busy ? 'Running…' : 'Run again'}
						</button>
					</div>
					{#if tuning}
						<div class="fields">
							<label>Seed <input type="number" min="0" bind:value={settings.seed} /></label>
							<label>
								Fights
								<input type="number" min="1" max="5000" bind:value={settings.samples} />
							</label>
							<label>
								Feet apart
								<input type="number" min="5" step="5" bind:value={settings.apart} />
							</label>
							<label title="The map, in 5-foot squares across: never less than 40">
								Map across
								<input
									type="number"
									min="40"
									max="1000"
									step="5"
									bind:value={settings.across}
									data-test="map-across"
								/>
							</label>
							<label title="The map, in 5-foot squares down: never less than 30">
								Map down
								<input
									type="number"
									min="30"
									max="1000"
									step="5"
									bind:value={settings.down}
									data-test="map-down"
								/>
							</label>
						</div>
					{/if}
				</div>

				<div class="styles">
					<div class="row head">
						<span>Playstyle</span>
						<span>Party wins</span>
						<span class="num">Rounds</span>
						<span class="num">Party down</span>
						<span>Watch a fight</span>
					</div>
					{#each bundle.styles as style (style.policy)}
						<div class="row" data-test="style">
							<div>
								<b>{capitalised(style.policy)}</b>
								<small>{style.blurb}</small>
							</div>
							<div class="odds">
								<div class="meter" title="{percent(style.win)} won, {percent(style.loss)} lost">
									<i class="w" style:width="{style.win * 100}%"></i>
									<i class="d" style:width="{style.draw * 100}%"></i>
									<i class="l" style:width="{style.loss * 100}%"></i>
								</div>
								<span class="share">{percent(style.win)}</span>
							</div>
							<span class="num">{style.rounds.toFixed(1)}</span>
							<span class="num">{style.deaths.toFixed(1)}</span>
							<div class="picks">
								{#each fights(style) as { entry: fight, index } (index)}
									{@const result = outcome(fight.replay.winner)}
									<a class="pick" href={address(index)} data-test="pick">
										<span>{capitalised(fight.pick)}</span>
										<small class={result.tone}>{result.word} · r{fight.replay.rounds}</small>
									</a>
								{/each}
							</div>
						</div>
					{/each}
				</div>
			</section>
		{/if}
	</main>
{/if}

<style>
	main {
		max-width: 1240px;
		margin: 0 auto;
		padding: 24px 28px 80px;
	}
	.back {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		color: var(--muted);
	}
	.back:hover {
		color: var(--text);
	}
	svg {
		fill: none;
		stroke: currentColor;
		stroke-width: 1.8;
		stroke-linecap: round;
		stroke-linejoin: round;
	}

	.hero {
		text-align: center;
		padding: 18px 0 30px;
	}
	.banner {
		display: flex;
		justify-content: center;
		align-items: center;
		gap: 28px;
		min-height: 72px;
		margin-bottom: 18px;
	}
	.tokens {
		display: flex;
		flex-wrap: wrap;
		justify-content: center;
		gap: 16px;
	}
	.vs {
		font: italic 26px var(--display);
		color: var(--faint);
	}
	.hero h1 {
		font-size: 30px;
		line-height: 1.2;
	}
	.against {
		display: block;
		font-size: 20px;
		font-weight: 400;
		font-style: italic;
		color: var(--foe);
	}

	.prediction {
		display: grid;
		grid-template-columns: minmax(260px, 320px) minmax(0, 1fr);
		gap: 18px;
		align-items: start;
	}
	.verdict,
	.styles {
		border-radius: var(--radius);
		background: var(--panel);
		border: 1px solid var(--border-strong);
		box-shadow: var(--edge);
	}
	.verdict {
		padding: 22px;
	}
	.verdict h2 {
		font-size: 30px;
		letter-spacing: 0.04em;
		line-height: 1.1;
		margin-top: 6px;
	}
	.verdict p {
		margin: 8px 0 0;
	}
	.basis {
		font-size: 13px;
		color: var(--muted);
	}
	.tune {
		display: flex;
		gap: 8px;
		flex-wrap: wrap;
		margin-top: 18px;
	}
	.fields {
		display: grid;
		gap: 10px;
		margin-top: 14px;
		padding-top: 14px;
		border-top: 1px solid var(--border);
	}
	.fields label {
		display: flex;
		justify-content: space-between;
		align-items: center;
		color: var(--muted);
	}
	.fields input {
		width: 110px;
		padding: 5px 9px;
		border-radius: 2px;
		border: 1px solid var(--border-strong);
		background: var(--bg);
		color: var(--text);
	}

	.styles {
		overflow: hidden;
	}
	.row {
		display: grid;
		grid-template-columns: minmax(150px, 1.3fr) minmax(140px, 1.2fr) 70px 90px minmax(260px, 2fr);
		gap: 16px;
		align-items: center;
		padding: 14px 20px;
		border-top: 1px solid var(--border);
	}
	.row.head {
		border-top: 0;
		padding: 10px 20px;
		font-size: 11px;
		letter-spacing: 0.16em;
		text-transform: uppercase;
		color: var(--muted);
		background: rgba(0, 0, 0, 0.35);
	}
	.row b {
		display: block;
		font: 600 17px var(--display);
	}
	.row small {
		color: var(--muted);
		font-size: 13px;
	}
	.num {
		text-align: right;
		font-variant-numeric: tabular-nums;
	}
	.odds {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.meter {
		flex: 1;
		display: flex;
		height: 10px;
		overflow: hidden;
		background: #000;
		border: 1px solid #000;
		box-shadow: 0 0 0 1px rgba(255, 236, 190, 0.07);
	}
	.meter .w {
		background: var(--good);
	}
	.meter .d {
		background: var(--warn);
	}
	.meter .l {
		background: var(--bad);
	}
	.share {
		width: 46px;
		text-align: right;
		font-weight: 600;
		font-variant-numeric: tabular-nums;
	}
	.picks {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 8px;
	}
	.pick {
		display: grid;
		padding: 6px 11px;
		border-radius: 2px;
		border: 1px solid var(--border-strong);
		background: linear-gradient(180deg, #2b261f, #1a1713);
		box-shadow: var(--edge);
		line-height: 1.25;
		transition: border-color 0.15s;
	}
	.pick:hover {
		border-color: var(--accent);
	}
	.pick small {
		font-size: 12px;
	}

	.notice {
		display: grid;
		justify-items: center;
		gap: 10px;
		padding: 48px 28px;
		border-radius: var(--radius);
		border: 1px dashed var(--border-strong);
		color: var(--muted);
		text-align: center;
	}
	.notice b {
		font: 600 20px var(--display);
		color: var(--text);
	}
	.spinner {
		width: 28px;
		height: 28px;
		border-radius: 50%;
		border: 2px solid var(--surface-3);
		border-top-color: var(--accent);
		animation: spin 0.8s linear infinite;
	}
	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}

	@media (max-width: 1000px) {
		.prediction {
			grid-template-columns: 1fr;
		}
		.row {
			grid-template-columns: 1fr 1fr;
		}
		.row.head {
			display: none;
		}
		.picks {
			grid-column: 1 / -1;
		}
		.num {
			text-align: left;
		}
	}
</style>
