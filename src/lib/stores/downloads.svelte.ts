import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Download, DownloadEvent } from '$lib/types';
import { getActiveDownloads } from '$lib/api/downloads';

const isActive = (d: Download) =>
	d.status === 'queued' || d.status === 'downloading' || d.status === 'processing';

// Only keep a bounded window of downloads in memory for rendering. Queuing a huge
// playlist (thousands of tracks) would otherwise hold thousands of objects and make
// every incoming progress event O(n) — freezing the UI. The true count lives in
// `activeCount`; the list is just a window (downloading/processing always kept).
const MAX_KEPT = 300;

let downloads: Download[] = $state([]);
let activeCount = $state(0);
let initialized = false;
let unlisten: UnlistenFn | null = null;
let initialFetchDone = false;
let initGeneration = 0;

// Incoming events are buffered and flushed in one batch so a burst of thousands of
// "queued" events collapses into a single reactive update instead of thousands.
let eventQueue: DownloadEvent[] = [];
let flushScheduled = false;
let flushTimer: ReturnType<typeof setTimeout> | null = null;

function placeholderFromEvent(data: DownloadEvent): Download {
	return {
		id: data.id,
		url: '',
		title: data.title ?? null,
		artist: null,
		platform: '',
		status: data.status,
		progress: data.progress,
		error_message: data.error ?? null,
		file_path: null,
		track_id: data.track_id ?? null,
		playlist_id: null,
		format: 'mp3',
		quality: 'best',
		created_at: new Date().toISOString(),
		started_at: null,
		completed_at: null,
		target_album_id: null,
		target_artist_id: null,
		target_isrc: null,
		target_disc_number: null,
		target_track_number: null,
		target_duration_ms: null,
		target_album_name: null,
		target_recording_mbid: null,
		speed: data.speed ?? null,
		eta: data.eta ?? null,
	};
}

function mergeEvent(dl: Download, data: DownloadEvent): Download {
	return {
		...dl,
		status: data.status,
		progress: data.progress,
		// Presentation-only live stats: always overwrite so a stale speed/ETA
		// never lingers after the download leaves the 'downloading' state.
		speed: data.speed ?? null,
		eta: data.eta ?? null,
		...(data.error ? { error_message: data.error } : {}),
		...(data.title ? { title: data.title } : {}),
		...(data.track_id ? { track_id: data.track_id } : {}),
	};
}

/** Keep at most MAX_KEPT objects, always retaining anything currently downloading. */
function boundList(list: Download[]): Download[] {
	if (list.length <= MAX_KEPT) return list;
	const downloading = list.filter((d) => d.status === 'downloading' || d.status === 'processing');
	const rest = list.filter((d) => d.status !== 'downloading' && d.status !== 'processing');
	return [...downloading, ...rest].slice(0, MAX_KEPT);
}

function scheduleFlush() {
	if (flushScheduled) return;
	flushScheduled = true;
	flushTimer = setTimeout(flushEvents, 60);
}

function flushEvents() {
	flushScheduled = false;
	flushTimer = null;
	if (eventQueue.length === 0) return;
	const batch = eventQueue;
	eventQueue = [];

	const existing = downloads.slice();
	const index = new Map<number, number>();
	for (let i = 0; i < existing.length; i++) index.set(existing[i].id, i);

	const newById = new Map<number, Download>();
	const removed = new Set<number>();
	let activeDelta = 0;
	let sawUnknownActive = false;

	for (const data of batch) {
		const idx = index.get(data.id);
		if (idx !== undefined) {
			if (data.status === 'cancelled') {
				if (!removed.has(data.id) && isActive(existing[idx])) activeDelta--;
				removed.add(data.id);
				// Record it so a duplicate terminal event in a later batch
				// can't double-decrement the badge.
				terminalUnknown.add(data.id);
			} else {
				const wasActive = isActive(existing[idx]);
				existing[idx] = mergeEvent(existing[idx], data);
				removed.delete(data.id);
				activeDelta += (isActive(existing[idx]) ? 1 : 0) - (wasActive ? 1 : 0);
			}
		} else if (newById.has(data.id)) {
			const cur = newById.get(data.id)!;
			if (data.status === 'cancelled') {
				if (isActive(cur)) activeDelta--;
				newById.delete(data.id);
				terminalUnknown.add(data.id);
			} else {
				newById.set(data.id, mergeEvent(cur, data));
			}
		} else if (data.status === 'queued' || data.status === 'downloading' || data.status === 'processing') {
			newById.set(data.id, placeholderFromEvent(data));
			activeDelta++;
			// A re-queued id (e.g. retry) is tracked again — forget any old
			// terminal count so a future out-of-window terminal still counts.
			terminalUnknown.delete(data.id);
			// The id may have been evicted from the window while still counted as
			// active, so the local ++ can drift — re-sync with the backend.
			sawUnknownActive = true;
		} else if (!terminalUnknown.has(data.id)) {
			// Terminal status for an id outside our window: it was evicted by
			// boundList while still active, so decrement — ignoring these left
			// the badge stuck at hundreds of "active" downloads after a big
			// playlist finished. A backend refresh re-syncs authoritatively.
			terminalUnknown.add(data.id);
			activeDelta--;
		}
	}

	const keptExisting = removed.size ? existing.filter((d) => !removed.has(d.id)) : existing;
	const fresh = [...newById.values()].reverse(); // newest first
	downloads = boundList([...fresh, ...keptExisting]);
	activeCount = Math.max(0, activeCount + activeDelta);

	if (terminalUnknown.size > 0 || sawUnknownActive) scheduleRefresh();
}

