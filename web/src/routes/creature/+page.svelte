<script lang="ts">
	// One creature. It is made by handing the model whatever there is of it -
	// a stat block, a sheet, a description - and then put right on a sheet of
	// fields, with what it loads as beside it. The file itself is there to be
	// edited too, for what the sheet has no field for.
	//
	// `new=import` starts from pasted text, `new=character` or `new=monster`
	// from a blank one, `from=path` from a copy, and `path` opens the one there.
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import CreatureForm from '../../lib/CreatureForm.svelte';
	import StatCard from '../../lib/StatCard.svelte';
	import * as api from '../../lib/api';
	import { isPrivate, slug } from '../../lib/names';
	import { forget } from '../../lib/store.svelte';
	import { CHARACTER, MONSTER } from '../../lib/templates';
	import type { Card, CreatureForm as Form } from '../../lib/types';

	const FOLDERS = ['content/local/characters', 'content/local/monsters'];

	const path = $derived(page.url.searchParams.get('path') ?? '');
	const from = $derived(page.url.searchParams.get('from') ?? '');
	const start = $derived(page.url.searchParams.get('new') ?? '');
	const fresh = $derived(!path);

	let text = $state('');
	let saved = $state('');
	let form = $state<Form | null>(null);
	let card = $state<Card | null>(null);
	let error = $state('');
	/** Comments in the file that saving from the sheet would drop. */
	let comments = $state(0);
	let view = $state<'sheet' | 'file'>('sheet');
	let usedBy = $state<string[]>([]);
	let features = $state<string[]>([]);
	let folders = $state<string[]>([]);
	let folder = $state(FOLDERS[1]);
	let file = $state('');
	let failure = $state('');
	let note = $state('');
	let busy = $state(false);
	let loaded = $state(false);

	// What the model is asked, and what it said about its own draft.
	let source = $state('');
	let kind = $state('');
	let drafting = $state(false);
	let caveats = $state<string[]>([]);
	let drafted = $state('');

	const empty = $derived(text.trim() === '');
	const named = $derived(slug(file) || slug(card?.name ?? ''));
	const target = $derived(fresh ? `${folder.replace(/^\/+|\/+$/g, '')}/${named}.toml` : path);
	const changed = $derived(text !== saved);
	const title = $derived.by(() => {
		if (!fresh) return card?.name ?? path;
		if (from) return 'Copy of a creature';
		if (start === 'character') return 'New character';
		if (start === 'monster') return 'New monster';
		return 'New creature';
	});

	/** A sheet with every list and table it has fields for, empty or not. */
	function whole(sheet: Form): Form {
		const table = sheet.table;
		for (const list of [
			'traits',
			'features',
			'actions',
			'bonus',
			'reactions',
			'legendary',
			'auras',
			'resist',
			'immune',
			'vulnerable',
			'condition_immune'
		]) {
			table[list] ??= [];
		}
		table.saves ??= {};
		table.resources ??= {};
		table.resources.slots ??= {};
		table.size ??= '';
		table.creature_type ??= '';
		return sheet;
	}

	// Each read of the draft is numbered, so a slow answer to an old one is
	// dropped rather than written over a newer one.
	let reads = 0;
	let timer: ReturnType<typeof setTimeout> | undefined;

	/** Take `next` as the file, and read it: what it loads as, and its sheet. */
	async function take(next: string) {
		text = next;
		const read = ++reads;
		try {
			const checked = await api.checkCreature(next);
			if (read !== reads) return;
			card = checked.card ?? card;
			error = checked.error ?? '';
			comments = checked.comments;
			form = checked.form ? whole(checked.form) : null;
			if (!checked.form) view = 'file';
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		}
	}

	/** The sheet changed: write it out as the file, a moment after the typing stops. */
	function sheetChanged() {
		note = '';
		clearTimeout(timer);
		const read = ++reads;
		timer = setTimeout(async () => {
			if (!form) return;
			try {
				const rendered = await api.renderCreature($state.snapshot(form) as Form);
				if (read !== reads) return;
				text = rendered.text;
				card = rendered.card ?? card;
				error = rendered.error ?? '';
				comments = 0;
			} catch (e) {
				if (read === reads) error = e instanceof Error ? e.message : String(e);
			}
		}, 250);
	}

	/** The file changed: read it, a moment after the typing stops. */
	function fileChanged() {
		note = '';
		clearTimeout(timer);
		const draft = text;
		timer = setTimeout(() => take(draft), 250);
	}

	async function load() {
		loaded = false;
		failure = '';
		note = page.url.searchParams.has('saved') ? 'Saved.' : '';
		caveats = [];
		drafted = '';
		source = '';
		form = null;
		card = null;
		error = '';
		view = 'sheet';
		file = '';
		try {
			if (path || from) {
				const sheet = await api.creature(path || from);
				usedBy = path ? sheet.used_by : [];
				// A copy of an example is somebody's own: it goes with the private ones.
				if (from) folder = FOLDERS[sheet.card?.kind === 'character' ? 0 : 1];
				saved = path ? sheet.text : '';
				await take(sheet.text);
			} else {
				usedBy = [];
				saved = '';
				folder = FOLDERS[start === 'character' ? 0 : 1];
				if (start === 'character' || start === 'monster') {
					await take(start === 'monster' ? MONSTER : CHARACTER);
				} else {
					text = '';
				}
			}
			const [roster, known] = await Promise.all([api.creatures(), api.features()]);
			folders = [...new Set([...FOLDERS, ...roster.map((c) => c.group)])];
			features = known;
		} catch (e) {
			failure = e instanceof Error ? e.message : String(e);
		}
		loaded = true;
	}

	// Read whatever the address names, again when it names something else.
	$effect(() => {
		void [path, from, start];
		load();
	});

	/** Hand the model the pasted text, or the change asked for, and take its file. */
	async function draft() {
		drafting = true;
		failure = '';
		note = '';
		clearTimeout(timer);
		try {
			const answer = await api.draftCreature({
				source,
				kind: kind || null,
				current: empty ? null : text
			});
			caveats = answer.notes;
			drafted =
				answer.attempts > 1
					? `Drafted in ${answer.attempts} tries: the first did not load, and it was sent back.`
					: 'Drafted.';
			const made = empty;
			await take(answer.text);
			if (made && card) folder = FOLDERS[card.kind === 'character' ? 0 : 1];
			source = '';
		} catch (e) {
			failure = e instanceof Error ? e.message : String(e);
		}
		drafting = false;
	}

	async function save() {
		busy = true;
		failure = '';
		note = '';
		try {
			await api.saveCreature(target, text, fresh);
			forget();
			saved = text;
			if (fresh) await goto(`/creature?path=${encodeURIComponent(target)}&saved`);
			else note = 'Saved.';
		} catch (e) {
			failure = e instanceof Error ? e.message : String(e);
		}
		busy = false;
	}

	async function remove() {
		if (!confirm(`Delete ${path}? This removes the file.`)) return;
		busy = true;
		failure = '';
		try {
			await api.removeCreature(path);
			forget();
			await goto('/creatures');
		} catch (e) {
			failure = e instanceof Error ? e.message : String(e);
		}
		busy = false;
	}

	// Ctrl+S saves, as it would in an editor.
	function keys(event: KeyboardEvent) {
		if ((event.ctrlKey || event.metaKey) && event.key === 's') {
			event.preventDefault();
			if (!error && named && !empty) save();
		}
	}
