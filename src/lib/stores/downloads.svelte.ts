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

// Incoming events are buffered and flushed in one batch so a burst of thousands of
// "queued" events collapses into a single reactive update instead of thousands.
let eventQueue: DownloadEvent[] = [];
let flushScheduled = false;

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
	};
}

function mergeEvent(dl: Download, data: DownloadEvent): Download {
	return {
		...dl,
		status: data.status,
		progress: data.progress,
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
	setTimeout(flushEvents, 60);
}

function flushEvents() {
	flushScheduled = false;
	if (eventQueue.length === 0) return;
	const batch = eventQueue;
	eventQueue = [];

	const existing = downloads.slice();
	const index = new Map<number, number>();
	for (let i = 0; i < existing.length; i++) index.set(existing[i].id, i);

	const newById = new Map<number, Download>();
	const removed = new Set<number>();
	let activeDelta = 0;

	for (const data of batch) {
		const idx = index.get(data.id);
		if (idx !== undefined) {
			if (data.status === 'cancelled') {
				if (!removed.has(data.id) && isActive(existing[idx])) activeDelta--;
				removed.add(data.id);
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
			} else {
				newById.set(data.id, mergeEvent(cur, data));
			}
		} else if (data.status === 'queued' || data.status === 'downloading') {
			newById.set(data.id, placeholderFromEvent(data));
			activeDelta++;
		}
		// terminal status for an id outside our window: ignore (count stays from backend refresh)
	}

	const keptExisting = removed.size ? existing.filter((d) => !removed.has(d.id)) : existing;
	const fresh = [...newById.values()].reverse(); // newest first
	downloads = boundList([...fresh, ...keptExisting]);
	activeCount = Math.max(0, activeCount + activeDelta);
}

async function init() {
	if (initialized) return;
	initialized = true;
	initialFetchDone = false;
	eventQueue = [];

	if (unlisten) {
		unlisten();
		unlisten = null;
	}

	try {
		unlisten = await listen<DownloadEvent>('download-event', (event) => {
			eventQueue.push(event.payload);
			if (initialFetchDone) scheduleFlush();
		});
	} catch (e) {
		console.error('Failed to register download listener:', e);
		initialized = false;
		return;
	}

	try {
		const active = await getActiveDownloads();
		activeCount = active.length;
		downloads = boundList(active);
	} catch {
		// Ignore on first load
	}

	// Replay any events that arrived during the fetch.
	initialFetchDone = true;
	scheduleFlush();
}

function destroy() {
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
