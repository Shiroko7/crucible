// What the screens share: how a run is set up, and what has already been
// fetched, so going back to a screen does not run its simulation again.
import * as api from './api';
import type { Bundle, ScenarioInfo } from './types';

export const settings = $state({ seed: 1, samples: 500, apart: 30, across: 40, down: 30 });

let library: Promise<ScenarioInfo[]> | null = null;
const bundles = new Map<string, Promise<Bundle>>();

export function loadLibrary(): Promise<ScenarioInfo[]> {
	library ??= api.scenarios().catch((e) => {
		library = null;
		throw e;
	});
	return library;
}

/** The recorded fights of a scenario under the current settings. */
export function loadBundle(path: string, again = false): Promise<Bundle> {
	const map: [number, number] = [settings.across, settings.down];
	const key = `${path}|${settings.seed}|${settings.samples}|${settings.apart}|${map}`;
	if (again) bundles.delete(key);
	let bundle = bundles.get(key);
	if (!bundle) {
		const { seed, samples, apart } = settings;
		bundle = api.showcase(path, { seed, samples, apart, map }).catch((e) => {
			bundles.delete(key);
			throw e;
		});
		bundles.set(key, bundle);
	}
	return bundle;
}

/** A scenario or a creature was saved or removed: nothing fetched still holds. */
export function forget() {
	library = null;
	bundles.clear();
}