</script>

<svelte:head><title>{card?.name ?? 'Creature'} · Crucible</title></svelte:head>
<svelte:window onkeydown={keys} />

<main>
	<div class="top">
		<a class="back" href="/creatures">← Creatures</a>
		<h1 data-test="creature-title">{title}</h1>
		<div class="actions">
			{#if note}<span class="note" data-test="saved">{note}</span>{/if}
			{#if !fresh}
				<a class="button" href="/creature?from={encodeURIComponent(path)}">Duplicate</a>
				<button class="button danger" onclick={remove} disabled={busy}>Delete</button>
			{/if}
			{#if !empty}
				<button
					class="button primary"
					data-test="save"
					onclick={save}
					disabled={busy || drafting || !!error || !named || (!fresh && !changed)}
				>
					{fresh ? 'Create' : 'Save'}
				</button>
			{/if}
		</div>
	</div>
	{#if !fresh}<p class="path" data-test="creature-path">{path}</p>{/if}

	{#if failure}
		<p class="problem" data-test="failure">{failure}</p>
	{/if}

	<section class="panel ask" class:first={empty}>
		{#if empty}
			<h2>Paste it, in whatever shape you have it</h2>
			<p class="hint">
				A stat block copied out of a book or a PDF, a character sheet, a wiki page, or a few lines
				describing it. It is read once and written out as a creature you can check and change.
			</p>
		{:else}
			<h2>Change it by saying how</h2>
		{/if}
		<textarea
			class="input said"
			bind:value={source}
			rows={empty ? 12 : 2}
			disabled={drafting}
			data-test="ask"
			placeholder={empty
				? 'Goblin Boss\nSmall Humanoid (Goblinoid), Chaotic Neutral\nAC 17   HP 21 (6d6)   Speed 30 ft.\n...'
				: 'Give it a longbow: +6 to hit, 1d8+3 piercing. Or paste a fuller stat block to fill it in.'}
		></textarea>
		<div class="askbar">
			{#if empty}
				<label class="which">
					It is
					<select class="input" bind:value={kind} disabled={drafting}>
						<option value="">whichever it reads as</option>
						<option value="character">a player character</option>
						<option value="monster">a monster</option>
					</select>
				</label>
			{/if}
			<button
				class="button primary"
				onclick={draft}
				disabled={drafting || !source.trim()}
				data-test="draft"
			>
				{drafting ? 'Reading it…' : empty ? 'Make the creature' : 'Change it'}
			</button>
			{#if drafting}
				<span class="hint" data-test="drafting">This takes up to a minute.</span>
			{:else if empty}
				<span class="hint">
					Or start from a blank
					<a class="link" href="/creature?new=character">character</a> or
					<a class="link" href="/creature?new=monster">monster</a>.
				</span>
			{/if}
		</div>
	</section>

	{#if drafted}
		<div class="panel report" data-test="drafted">
			<p>
				<b>{drafted}</b> Check it against where it came from before trusting what it predicts: the numbers
				are copied, not verified.
			</p>
			{#if caveats.length > 0}
				<p class="caption">What it says it left out or guessed</p>
				<ul>
					{#each caveats as caveat, i (i)}<li data-test="caveat">{caveat}</li>{/each}
				</ul>
			{/if}
		</div>
	{/if}

	{#if !empty}
		<div class="columns">
			<section class="panel editor">
				<div class="tabs">
					<button
						class:on={view === 'sheet'}
						onclick={() => (view = 'sheet')}
						disabled={!form}
						data-test="tab-sheet"
					>
						Sheet
					</button>
					<button class:on={view === 'file'} onclick={() => (view = 'file')} data-test="tab-file">
						File
					</button>
					<span class="hint grow">
						{view === 'sheet'
							? 'The creature, field by field.'
							: 'The same creature as the file it is saved as, for what the sheet has no field for.'}
					</span>
				</div>

				{#if fresh}
					<div class="where">
						<label class="label">
							Folder
							<input class="input" list="folders" bind:value={folder} data-test="folder" />
							<datalist id="folders">
								{#each folders as known (known)}<option value={known}></option>{/each}
							</datalist>
						</label>
						<label class="label">
							File name
							<input
								class="input"
								bind:value={file}
								placeholder={slug(card?.name ?? '') || 'from its name'}
								data-test="file"
							/>
						</label>
					</div>
					<p class="hint">
						Saved as <code data-test="target">{target}</code>.
						{#if isPrivate(target)}
							Private: anything under a <code>local</code> folder stays out of git.
						{:else}
							<b class="warned">Not private: this one would be committed with the repo.</b>
						{/if}
					</p>
				{/if}

				{#if view === 'sheet' && form}
					{#if comments > 0}
						<p class="warning" data-test="comments">
							This file has {comments}
							{comments === 1 ? 'comment' : 'comments'} written inside it. Changing anything on the sheet
							rewrites the file without {comments === 1 ? 'it' : 'them'}; the note it opens with is
							kept. To keep them, make the change under File instead.
						</p>
					{/if}
					<CreatureForm bind:form {features} onchange={sheetChanged} />
				{:else}
					{#if !form}
						<p class="warning">
							The file has a mistake the sheet cannot read past. Put it right here and the sheet comes
							back.
						</p>
					{/if}
					<textarea
						class="input source"
						bind:value={text}
						oninput={fileChanged}
						spellcheck="false"
						data-test="source"
						aria-label="The creature's file"
					></textarea>
				{/if}
			</section>

			<aside class="panel preview">
				{#if error}
					<p class="problem" data-test="problem">
						<b>This does not load.</b>
						{error}
					</p>
				{:else if card}
					<p class="fine" data-test="loads">Loads.</p>
				{/if}
				{#if card}
					<div class:stale={!!error}><StatCard {card} /></div>
				{/if}
				{#if usedBy.length > 0}
					<h3>Fielded in</h3>
					<ul>
						{#each usedBy as scenario (scenario)}
							<li><a href="/build?path={encodeURIComponent(scenario)}">{scenario}</a></li>
						{/each}
					</ul>
				{/if}
			</aside>
		</div>
	{/if}
</main>

<style>
	main {
		max-width: 1320px;
		margin: 0 auto;
		padding: 24px 28px 60px;
	}
	.top {
		display: flex;
		align-items: center;
		gap: 16px;
		flex-wrap: wrap;
		margin-bottom: 4px;
	}
	.back {
		color: var(--muted);
		font-size: 13px;
	}
	.back:hover {
		color: var(--accent);
	}
	h1 {
		flex: 1;
		font-size: 24px;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.path {
		margin: 0 0 0 0;
		font: 12px Consolas, monospace;
		color: var(--faint);
	}
	.actions {
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 32px;
	}
	.note {
		color: var(--good);
		font-size: 13px;
	}
	.panel {
		padding: 16px;
	}
	.ask {
		display: grid;
		gap: 10px;
		margin: 14px 0;
	}
	.ask.first {
		max-width: 860px;
		margin: 28px auto;
		padding: 24px;
	}
	h2 {
		font: 600 12px var(--body);
		letter-spacing: 0.18em;
		text-transform: uppercase;
		color: var(--accent);
	}
	.ask.first h2 {
		font: 600 22px var(--display);
		letter-spacing: 0.02em;
		text-transform: none;
		color: var(--text);
	}
	.said {
		font: inherit;
		white-space: pre-wrap;
	}
	.askbar {
		display: flex;
		align-items: center;
		gap: 14px;
		flex-wrap: wrap;
	}
	.which {
		display: flex;
		align-items: center;
		gap: 8px;
		color: var(--muted);
	}
	.which .input {
		width: auto;
	}
	.link {
		color: var(--accent);
	}
	.report {
		margin-bottom: 14px;
		border-color: var(--accent);
		font-size: 13px;
	}
	.report p {
		margin: 0;
	}
	.report b {
		color: var(--accent);
	}
	.caption {
		margin: 10px 0 4px !important;
		font-size: 11px;
		letter-spacing: 0.14em;
		text-transform: uppercase;
		color: var(--muted);
	}
	.report ul {
		margin: 0;
		padding-left: 18px;
		list-style: square;
		color: var(--text);
	}
	.columns {
		display: grid;
		grid-template-columns: minmax(0, 1.7fr) minmax(300px, 1fr);
		gap: 14px;
		align-items: start;
	}
	@media (max-width: 980px) {
		.columns {
			grid-template-columns: 1fr;
		}
	}
	.editor {
		display: grid;
		gap: 14px;
	}
	.tabs {
		display: flex;
		align-items: center;
		gap: 2px;
		border-bottom: 1px solid var(--border-strong);
	}
	.tabs button {
		padding: 6px 16px;
		font-size: 12px;
		letter-spacing: 0.14em;
		text-transform: uppercase;
		color: var(--muted);
		border-bottom: 2px solid transparent;
		margin-bottom: -1px;
	}
	.tabs button.on {
		color: var(--accent);
		border-bottom-color: var(--accent);
	}
	.tabs .grow {
		flex: 1;
		text-align: right;
	}
	.where {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
	}
	.source {
		min-height: 62vh;
	}
	.hint {
		margin: 0;
		font-size: 12px;
		color: var(--muted);
	}
	.warned {
		color: var(--warn);
		font-weight: 400;
	}
	.warning {
		margin: 0;
		padding: 9px 12px;
		border: 1px solid var(--warn);
		background: rgba(201, 161, 63, 0.07);
		font-size: 13px;
		color: #e2cf9a;
	}
	code {
		padding: 1px 5px;
		background: var(--surface-2);
		font: 12px Consolas, monospace;
		color: var(--text);
	}
	.preview {
		display: grid;
		gap: 14px;
		position: sticky;
		top: 64px;
	}
	.problem {
		margin: 0;
		padding: 9px 12px;
		border: 1px solid var(--bad);
		background: rgba(194, 86, 74, 0.08);
		color: #e6a69e;
		font-size: 13px;
		overflow-wrap: anywhere;
	}
	main > .problem {
		margin: 12px 0;
	}
	.problem b {
		display: block;
		color: var(--bad);
	}
	.fine {
		margin: 0;
		font-size: 11px;
		letter-spacing: 0.16em;
		text-transform: uppercase;
		color: var(--good);
	}
	.stale {
		opacity: 0.45;
	}
	h3 {
		font: 600 11px var(--body);
		letter-spacing: 0.16em;
		text-transform: uppercase;
		color: var(--accent);
	}
	.preview ul {
		margin: -6px 0 0;
		padding: 0;
		list-style: none;
		font-size: 13px;
	}
	.preview ul a:hover {
		color: var(--accent);
	}
</style>
