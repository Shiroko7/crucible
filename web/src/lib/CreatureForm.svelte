<script lang="ts">
	// A creature as a sheet to fill in. It edits the one table its file
	// holds; whatever it has no field for is left in that table as it was.
	import { PHRASES } from './templates';
	import type { CreatureForm } from './types';

	let {
		form = $bindable(),
		features,
		onchange
	}: {
		form: CreatureForm;
		/** The features a file can name. */
		features: string[];
		/** Something on the sheet changed. */
		onchange: () => void;
	} = $props();

	const ABILITIES = ['str', 'dex', 'con', 'int', 'wis', 'cha'];
	const SIZES = ['Tiny', 'Small', 'Medium', 'Large', 'Huge', 'Gargantuan'];
	const TYPES = [
		'Aberration',
		'Beast',
		'Celestial',
		'Construct',
		'Dragon',
		'Elemental',
		'Fey',
		'Fiend',
		'Giant',
		'Humanoid',
		'Monstrosity',
		'Ooze',
		'Plant',
		'Undead'
	];
	const LEVELS = ['1', '2', '3', '4', '5', '6', '7', '8', '9'];

	const character = $derived(form.kind === 'character');
	const table = $derived(form.table);
	const MOVES = $derived(
		[
			{ key: 'actions', label: 'Actions', one: 'action', hint: 'What it does with its action.' },
			{ key: 'bonus', label: 'Bonus actions', one: 'bonus action', hint: '' },
			{
				key: 'reactions',
				label: 'Reactions',
				one: 'reaction',
				hint: 'Each needs a "when ..." clause: when hit in melee, when enemy pulled.'
			},
			{
				key: 'legendary',
				label: 'Legendary actions',
				one: 'legendary action',
				hint: '',
				monster: true
			},
			{
				key: 'auras',
				label: 'Auras',
				one: 'aura',
				hint: 'Every enemy is subject to it at the start of its turn.'
			}
		].filter((slot) => !slot.monster || !character)
	);
	const DEFENCES = [
		{ key: 'resist', label: 'Resistances', hint: 'fire, cold' },
		{ key: 'immune', label: 'Immunities', hint: 'poison' },
		{ key: 'vulnerable', label: 'Vulnerabilities', hint: 'radiant' },
		{ key: 'condition_immune', label: 'Condition immunities', hint: 'poisoned, frightened' }
	];

	function split(text: string): string[] {
		return text
			.split(',')
			.map((part) => part.trim())
			.filter(Boolean);
	}

	function add(key: string, item: unknown) {
		table[key].push(item);
		onchange();
	}

	function remove(key: string, at: number) {
		table[key].splice(at, 1);
		onchange();
	}

	function raise(key: string, at: number) {
		if (at === 0) return;
		const list = table[key];
		[list[at - 1], list[at]] = [list[at], list[at - 1]];
		onchange();
	}

	// Named pools sit beside the spell slots in one table of the file.
	const pools = $derived(
		Object.entries(table.resources as Record<string, unknown>).filter(([name]) => name !== 'slots')
	);

	function renamePool(old: string, name: string) {
		const key = name.trim().toLowerCase().replace(/\s+/g, '_');
		if (!key || key === old || key === 'slots' || key in table.resources) return;
		const max = table.resources[old];
		delete table.resources[old];
		table.resources[key] = max;
		onchange();
	}

	function addPool() {
		let name = 'pool';
		for (let n = 2; name in table.resources; n += 1) name = `pool_${n}`;
		table.resources[name] = 1;
		onchange();
	}

	function removePool(name: string) {
		delete table.resources[name];
		onchange();
	}

	function setSlot(level: string, value: string) {
		if (value === '' || Number(value) <= 0) delete table.resources.slots[level];
		else table.resources.slots[level] = Number(value);
	}

	function caster(on: boolean) {
		if (on) table.spellcasting = { ability: 'wis', ability_modifier: 0, proficiency_bonus: 2 };
		else delete table.spellcasting;
		onchange();
	}

	/** A feature's own settings: the ones that are a number or a word. */
	function settings(feature: Record<string, unknown>): string[] {
		return Object.keys(feature).filter(
			(key) => key !== 'plugin' && ['number', 'string'].includes(typeof feature[key])
		);
	}

	function others(feature: Record<string, unknown>): string[] {
		return Object.keys(feature).filter(
			(key) => key !== 'plugin' && !['number', 'string'].includes(typeof feature[key])
		);
	}

	function spoken(id: string): string {
		return id.replaceAll('_', ' ');
	}
