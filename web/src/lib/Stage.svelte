<script lang="ts">
	// One recorded fight, played like a game. The map is the whole window and
	// everything else sits over it the way a game's interface does: the turn
	// order along the top, each side's unit frames down the edges, and the
	// combat log and the rolls along the bottom.
	import { tick } from 'svelte';
	import Board from './Board.svelte';
	import Token from './Token.svelte';
	import Tray from './Tray.svelte';
	import { capitalised } from './names';
	import { CONCENTRATING, statuses } from './status';
	import type { Entry, FrameKind } from './types';

	let {
		entry,
		matchup,
		start = 0,
		still = null,
		onclose,
		onstep
	}: {
		entry: Entry;
		/** The scenario in words, for the heading. */
		matchup: string;
		/** The frame to open on. */
		start?: number;
		/** Hold the board's animation this far through each beat, 0 to 1. */
		still?: number | null;
		onclose: () => void;
		/** Move to the fight before (-1) or after (+1) this one, when there is one. */
		onstep: (by: number) => boolean;
	} = $props();

	let frame = $state(0);
	let playing = $state(false);
	let hold = $state(700);
	let logOpen = $state(false);
	// The whole map at once, rather than following the fight across it.
	let whole = $state(false);
	let log = $state<HTMLElement>();
	// How much of the window the interface covers on each edge, which is
	// what the map keeps clear of.
	let above = $state(0);
	let below = $state(0);
	let beside = $state(0);

	const replay = $derived(entry.replay);
	const last = $derived(replay.frames.length - 1);
	const beat = $derived(replay.frames[Math.min(frame, last)]);
	const before = $derived(replay.frames[frame - 1]);
	const sides = $derived(
		[0, 1].map((team) =>
			replay.units.map((unit, seat) => ({ unit, seat })).filter(({ unit }) => unit.team === team)
		)
	);
	// Where each round starts on the timeline.
	const rounds = $derived(
		replay.frames
			.map((f, i) => ({ round: f.round, at: i }))
			.filter((mark, i, all) => i === 0 || mark.round !== all[i - 1].round)
	);
	const outcome = $derived(
		replay.winner === 0
			? { word: 'Victory', tone: 'won' }
			: replay.winner === 1
				? { word: 'Defeat', tone: 'lost' }
				: { word: 'Unresolved', tone: 'draw' }
	);
	// The last few lines of the log, the one on show last.
	const recent = $derived(
		replay.frames.slice(Math.max(0, frame - 3), frame + 1).map((line, i, all) => ({
			line,
			at: frame - (all.length - 1 - i)
		}))
	);

	// Whose turn it is: the last one to start, which is not always whoever
	// is acting - a reaction or an aura answers on somebody else's turn.
	const turn = $derived.by(() => {
		for (let i = Math.min(frame, last); i >= 0; i--) {
			const kind = replay.frames[i].kind;
			if (kind === 'turn' || kind === 'lost') return replay.frames[i].actor;
		}
		return null;
	});
	// The turn order as it stands: everyone who has rolled so far, highest
	// first, until the first turn - then the order the fight is played in.
	const order = $derived.by(() => {
		if (turn != null) return replay.order.map((seat) => ({ seat, total: null as number | null }));
		const rolled: { seat: number; total: number | null }[] = [];
		for (const f of replay.frames.slice(0, frame + 1)) {
			if (f.detail?.type === 'initiative' && f.actor != null) {
				rolled.push({ seat: f.actor, total: f.detail.total });
			}
		}
		// The engine's own order settles ties.
		const place = (seat: number) => replay.order.indexOf(seat);
		return rolled.sort((a, b) => place(a.seat) - place(b.seat));
	});

	// How long a beat is held, against the speed: a roll is read, a turn
	// changing hands is glanced at.
	const PACE: Partial<Record<FrameKind, number>> = {
		turn: 0.55,
		use: 0.8,
		condition: 0.75,
		down: 0.8,
		move: 0.8,
		attack: 1.25,
		save: 1.25,
		end: 1.5
	};

	// A new fight starts from its first frame, unless the address says where.
	$effect(() => {
		frame = Math.max(0, Math.min(start, entry.replay.frames.length - 1));
		playing = false;
	});

	// Playing: hold each frame, then step, and stop at the end.
	$effect(() => {
		if (!playing) return;
		const at = frame;
		const timer = setTimeout(() => {
			if (at >= last) playing = false;
			else frame = at + 1;
		}, hold * (PACE[replay.frames[at]?.kind] ?? 1));
		return () => clearTimeout(timer);
	});

	// The log follows the frame on show.
	$effect(() => {
		void frame;
		if (!logOpen) return;
		tick().then(() => log?.querySelector('.now')?.scrollIntoView({ block: 'nearest' }));
	});

	function seek(to: number) {
		frame = Math.max(0, Math.min(to, last));
	}

	function toggle() {
		if (!playing && frame >= last) frame = 0;
		playing = !playing;
	}

	function share(hp: number, max: number): number {
		return max > 0 ? Math.max(0, Math.min(1, hp / max)) * 100 : 0;
	}

	function keys(event: KeyboardEvent) {
		if ((event.target as HTMLElement).matches('input, select')) return;
		const actions: Record<string, () => void> = {
			ArrowRight: () => seek(frame + 1),
			ArrowLeft: () => seek(frame - 1),
			ArrowDown: () => onstep(1),
			ArrowUp: () => onstep(-1),
			Home: () => seek(0),
			End: () => seek(last),
			Escape: onclose,
			m: () => (whole = !whole),
			' ': toggle
		};
		const action = actions[event.key];
		if (action) {
			event.preventDefault();
			action();
		}
	}
