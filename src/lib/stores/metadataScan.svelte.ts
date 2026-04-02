import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { MetadataScanProgress } from '$lib/types';

let scanning = $state(false);
let progress: MetadataScanProgress | null = $state(null);
let lastResult: { enriched: number; failed: number; completeness_avg: number } | null = $state(null);

let initialized = false;
let unlistenProgress: UnlistenFn | null = null;
let unlistenComplete: UnlistenFn | null = null;

async function init() {
	if (initialized) return;
	initialized = true;

	unlistenProgress = await listen<MetadataScanProgress>('metadata-scan-progress', (event) => {
		scanning = true;
		progress = event.payload;
	});

	unlistenComplete = await listen<{ enriched: number; failed: number; completeness_avg: number }>(
		'metadata-scan-complete',
		(event) => {
			scanning = false;
			progress = null;
			lastResult = event.payload;
		}
	);
}

function destroy() {
	unlistenProgress?.();
	unlistenComplete?.();
	unlistenProgress = null;
	unlistenComplete = null;
	initialized = false;
}

function markScanning() {
	scanning = true;
	progress = null;
	lastResult = null;
}

function markDone() {
	scanning = false;
	progress = null;
}

export const metadataScanStore = {
	get scanning() { return scanning; },
	get progress() { return progress; },
	get lastResult() { return lastResult; },
	init,
	destroy,
	markScanning,
	markDone,
};
