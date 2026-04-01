import { listen } from '@tauri-apps/api/event';
import type { Download, DownloadEvent } from '$lib/types';
import { getActiveDownloads } from '$lib/api/downloads';

let downloads: Download[] = $state([]);
let initialized = false;

async function init() {
	if (initialized) return;
	initialized = true;

	// Load active downloads
	try {
		downloads = await getActiveDownloads();
	} catch {
		// Ignore on first load
	}

	// Listen for download events from Rust
	listen<DownloadEvent>('download-event', (event) => {
		const data = event.payload;
		const idx = downloads.findIndex((d) => d.id === data.id);

		if (idx >= 0) {
			const dl = downloads[idx];
			downloads[idx] = {
				...dl,
				status: data.status as Download['status'],
				progress: data.progress,
				error_message: data.error ?? dl.error_message,
				title: data.title ?? dl.title,
				track_id: data.track_id ?? dl.track_id,
			};
			// Trigger reactivity
			downloads = [...downloads];
		}
	});
}

export const downloadStore = {
	get downloads() {
		return downloads;
	},

	get activeCount() {
		return downloads.filter(
			(d) => d.status === 'queued' || d.status === 'downloading' || d.status === 'processing'
		).length;
	},

	init,

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
		downloads = downloads.filter(
			(d) => d.status === 'queued' || d.status === 'downloading' || d.status === 'processing'
		);
	},
};
