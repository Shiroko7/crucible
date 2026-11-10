import type {
	Bundle,
	Checked,
	CreatureForm,
	CreatureInfo,
	CreatureSheet,
	Drafted,
	Fielded,
	Rendered,
	RunOptions,
	ScenarioInfo,
	ScenarioSheet
} from './types';

async function answer<T>(response: Response): Promise<T> {
	const body = await response.json().catch(() => null);
	if (!response.ok) {
		throw new Error(body?.error ?? `the server answered ${response.status}`);
	}
	return body as T;
}

/** Every scenario on disk. */
export async function scenarios(): Promise<ScenarioInfo[]> {
	return answer(await fetch('/api/scenarios'));
}

/** Run a scenario and get back the fights worth watching. */
export async function showcase(path: string, options: RunOptions): Promise<Bundle> {
	return answer(
		await fetch('/api/showcase', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ path, ...options })
		})
	);
}

function sent(method: string, body: unknown): RequestInit {
	return {
		method,
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify(body)
	};
}

const at = (path: string) => `path=${encodeURIComponent(path)}`;

/** Every creature file on disk. */
export async function creatures(): Promise<CreatureInfo[]> {
	return answer(await fetch('/api/creatures'));
}

export async function creature(path: string): Promise<CreatureSheet> {
	return answer(await fetch(`/api/creature?${at(path)}`));
}

/** What a draft loads as, without saving it. */
export async function checkCreature(text: string): Promise<Checked> {
	return answer(await fetch('/api/creature/check', sent('POST', { text })));
}

/** A sheet as the file it would be saved as, and what that loads as. */
export async function renderCreature(form: CreatureForm): Promise<Rendered> {
	return answer(await fetch('/api/creature/render', sent('POST', form)));
}

/** Have the model write a creature from pasted text, or change the one given. */
export async function draftCreature(request: {
	source: string;
	kind: string | null;
	current: string | null;
}): Promise<Drafted> {
	return answer(await fetch('/api/creature/draft', sent('POST', request)));
}

/** The features a creature file can name. */
export async function features(): Promise<string[]> {
	return answer(await fetch('/api/features'));
}

export async function saveCreature(
	path: string,
	text: string,
	fresh: boolean
): Promise<CreatureInfo> {
	return answer(await fetch('/api/creature', sent('PUT', { path, text, fresh })));
}

export async function removeCreature(path: string): Promise<void> {
	await answer(await fetch(`/api/creature?${at(path)}`, { method: 'DELETE' }));
}

export async function scenario(path: string): Promise<ScenarioSheet> {
	return answer(await fetch(`/api/scenario?${at(path)}`));
}

/** Save a scenario: as text, or as the creature files it is built from. */
export async function saveScenario(
	path: string,
	fresh: boolean,
	body: { text: string } | { notes: string; entries: Fielded[] }
): Promise<ScenarioInfo> {
	return answer(await fetch('/api/scenario', sent('PUT', { path, fresh, ...body })));
}

export async function removeScenario(path: string): Promise<void> {
	await answer(await fetch(`/api/scenario?${at(path)}`, { method: 'DELETE' }));
}
