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

async function init() {
	if (initialized) return;
	initialized = true;

	unlisten = await listen('library-updated', () => {
		invalidateCache();
		version++;
	});
}

function destroy() {
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
