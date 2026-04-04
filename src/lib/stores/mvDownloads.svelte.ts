import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { downloadMusicVideo } from '$lib/api/library';
import { toast } from 'svelte-sonner';

interface MvDownload {
	trackId: number;
	progress: number;
}

let active: MvDownload[] = $state([]);
let unlisten: UnlistenFn | null = null;
let initialized = false;

async function init() {
	if (initialized) return;
	initialized = true;
	try {
		unlisten = await listen<{ track_id: number; percent: number }>(
			'music-video-download-progress',
			(event) => {
				const idx = active.findIndex((d) => d.trackId === event.payload.track_id);
				if (idx >= 0) {
					active[idx].progress = Math.round(event.payload.percent);
				}
			},
		);
	} catch (e) {
		console.error('Failed to register MV download listener:', e);
		initialized = false;
	}
}

function start(trackId: number) {
	if (active.some((d) => d.trackId === trackId)) return;
	active = [...active, { trackId, progress: 0 }];

	// Fire-and-forget — runs in background even if user navigates away
	downloadMusicVideo(trackId)
		.then(() => {
			toast.success('Music video downloaded');
		})
		.catch((e) => {
			toast.error('Failed to download music video', { description: String(e) });
		})
		.finally(() => {
			active = active.filter((d) => d.trackId !== trackId);
		});
}

function isDownloading(trackId: number): boolean {
	return active.some((d) => d.trackId === trackId);
}

function getProgress(trackId: number): number {
	return active.find((d) => d.trackId === trackId)?.progress ?? 0;
}

function destroy() {
	if (unlisten) {
		unlisten();
		unlisten = null;
	}
	initialized = false;
}

export const mvDownloadStore = {
	get active() {
		return active;
	},
	init,
	destroy,
	start,
	isDownloading,
	getProgress,
};