// Ids outside the rendered window whose terminal event was already counted,
// so duplicate terminal events don't double-decrement.
const terminalUnknown = new Set<number>();

// Debounced authoritative re-sync with the backend after out-of-window
// terminal events (the local delta is a best guess).
let refreshScheduled = false;
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
function scheduleRefresh() {
	if (refreshScheduled) return;
	refreshScheduled = true;
	refreshTimer = setTimeout(async () => {
		refreshScheduled = false;
		refreshTimer = null;
		terminalUnknown.clear();
		await downloadStore.refresh();
	}, 2000);
}

async function init() {
	if (initialized) return;
	initialized = true;
	const generation = ++initGeneration;
	initialFetchDone = false;
	eventQueue = [];

	if (unlisten) {
		unlisten();
		unlisten = null;
	}

	try {
		const fn = await listen<DownloadEvent>('download-event', (event) => {
			eventQueue.push(event.payload);
			if (initialFetchDone) scheduleFlush();
		});
		if (!initialized || generation !== initGeneration) {
			fn();
			return;
		}
		unlisten = fn;
	} catch (e) {
		console.error('Failed to register download listener:', e);
		if (generation === initGeneration) initialized = false;
		return;
	}

	try {
		const active = await getActiveDownloads();
		if (!initialized || generation !== initGeneration) return;
		activeCount = active.length;
		downloads = boundList(active);
	} catch {
		// Ignore on first load
	}

	// Replay any events that arrived during the fetch.
	if (initialized && generation === initGeneration) {
		initialFetchDone = true;
		scheduleFlush();
	}
}

function destroy() {
	initGeneration++;
	if (flushTimer) clearTimeout(flushTimer);
	if (refreshTimer) clearTimeout(refreshTimer);
	flushTimer = null;
	refreshTimer = null;
	flushScheduled = false;
	refreshScheduled = false;
	eventQueue = [];
	terminalUnknown.clear();
	initialFetchDone = false;
	if (unlisten) {
		unlisten();
		unlisten = null;
	}
	initialized = false;
}

export const downloadStore = {
	get downloads() {
		return downloads;
	},

	/** True number of active downloads (may exceed the rendered window). */
	get activeCount() {
		return activeCount;
	},

	init,
	destroy,

	addDownload(download: Download) {
		const idx = downloads.findIndex((d) => d.id === download.id);
		if (idx >= 0) {
			downloads[idx] = { ...downloads[idx], ...download };
			downloads = [...downloads];
		} else {
			downloads = boundList([download, ...downloads]);
			if (isActive(download)) activeCount++;
		}
	},

	addDownloads(newDownloads: Download[]) {
		const known = new Set(downloads.map((d) => d.id));
		const toAdd = newDownloads.filter((d) => !known.has(d.id));
		activeCount += toAdd.filter(isActive).length;
		downloads = boundList([...toAdd, ...downloads]);
	},

	removeDownload(id: number) {
		const dl = downloads.find((d) => d.id === id);
		if (dl && isActive(dl)) activeCount = Math.max(0, activeCount - 1);
		downloads = downloads.filter((d) => d.id !== id);
	},

	clearCompleted() {
		downloads = downloads.filter(isActive);
	},

	/** Re-sync with the backend — authoritative for both the window and the true count. */
	async refresh() {
		try {
			const active = await getActiveDownloads();
			activeCount = active.length;
			const kept = downloads.filter((d) => !isActive(d)).slice(0, 50);
			downloads = boundList([...active, ...kept]);
		} catch {
			// ignore
		}
	},

	reset() {
		downloads = [];
		activeCount = 0;
	},
};
