import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { MetadataScanProgress } from '$lib/types';

let scanning = $state(false);
let progress: MetadataScanProgress | null = $state(null);
let lastResult: { enriched: number; failed: number; completeness_avg: number } | null = $state(null);

// Background auto-enrich state
let autoEnriching = $state(false);
let autoPhase = $state<string>('');
let autoCurrent = $state(0);
let autoTotal = $state(0);
let autoTitle = $state('');

let initialized = false;
let unlistenProgress: UnlistenFn | null = null;
let unlistenComplete: UnlistenFn | null = null;
let unlistenAutoEnrich: UnlistenFn | null = null;
let initGeneration = 0;

async function init() {
	if (initialized) return;
	initialized = true;
	const generation = ++initGeneration;

	let progressListener: UnlistenFn;
	try {
		progressListener = await listen<MetadataScanProgress>('metadata-scan-progress', (event) => {
			scanning = true;
			progress = event.payload;
		});
	} catch (e) {
		console.error('Failed to register metadata progress listener:', e);
		if (generation === initGeneration) initialized = false;
		return;
	}
	if (!initialized || generation !== initGeneration) {
		progressListener();
		return;
	}
	unlistenProgress = progressListener;

	let completeListener: UnlistenFn;
	try {
		completeListener = await listen<{ enriched: number; failed: number; completeness_avg: number }>(
			'metadata-scan-complete',
			(event) => {
				scanning = false;
				progress = null;
				lastResult = event.payload;
			}
		);
	} catch (e) {
		console.error('Failed to register metadata completion listener:', e);
		if (generation === initGeneration) {
			progressListener();
			unlistenProgress = null;
			initialized = false;
		}
		return;
	}
	if (!initialized || generation !== initGeneration) {
		completeListener();
		return;
	}
	unlistenComplete = completeListener;

	let autoEnrichListener: UnlistenFn;
	try {
		autoEnrichListener = await listen<{ phase: string; current?: number; total?: number; title?: string }>(
			'auto-enrich-progress',
			(event) => {
				const p = event.payload;
				if (p.phase === 'complete') {
					autoEnriching = false;
					autoPhase = '';
					autoCurrent = 0;
					autoTotal = 0;
					autoTitle = '';
				} else {
					autoEnriching = true;
					autoPhase = p.phase;
					autoCurrent = p.current ?? 0;
					autoTotal = p.total ?? 0;
					autoTitle = p.title ?? '';
				}
			}
		);
	} catch (e) {
		console.error('Failed to register auto-enrichment listener:', e);
		if (generation === initGeneration) {
			progressListener();
			completeListener();
			unlistenProgress = null;
			unlistenComplete = null;
			initialized = false;
		}
		return;
	}
	if (!initialized || generation !== initGeneration) {
		autoEnrichListener();
		return;
	}
	unlistenAutoEnrich = autoEnrichListener;
}

function destroy() {
	initGeneration++;
	unlistenProgress?.();
	unlistenComplete?.();
	unlistenAutoEnrich?.();
	unlistenProgress = null;
	unlistenComplete = null;
	unlistenAutoEnrich = null;
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
	get autoEnriching() { return autoEnriching; },
	get autoPhase() { return autoPhase; },
	get autoCurrent() { return autoCurrent; },
	get autoTotal() { return autoTotal; },
	get autoTitle() { return autoTitle; },
	get active() { return scanning || autoEnriching; },
	init,
	destroy,
	markScanning,
	markDone,
};