</script>

<svelte:window onkeydown={keys} />

<div class="stage">
	<div class="field">
		<Board {replay} {frame} {hold} {still} pad={[above, beside, below, beside]} {whole} />
	</div>

	<header bind:clientHeight={above}>
		<div class="top">
			<button class="button" onclick={onclose} data-test="back">
				<svg viewBox="0 0 16 16"><path d="M10 3 5 8l5 5" /></svg>
				Prediction
			</button>
			<h1 data-test="fight-title">
				<span class="matchup">{matchup}</span>
				<span class="which">{capitalised(entry.policy)} · {entry.pick} fight</span>
				<span class="result {outcome.tone}">{outcome.word} in round {replay.rounds}</span>
			</h1>
			<div class="hop">
				<button class="button" onclick={() => onstep(-1)} title="Previous fight (Up)">
					<svg viewBox="0 0 16 16"><path d="m3 10 5-5 5 5" /></svg>
				</button>
				<button class="button" onclick={() => onstep(1)} title="Next fight (Down)">
					<svg viewBox="0 0 16 16"><path d="m3 6 5 5 5-5" /></svg>
				</button>
			</div>
		</div>

		<div class="tracker">
			<div class="round panel" data-test="round">
				<span class="eyebrow">Round</span>
				<b>{beat.round}</b>
			</div>
			<div class="order panel" data-test="order">
				{#each order as { seat, total } (seat)}
					{@const unit = replay.units[seat]}
					{@const state = beat.units[seat]}
					{#if unit && state}
						<span class="place" class:mine={turn === seat} class:dead={!state.alive}>
							<Token
								square
								name={unit.name}
								team={unit.team}
								size={turn === seat ? 42 : 32}
								down={!state.alive}
								active={turn === seat}
							/>
							<span class="life"><i style:width="{share(state.hp, unit.max_hp)}%"></i></span>
							{#if total != null}<small>{total}</small>{/if}
						</span>
					{/if}
				{:else}
					<span class="waiting">Roll for initiative</span>
				{/each}
			</div>
			<div class="whose panel">
				{#if turn != null && replay.units[turn]}
					<span class="eyebrow">Acting</span>
					<b class:foe={replay.units[turn].team === 1} data-test="turn">
						{replay.units[turn].name}'s turn
					</b>
				{:else}
					<span class="eyebrow">Initiative</span>
					<b>{order.length} / {replay.units.length}</b>
				{/if}
			</div>
		</div>
	</header>

	{#each sides as side, team (team)}
		<aside class="side" class:foes={team === 1} data-test="side-{team}" bind:clientWidth={beside}>
			<span class="eyebrow">{team === 0 ? 'Party' : 'Enemies'}</span>
			{#each side as { unit, seat } (seat)}
				{@const state = beat.units[seat]}
				{#if state}
					{@const change = state.hp - (before?.units[seat]?.hp ?? state.hp)}
					{@const full = share(state.hp, unit.max_hp)}
					<div
						class="frame panel"
						class:down={!state.alive}
						class:acting={turn === seat}
						class:targeted={beat.target === seat && beat.kind !== 'move'}
						data-test="plate"
					>
						<Token square name={unit.name} team={unit.team} size={36} down={!state.alive} />
						<div class="vitals">
							<div class="who">
								<span class="name">{unit.name}</span>
								{#if change}
									<b class={change < 0 ? 'lost' : 'won'}>{change > 0 ? '+' : ''}{change}</b>
								{/if}
								<span
									class="ac"
									class:raised={state.ac > unit.ac}
									class:lowered={state.ac < unit.ac}
									title="Armor Class{state.ac !== unit.ac ? ` (${unit.ac} normally)` : ''}"
									data-test="ac"
								>
									AC <b>{state.ac}</b>
								</span>
							</div>
							<div class="health" class:mid={full <= 50} class:low={full <= 25}>
								<i class="was" style:width="{full}%"></i>
								<i class="is" style:width="{full}%"></i>
								{#if state.temp_hp > 0}
									<i class="temp" style:width="{share(state.temp_hp, unit.max_hp)}%"></i>
								{/if}
								<span>
									{state.hp} / {unit.max_hp}
									{#if state.temp_hp > 0}
										<em title="Temporary hit points" data-test="temp">+{state.temp_hp}</em>
									{/if}
								</span>
							</div>
							{#if !state.alive}
								<div class="effects"><span class="effect gone">Down</span></div>
							{:else if unit.summon || state.concentrating || state.conditions.length}
								<div class="effects">
									{#if unit.summon}<span class="effect">summoned</span>{/if}
									{#if state.concentrating}
										<span
											class="effect"
											style:--tone={CONCENTRATING.colour}
											data-test="concentrating"
										>
											concentrating · {state.concentrating}
										</span>
									{/if}
									{#each statuses(state.conditions) as mark (mark.label)}
										<span class="effect" style:--tone={mark.colour} data-test="condition">
											{mark.label}
										</span>
									{/each}
								</div>
							{/if}
						</div>
					</div>
				{/if}
			{/each}
		</aside>
	{/each}

	{#if beat.kind === 'end'}
		<div class="verdict {outcome.tone}" style:top="{above + 4}px">
			<span>{outcome.word}</span>
			<small>Round {replay.rounds}</small>
		</div>
	{/if}

	<footer bind:clientHeight={below}>
		<div class="dock">
			<div class="said panel">
				{#each recent as { line, at } (at)}
					{#if at === frame}
						<p class="now {line.kind}" data-test="narration">{line.text}</p>
					{:else}
						<p class={line.kind}>{line.text}</p>
					{/if}
				{/each}
			</div>
			<Tray detail={beat.detail} at={frame} />
		</div>

		<div class="transport">
			<div class="buttons">
				<button class="icon" onclick={() => seek(0)} title="Start (Home)" aria-label="Start">
					<svg viewBox="0 0 16 16"><path d="M4 3v10M13 3 6 8l7 5z" /></svg>
				</button>
				<button class="icon" onclick={() => seek(frame - 1)} title="Back (Left)" aria-label="Back">
					<svg viewBox="0 0 16 16"><path d="M11 3 5 8l6 5z" /></svg>
				</button>
				<button class="icon play" onclick={toggle} title="Play (Space)" data-test="play">
					{#if playing}
						<svg viewBox="0 0 16 16"><path d="M5 3v10M11 3v10" /></svg>
					{:else}
						<svg viewBox="0 0 16 16"><path d="m5 3 8 5-8 5z" /></svg>
					{/if}
					<span class="sr">{playing ? 'Pause' : 'Play'}</span>
				</button>
				<button
					class="icon"
					onclick={() => seek(frame + 1)}
					title="Forward (Right)"
					aria-label="Forward"
				>
					<svg viewBox="0 0 16 16"><path d="m5 3 6 5-6 5z" /></svg>
				</button>
				<button class="icon" onclick={() => seek(last)} title="End (End)" aria-label="End">
					<svg viewBox="0 0 16 16"><path d="M12 3v10M3 3l7 5-7 5z" /></svg>
				</button>
			</div>

			<div class="timeline">
				<!-- It lets go of the keyboard once used, so the arrow keys go back to stepping. -->
				<input
					type="range"
					min="0"
					max={last}
					bind:value={frame}
					onchange={(e) => e.currentTarget.blur()}
					style:--done="{last ? (frame / last) * 100 : 0}%"
					aria-label="Timeline"
				/>
				<div class="marks">
					{#each rounds as mark (mark.round)}
						<button
							style:left="{last ? (mark.at / last) * 100 : 0}%"
							class:past={mark.at <= frame}
							onclick={() => seek(mark.at)}
							title="Round {mark.round}"
						>
							{mark.round}
						</button>
					{/each}
				</div>
			</div>

			<span class="counter" data-test="counter">{frame + 1} / {last + 1}</span>
			<select bind:value={hold} title="Speed" onchange={(e) => e.currentTarget.blur()}>
				<option value={1400}>0.5×</option>
				<option value={700}>1×</option>
				<option value={350}>2×</option>
				<option value={140}>5×</option>
			</select>
			<button
				class="button"
				class:on={whole}
				onclick={() => (whole = !whole)}
				title="Show the whole map, or follow the fight across it (M)"
				data-test="whole"
			>
				Whole map
			</button>
			<button class="button" class:on={logOpen} onclick={() => (logOpen = !logOpen)} data-test="log">
				Combat log
			</button>
		</div>
	</footer>

	{#if logOpen}
		<aside class="log" bind:this={log} data-test="log-panel">
			<div class="log-head">
				<h2>Combat log</h2>
				<button class="button" onclick={() => (logOpen = false)}>Close</button>
			</div>
			{#each replay.frames as line, i (i)}
				<button
					class="line {line.kind}"
					class:now={i === frame}
					class:later={i > frame}
					onclick={() => seek(i)}
				>
					<span class="r">R{line.round}</span>
					<span>{line.text}</span>
				</button>
			{/each}
		</aside>
	{/if}
</div>

<style>
	.stage {
		position: fixed;
		inset: 0;
		/* Over the app's own bar: a fight takes the whole window. */
		z-index: 10;
		display: grid;
		grid-template-columns: var(--side) minmax(0, 1fr) var(--side);
		grid-template-rows: auto minmax(0, 1fr) auto;
		--side: clamp(190px, 19vw, 270px);
		background: #0b0a09;
		overflow: hidden;
	}
	/* The map lies under all of it. */
	.field {
		position: absolute;
		inset: 0;
	}

	svg {
		width: 14px;
		height: 14px;
		fill: none;
		stroke: currentColor;
		stroke-width: 1.8;
		stroke-linecap: round;
		stroke-linejoin: round;
	}

	header {
		grid-column: 1 / -1;
		position: relative;
		background: linear-gradient(180deg, rgba(8, 7, 6, 0.92) 55%, transparent);
		padding-bottom: 10px;
	}
	.top {
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 8px 12px;
	}
	h1 {
		flex: 1;
		min-width: 0;
		display: flex;
		align-items: baseline;
		gap: 14px;
		font-size: 17px;
		line-height: 1.2;
		white-space: nowrap;
		overflow: hidden;
	}
	.matchup {
		color: var(--text);
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.which,
	.result {
		font: 11px var(--body);
		letter-spacing: 0.16em;
		text-transform: uppercase;
		color: var(--muted);
	}
	.which {
		padding-left: 14px;
		border-left: 1px solid var(--border-strong);
	}
	.result.won {
		color: var(--good);
	}
	.result.lost {
		color: var(--bad);
	}
	.result.draw {
		color: var(--warn);
	}
	.hop {
		display: flex;
		gap: 4px;
	}
	.hop .button,
	.top > .button {
		padding: 5px 9px;
	}

	.tracker {
		display: flex;
		justify-content: center;
		align-items: stretch;
		gap: 6px;
		padding: 0 12px;
	}
	.round,
	.whose {
		display: grid;
		align-content: center;
		justify-items: center;
		padding: 2px 14px;
		line-height: 1.15;
		white-space: nowrap;
	}
	.round b {
		font: 600 24px/1 var(--display);
		color: var(--accent);
	}
	.whose {
		min-width: 130px;
	}
	.whose b {
		font: 600 15px var(--display);
		color: var(--party);
	}
	.whose b.foe {
		color: var(--foe);
	}
	.order {
		display: flex;
		align-items: flex-end;
		gap: 9px;
		min-height: 64px;
		padding: 8px 12px 7px;
		overflow-x: auto;
	}
	.waiting {
		align-self: center;
		padding: 0 18px;
		font: italic 14px var(--display);
		color: var(--muted);
	}
	.place {
		position: relative;
		display: grid;
		justify-items: center;
		gap: 5px;
		animation: join 0.3s ease-out;
	}
	.place.dead {
		opacity: 0.45;
	}
	/* Each one's health under its portrait. */
	.life {
		width: 100%;
		height: 3px;
		background: #000;
	}
	.life i {
		display: block;
		height: 100%;
		background: var(--good);
		transition: width 0.3s;
	}
	.place small {
		position: absolute;
		top: -7px;
		right: -7px;
		min-width: 17px;
		padding: 0 3px;
		font: 700 10px/15px var(--body);
		text-align: center;
		color: var(--accent);
		background: #0d0b09;
		border: 1px solid var(--border-strong);
	}
	@keyframes join {
		from {
			opacity: 0;
			transform: translateY(-10px);
		}
	}

	.side {
		grid-column: 1;
		grid-row: 2;
		position: relative;
		display: flex;
		flex-direction: column;
		gap: 5px;
		padding: 4px 4px 8px 12px;
		overflow-y: auto;
		scrollbar-width: thin;
	}
	.side.foes {
		grid-column: 3;
		padding: 4px 12px 8px 4px;
	}
	.side > .eyebrow {
		text-shadow: 0 1px 2px #000;
	}
	.side.foes > .eyebrow {
		text-align: right;
	}

	.frame {
		display: flex;
		gap: 10px;
		align-items: flex-start;
		padding: 7px 8px 6px 9px;
		border-left: 2px solid var(--party);
		transition:
			border-color 0.2s,
			opacity 0.2s;
	}
	.foes .frame {
		border-left-color: var(--foe);
	}
	.frame.acting {
		border-color: var(--accent);
		box-shadow:
			var(--edge),
			0 0 14px rgba(201, 164, 92, 0.22);
	}
	.frame.targeted {
		border-top-color: var(--foe);
		border-right-color: var(--foe);
		border-bottom-color: var(--foe);
	}
	.frame.down {
		opacity: 0.5;
	}
	.frame :global(.token) {
		margin: 3px;
	}
	.vitals {
		flex: 1;
		min-width: 0;
	}
	.who {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		gap: 8px;
	}
	.name {
		font: 600 14px/1.25 var(--display);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.who > b {
		margin-left: auto;
		font-size: 12px;
		font-variant-numeric: tabular-nums;
	}
	.name {
		flex: 1;
	}
	.ac {
		flex: none;
		padding: 0 5px;
		font-size: 10px;
		letter-spacing: 0.08em;
		color: var(--muted);
		background: #0d0b09;
		border: 1px solid var(--border-strong);
	}
	.ac b {
		font-size: 12px;
		color: var(--text);
	}
	.ac.raised b {
		color: #7fd0e8;
	}
	.ac.lowered b {
		color: #e08a7c;
	}
	.health {
		position: relative;
		height: 15px;
		margin-top: 3px;
		background: #0a0807;
		border: 1px solid #000;
		box-shadow: 0 0 0 1px rgba(255, 236, 190, 0.07);
		--fill: #4f8a45;
	}
	.health.mid {
		--fill: #a8842c;
	}
	.health.low {
		--fill: #a23a30;
	}
	.health i {
		position: absolute;
		inset: 0 auto 0 0;
	}
	/* What was just lost stays lit for a moment, then drains. */
	.health .was {
		background: #e8dcc0;
		opacity: 0.55;
		transition: width 0.5s 0.35s ease-in;
	}
	.health .is {
		background: linear-gradient(180deg, color-mix(in srgb, var(--fill) 80%, #fff), var(--fill) 45%, color-mix(in srgb, var(--fill) 70%, #000));
		transition: width 0.15s;
	}
	.health .temp {
		height: 4px;
		bottom: auto;
		background: #7fd0e8;
	}
	.health em {
		font-style: normal;
		color: #9fe2f5;
	}
	.health span {
		position: absolute;
		inset: 0;
		font: 600 10.5px/13px var(--body);
		text-align: center;
		font-variant-numeric: tabular-nums;
		color: #f4eede;
		text-shadow:
			0 1px 1px #000,
			0 0 3px #000;
	}
	.effects {
		display: flex;
		flex-wrap: wrap;
		gap: 2px 10px;
		margin-top: 4px;
	}
	.effect {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		font-size: 11px;
		line-height: 1.3;
		color: var(--tone, var(--muted));
	}
	.effect::before {
		content: '';
		flex: none;
		width: 6px;
		height: 6px;
		background: currentColor;
		transform: rotate(45deg);
	}
	.effect.gone {
		letter-spacing: 0.14em;
		text-transform: uppercase;
		color: var(--bad);
	}

	/* How it ended, across the map. */
	.verdict {
		position: absolute;
		left: 0;
		right: 0;
		display: grid;
		justify-items: center;
		padding: 10px 0 9px;
		pointer-events: none;
		background: linear-gradient(90deg, transparent, rgba(0, 0, 0, 0.82) 30%, rgba(0, 0, 0, 0.82) 70%, transparent);
		animation: fall 0.6s ease-out;
	}
	.verdict span {
		font: 600 38px/1.1 var(--display);
		letter-spacing: 0.3em;
		text-indent: 0.3em;
		text-transform: uppercase;
	}
	.verdict small {
		font-size: 11px;
		letter-spacing: 0.3em;
		text-transform: uppercase;
		color: var(--muted);
	}
	@keyframes fall {
		from {
			opacity: 0;
			letter-spacing: 0.2em;
		}
	}

	footer {
		grid-column: 1 / -1;
		grid-row: 3;
		position: relative;
		display: grid;
		gap: 6px;
		padding-top: 12px;
		background: linear-gradient(0deg, rgba(8, 7, 6, 0.94) 60%, transparent);
	}
	.dock {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(280px, 380px);
		gap: 6px;
		height: 122px;
		/* Between the two sides' frames, under the map. */
		margin: 0 calc(var(--side) + 4px);
	}
	.said {
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		gap: 2px;
		padding: 8px 14px 9px;
		overflow: hidden;
	}
	.said p {
		margin: 0;
		font-size: 12.5px;
		line-height: 1.35;
		color: var(--faint);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		flex: none;
	}
	.said p.now {
		font: 16px/1.35 var(--display);
		color: var(--text);
		white-space: normal;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		margin-top: 3px;
		padding-top: 5px;
		border-top: 1px solid var(--border);
	}
	/* The log is coloured by what each line is, as a game's is. */
	.said p.now.turn,
	.said p.now.start,
	.said p.now.end,
	.line.turn:not(.later) {
		color: var(--accent);
	}
	.said p.now.damage,
	.said p.now.down,
	.line.damage:not(.later),
	.line.down:not(.later) {
		color: #e08a7c;
	}
	.said p.now.heal,
	.line.heal:not(.later) {
		color: #8fc783;
	}
	.said p.now.use,
	.line.use:not(.later) {
		color: #c9b8f0;
	}

	.transport {
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 0 12px 8px;
	}
	.buttons {
		display: flex;
		align-items: center;
		gap: 3px;
	}
	.icon {
		display: grid;
		place-items: center;
		width: 28px;
		height: 28px;
		border: 1px solid var(--border);
		border-radius: 2px;
		background: #14120f;
		color: var(--muted);
		transition:
			border-color 0.15s,
			color 0.15s;
	}
	.icon:hover {
		border-color: var(--accent);
		color: var(--text);
	}
	.icon.play {
		width: 40px;
		color: var(--accent);
		border-color: var(--border-strong);
	}
	.icon.play svg {
		fill: currentColor;
	}
	.sr {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip-path: inset(50%);
	}

	.timeline {
		flex: 1;
		min-width: 0;
		padding-top: 9px;
	}
	input[type='range'] {
		appearance: none;
		display: block;
		width: 100%;
		height: 4px;
		margin: 0;
		background: linear-gradient(90deg, var(--accent) var(--done), #2a251e var(--done));
		box-shadow: 0 0 0 1px #000;
		cursor: pointer;
	}
	input[type='range']::-webkit-slider-thumb {
		appearance: none;
		width: 9px;
		height: 15px;
		background: #e6dcc6;
		border: 1px solid #000;
	}
	input[type='range']::-moz-range-thumb {
		width: 7px;
		height: 13px;
		border-radius: 0;
		background: #e6dcc6;
		border: 1px solid #000;
	}
	.marks {
		position: relative;
		height: 15px;
		margin: 3px 4px 0;
	}
	.marks button {
		position: absolute;
		transform: translateX(-50%);
		font-size: 10px;
		color: var(--faint);
	}
	.marks button.past {
		color: var(--muted);
	}
	.marks button:hover {
		color: var(--accent);
	}
	.counter {
		font-size: 12px;
		color: var(--muted);
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}
	select {
		padding: 5px 8px;
		font-size: 13px;
		border-radius: 2px;
		border: 1px solid var(--border-strong);
		background: #14120f;
	}
	.button.on {
		border-color: var(--accent);
		color: var(--accent);
	}

	.log {
		position: absolute;
		top: 0;
		right: 0;
		bottom: 0;
		width: min(440px, 92vw);
		overflow-y: auto;
		background: #100e0c;
		border-left: 1px solid var(--border-strong);
		box-shadow: -20px 0 40px rgba(0, 0, 0, 0.6);
	}
	.log-head {
		position: sticky;
		top: 0;
		display: flex;
		justify-content: space-between;
		align-items: center;
		padding: 10px 14px;
		background: #100e0c;
		border-bottom: 1px solid var(--border-strong);
	}
	.log-head h2 {
		font-size: 16px;
	}
	.line {
		display: flex;
		gap: 10px;
		width: 100%;
		padding: 4px 14px;
		text-align: left;
		font-size: 13px;
		border-left: 2px solid transparent;
	}
	.line:hover {
		background: var(--surface-2);
	}
	.line.now {
		background: var(--accent-soft);
		border-left-color: var(--accent);
	}
	.line.later {
		color: var(--faint);
	}
	.r {
		flex: none;
		width: 28px;
		color: var(--faint);
		font-variant-numeric: tabular-nums;
	}

	@media (max-width: 900px) {
		.stage {
			--side: 150px;
		}
		.which,
		.round,
		.whose,
		.counter,
		.effects {
			display: none;
		}
		.dock {
			grid-template-columns: 1fr;
			height: auto;
			margin: 0 8px;
		}
		.said {
			height: 74px;
		}
	}
</style>
