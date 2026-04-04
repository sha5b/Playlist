import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Download, DownloadEvent } from '$lib/types';
import { getActiveDownloads } from '$lib/api/downloads';

const isActive = (d: Download) =>
	d.status === 'queued' || d.status === 'downloading' || d.status === 'processing';

let downloads: Download[] = $state([]);
let initialized = false;
let unlisten: UnlistenFn | null = null;

// Buffer events received before initial DB fetch completes to prevent duplicates
let pendingEvents: DownloadEvent[] = [];
let initialFetchDone = false;

function handleEvent(data: DownloadEvent) {
	const idx = downloads.findIndex((d) => d.id === data.id);

	if (idx >= 0) {
		const dl = downloads[idx];
		const updated = {
			...dl,
			status: data.status,
			progress: data.progress,
			...(data.error ? { error_message: data.error } : {}),
			...(data.title ? { title: data.title } : {}),
			...(data.track_id ? { track_id: data.track_id } : {}),
		};
		if (data.status === 'cancelled') {
			downloads = downloads.filter((d) => d.id !== data.id);
		} else {
			downloads = downloads.map((d, i) => (i === idx ? updated : d));
		}
	} else if (data.status === 'queued' || data.status === 'downloading') {
		downloads = [
			{
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
				format: 'opus',
				quality: 'best',
				created_at: new Date().toISOString(),
				started_at: null,
				completed_at: null,
				target_album_id: null,
				target_artist_id: null,
				target_isrc: null,
			},
			...downloads,
		];
	}

	if (downloads.length > 100) {
		const active = downloads.filter(isActive);
		const rest = downloads.filter((d) => !isActive(d)).slice(0, 50);
		downloads = [...active, ...rest];
	}
}

async function init() {
	if (initialized) return;
	initialized = true;
	initialFetchDone = false;
	pendingEvents = [];

	if (unlisten) {
		unlisten();
		unlisten = null;
	}

	try {
	unlisten = await listen<DownloadEvent>('download-event', (event) => {
		if (!initialFetchDone) {
			pendingEvents.push(event.payload);
			return;
		}
		handleEvent(event.payload);
	});
	} catch (e) {
		console.error('Failed to register download listener:', e);
		initialized = false;
		return;
	}

	try {
		downloads = await getActiveDownloads();
	} catch {
		// Ignore on first load
	}

	// Replay any events that arrived during the fetch
	initialFetchDone = true;
	for (const evt of pendingEvents) {
		handleEvent(evt);
	}
	pendingEvents = [];
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

	get activeCount() {
		return downloads.filter(isActive).length;
	},

	init,
	destroy,

	addDownload(download: Download) {
		const idx = downloads.findIndex((d) => d.id === download.id);
		if (idx >= 0) {
			// Merge: command response has richer data than the early event
			downloads[idx] = { ...downloads[idx], ...download };
			downloads = [...downloads];
		} else {
			downloads = [download, ...downloads];
		}
	},

	addDownloads(newDownloads: Download[]) {
		const toAdd: Download[] = [];
		for (const dl of newDownloads) {
			const idx = downloads.findIndex((d) => d.id === dl.id);
			if (idx >= 0) {
				downloads[idx] = { ...downloads[idx], ...dl };
			} else {
				toAdd.push(dl);
			}
		}
		downloads = [...toAdd, ...downloads];
	},

	removeDownload(id: number) {
		downloads = downloads.filter((d) => d.id !== id);
	},

	clearCompleted() {
		downloads = downloads.filter(isActive);
	},

	/** Re-sync with backend — removes stale entries that were cancelled/completed in the DB. */
	async refresh() {
		try {
			const active = await getActiveDownloads();
			// Keep backend-confirmed active downloads + recent completed/failed ones from the store
			const kept = downloads.filter((d) => !isActive(d));
			downloads = [...active, ...kept.slice(0, 50)];
		} catch {
			// ignore
		}
	},

	/** Clear all downloads from the store (used on library reset). */
	reset() {
		downloads = [];
	},
};
