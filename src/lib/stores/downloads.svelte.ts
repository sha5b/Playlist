import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Download, DownloadEvent } from '$lib/types';
import { getActiveDownloads } from '$lib/api/downloads';

const isActive = (d: Download) =>
	d.status === 'queued' || d.status === 'downloading' || d.status === 'processing';

let downloads: Download[] = $state([]);
let initialized = false;
let unlisten: UnlistenFn | null = null;

async function init() {
	if (initialized) return;
	initialized = true;

	// Load active downloads
	try {
		downloads = await getActiveDownloads();
	} catch {
		// Ignore on first load
	}

	// Clean up any previous listener before registering
	if (unlisten) {
		unlisten();
		unlisten = null;
	}

	// Listen for download events from Rust
	unlisten = await listen<DownloadEvent>('download-event', (event) => {
		const data = event.payload;
		const idx = downloads.findIndex((d) => d.id === data.id);

		if (idx >= 0) {
			const dl = downloads[idx];
			const statusChanged = dl.status !== data.status;
			// Mutate in place for progress-only updates (avoids cloning the whole array)
			dl.status = data.status as Download['status'];
			dl.progress = data.progress;
			if (data.error) dl.error_message = data.error;
			if (data.title) dl.title = data.title;
			if (data.track_id) dl.track_id = data.track_id;
			// Only clone the array when status actually changes (not on progress ticks)
			if (statusChanged) {
				// Remove cancelled downloads from the store immediately
				if (data.status === 'cancelled') {
					downloads = downloads.filter((d) => d.id !== data.id);
				} else {
					downloads = [...downloads];
				}
			}
		} else if (data.status === 'queued' || data.status === 'downloading') {
			// Auto-add downloads from bulk operations
			downloads = [
				{
					id: data.id,
					url: '',
					title: data.title ?? null,
					artist: null,
					platform: '',
					status: data.status as Download['status'],
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
				},
				...downloads,
			];
		}

		// Prevent unbounded growth — keep active + last 50 completed
		if (downloads.length > 100) {
			const active = downloads.filter(isActive);
			const rest = downloads.filter((d) => !isActive(d)).slice(0, 50);
			downloads = [...active, ...rest];
		}
	});
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
		downloads = [download, ...downloads];
	},

	addDownloads(newDownloads: Download[]) {
		downloads = [...newDownloads, ...downloads];
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
			const activeIds = new Set(active.map((d) => d.id));
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
