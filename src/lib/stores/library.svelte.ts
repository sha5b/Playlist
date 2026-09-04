import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invalidateCache } from '$lib/api/library';

/**
 * Reactive version counter that increments whenever the library changes
 * (download completed, folder imported, etc).
 * Pages use `$effect(() => { libraryStore.version; load(); })` to auto-refresh.
 */
let version = $state(0);
let initialized = false;
let unlisten: UnlistenFn | null = null;
let initGeneration = 0;

async function init() {
	if (initialized) return;
	initialized = true;
	const generation = ++initGeneration;

	let fn: UnlistenFn;
	try {
		fn = await listen('library-updated', () => {
			invalidateCache();
			version++;
		});
	} catch (e) {
		console.error('Failed to register library listener:', e);
		if (generation === initGeneration) initialized = false;
		return;
	}
	if (!initialized || generation !== initGeneration) {
		fn();
		return;
	}
	unlisten = fn;
}

function destroy() {
	initGeneration++;
	if (unlisten) {
		unlisten();
		unlisten = null;
	}
	initialized = false;
}

export const libraryStore = {
	get version() {
		return version;
	},
	init,
	destroy,
};