</script>

<div class="sheet" oninput={onchange} data-test="sheet">
	<section>
		<h3>Who it is</h3>
		<div class="row">
			<label class="label grow">
				Name
				<input class="input" bind:value={table.name} data-test="field-name" />
			</label>
			{#if character}
				<label class="label">
					Class
					<input class="input" bind:value={table.class} />
				</label>
				<label class="label">
					Subclass
					<input class="input" bind:value={table.subclass} />
				</label>
				<label class="label">
					Species
					<input class="input" bind:value={table.species} />
				</label>
				<label class="label small">
					Level
					<input class="input" type="number" min="1" max="20" bind:value={table.level} />
				</label>
			{:else}
				<label class="label small">
					CR
					<input class="input" bind:value={table.cr} />
				</label>
				<label class="label">
					Size
					<select class="input" bind:value={table.size} data-test="field-size">
						<option value="">Medium</option>
						{#each SIZES as size (size)}<option value={size}>{size}</option>{/each}
					</select>
				</label>
				<label class="label">
					Type
					<select class="input" bind:value={table.creature_type}>
						<option value="">-</option>
						{#each TYPES as type (type)}<option value={type}>{type}</option>{/each}
					</select>
				</label>
			{/if}
		</div>
	</section>

	<section>
		<h3>In a fight</h3>
		<div class="row">
			<label class="label small">
				Armor Class
				<input class="input" type="number" bind:value={table.ac} data-test="field-ac" />
			</label>
			<label class="label small">
				Hit Points
				<input class="input" type="number" min="1" bind:value={table.hp} data-test="field-hp" />
			</label>
			<label class="label small">
				Initiative
				<input class="input" type="number" bind:value={table.initiative} placeholder="0" />
			</label>
			<label class="label small">
				Speed (ft)
				<input class="input" type="number" min="0" step="5" bind:value={table.speed} placeholder="30" />
			</label>
			{#if !character}
				<label class="label small">
					Legendary / round
					<input class="input" type="number" min="0" bind:value={table.legendary_uses} placeholder="0" />
				</label>
			{/if}
		</div>
		<p class="caption">Saving throw bonuses</p>
		<div class="row six">
			{#each ABILITIES as ability (ability)}
				<label class="label small">
					{ability}
					<input class="input" type="number" bind:value={table.saves[ability]} placeholder="0" />
				</label>
			{/each}
		</div>
	</section>

	{#each MOVES as slot (slot.key)}
		<section data-test="moves-{slot.key}">
			<h3>{slot.label}</h3>
			{#if slot.hint && table[slot.key].length > 0}<p class="hint">{slot.hint}</p>{/if}
			{#each table[slot.key] as move, at (at)}
				<div class="move">
					<input
						class="input name"
						bind:value={move.name}
						placeholder="Name"
						aria-label="Name"
						data-test="move-name"
					/>
					<textarea
						class="input effect"
						bind:value={move.effect}
						rows="1"
						spellcheck="false"
						placeholder="strikes 1 | hit +5 | 1d8+3 slashing"
						aria-label="What it does"
						data-test="move-effect"
					></textarea>
					<button class="icon" onclick={() => raise(slot.key, at)} disabled={at === 0} title="Prefer it">
						↑
					</button>
					<button class="icon" onclick={() => remove(slot.key, at)} title="Remove">✕</button>
				</div>
			{/each}
			<button
				class="button add"
				onclick={() => add(slot.key, { name: '', effect: '' })}
				data-test="add-{slot.key}"
			>
				+ Add {slot.one}
			</button>
		</section>
	{/each}
	<details>
		<summary>How a move is written</summary>
		<p class="hint">
			What a move does is phrases joined by <code>|</code>. Listed first is preferred first.
		</p>
		<dl>
			{#each PHRASES as phrase (phrase.say)}
				<dt><code>{phrase.say}</code></dt>
				<dd>{phrase.means}</dd>
			{/each}
		</dl>
	</details>

	<section>
		<h3>Spells and features it knows</h3>
		<p class="hint">
			Things the simulator already knows how to play. Anything else is written out as a move.
		</p>
		{#each table.features as feature, at (at)}
			<div class="move">
				<select class="input name" bind:value={feature.plugin} aria-label="Feature">
					{#if !features.includes(feature.plugin)}
						<option value={feature.plugin}>{feature.plugin || 'Choose one'}</option>
					{/if}
					{#each features as id (id)}<option value={id}>{spoken(id)}</option>{/each}
				</select>
				<div class="settings">
					{#each settings(feature) as key (key)}
						<label class="setting">
							{spoken(key)}
							{#if typeof feature[key] === 'number'}
								<input class="input" type="number" bind:value={feature[key]} />
							{:else}
								<input class="input" bind:value={feature[key]} />
							{/if}
						</label>
					{/each}
					{#each others(feature) as key (key)}
						<span class="fixed" title="Change this one in the file">
							{spoken(key)}: {JSON.stringify(feature[key])}
						</span>
					{/each}
				</div>
				<button class="icon" onclick={() => remove('features', at)} title="Remove">✕</button>
			</div>
		{/each}
		<button class="button add" onclick={() => add('features', { plugin: '' })}>
			+ Add a spell or feature
		</button>
	</section>

	<section>
		<h3>Spellcasting and resources</h3>
		<div class="row">
			<label class="check">
				<input
					type="checkbox"
					checked={!!table.spellcasting}
					onchange={(e) => caster(e.currentTarget.checked)}
				/>
				Casts spells
			</label>
			{#if table.spellcasting}
				<label class="label">
					Ability
					<select class="input" bind:value={table.spellcasting.ability}>
						{#each ABILITIES as ability (ability)}<option value={ability}>{ability}</option>{/each}
					</select>
				</label>
				<label class="label small">
					Modifier
					<input class="input" type="number" bind:value={table.spellcasting.ability_modifier} />
				</label>
				<label class="label small">
					Proficiency
					<input class="input" type="number" bind:value={table.spellcasting.proficiency_bonus} />
				</label>
			{/if}
		</div>
		<p class="caption">Spell slots by level</p>
		<div class="row nine">
			{#each LEVELS as level (level)}
				<label class="label small">
					{level}
					<input
						class="input"
						type="number"
						min="0"
						value={table.resources.slots[level] ?? ''}
						oninput={(e) => setSlot(level, e.currentTarget.value)}
						placeholder="-"
					/>
				</label>
			{/each}
		</div>
		<p class="caption">Pools a move can spend from</p>
		{#each pools as [name, max] (name)}
			<div class="move">
				<input
					class="input name"
					value={name}
					onchange={(e) => renamePool(name, e.currentTarget.value)}
					aria-label="Pool"
				/>
				<input
					class="input count"
					type="number"
					min="0"
					value={max}
					oninput={(e) => (table.resources[name] = Number(e.currentTarget.value))}
					aria-label="How many"
				/>
				<span class="grow hint">A move pays with <code>cost {name} 1</code>.</span>
				<button class="icon" onclick={() => removePool(name)} title="Remove">✕</button>
			</div>
		{/each}
		<button class="button add" onclick={addPool}>+ Add a pool</button>
	</section>

	<section>
		<h3>Defences and traits</h3>
		<div class="row">
			{#each DEFENCES as defence (defence.key)}
				<label class="label grow">
					{defence.label}
					<input
						class="input"
						value={table[defence.key].join(', ')}
						placeholder={defence.hint}
						onchange={(e) => {
							table[defence.key] = split(e.currentTarget.value);
							onchange();
						}}
					/>
				</label>
			{/each}
		</div>
		<p class="caption">Traits: what is always true of it</p>
		{#each table.traits as _, at (at)}
			<div class="move">
				<input
					class="input effect"
					bind:value={table.traits[at]}
					placeholder="legendary resistance 3"
					spellcheck="false"
					aria-label="Trait"
				/>
				<button class="icon" onclick={() => remove('traits', at)} title="Remove">✕</button>
			</div>
		{/each}
		<button class="button add" onclick={() => add('traits', '')}>+ Add a trait</button>
	</section>

	<section>
		<h3>Notes</h3>
		<textarea
			class="input notes"
			bind:value={form.notes}
			rows="3"
			placeholder="Where it is from, rulings made at the table: kept at the top of its file."
		></textarea>
	</section>
</div>

<style>
	.sheet {
		display: grid;
		gap: 22px;
	}
	section {
		display: grid;
		gap: 9px;
	}
	h3 {
		font: 600 11px var(--body);
		letter-spacing: 0.18em;
		text-transform: uppercase;
		color: var(--accent);
		padding-bottom: 6px;
		border-bottom: 1px solid var(--border);
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: 10px;
		align-items: end;
	}
	.label {
		flex: 1 1 130px;
	}
	.label.grow {
		flex: 2 1 200px;
	}
	.label.small {
		flex: 0 1 96px;
	}
	.six .label.small,
	.nine .label.small {
		flex: 1 1 56px;
	}
	.caption {
		margin: 6px 0 -2px;
		font-size: 11px;
		letter-spacing: 0.12em;
		text-transform: uppercase;
		color: var(--faint);
	}
	.hint {
		margin: 0;
		font-size: 12px;
		color: var(--muted);
	}
	.move {
		display: flex;
		gap: 8px;
		align-items: start;
	}
	.name {
		flex: 0 0 190px;
		width: 190px;
	}
	.count {
		flex: 0 0 80px;
		width: 80px;
	}
	.effect {
		flex: 1;
		min-width: 0;
		font: 13px/1.55 Consolas, 'Cascadia Mono', Menlo, monospace;
		white-space: pre-wrap;
		field-sizing: content;
		min-height: 34px;
	}
	.grow {
		flex: 1;
		align-self: center;
	}
	.settings {
		flex: 1;
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		align-items: center;
		min-width: 0;
	}
	.setting {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 12px;
		color: var(--muted);
	}
	.setting .input {
		width: 110px;
	}
	.fixed {
		font: 12px Consolas, monospace;
		color: var(--faint);
		overflow-wrap: anywhere;
	}
	.icon {
		flex: 0 0 30px;
		height: 34px;
		border: 1px solid var(--border);
		color: var(--muted);
	}
	.icon:hover:not(:disabled) {
		border-color: var(--accent);
		color: var(--accent);
	}
	.add {
		justify-self: start;
		padding: 4px 10px;
		font-size: 12px;
		color: var(--muted);
	}
	.check {
		display: flex;
		align-items: center;
		gap: 8px;
		padding-bottom: 8px;
		flex: 0 0 auto;
	}
	.notes {
		font: inherit;
		white-space: pre-wrap;
	}
	details {
		margin-top: -10px;
	}
	summary {
		cursor: pointer;
		font-size: 12px;
		color: var(--muted);
	}
	dl {
		margin: 10px 0 0;
		display: grid;
		gap: 2px;
	}
	dd {
		margin: 0 0 8px;
		font-size: 12px;
		color: var(--muted);
	}
	code {
		padding: 1px 5px;
		background: var(--surface-2);
		font: 12px Consolas, monospace;
		color: var(--text);
	}
</style>
