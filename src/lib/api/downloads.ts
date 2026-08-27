import { invoke } from '@tauri-apps/api/core';
import type { Download, DepsStatus } from '$lib/types';

export async function checkDeps(): Promise<DepsStatus> {
	return invoke('download_check_deps');
}

/** Resolved default download dir (OS music folder, e.g. ~/Music/Playlist). */
export async function getDefaultDownloadDir(): Promise<string> {
	return invoke('download_default_dir');
}

export async function ensureDeps(): Promise<void> {
	return invoke('download_ensure_deps');
}

export async function startDownload(
	url: string,
	format?: string,
	quality?: string
): Promise<Download> {
	return invoke('download_start', { url, format, quality });
}

export interface SearchDownloadRequest {
	query: string;
	title?: string;
	artist?: string;
	album_id?: number;
	artist_id?: number;
}

export async function searchAndDownload(
	query: string,
	title?: string,
	artist?: string,
	format?: string,
	quality?: string,
	albumId?: number,
	artistId?: number,
	discNumber?: number,
	trackNumber?: number
): Promise<Download> {
	// Tauri exposes snake_case Rust args as camelCase — snake_case keys were
	// silently dropped (all Option fields), losing the album/artist linkage.
	return invoke('download_search_and_start', {
		query, title, artist, format, quality,
		albumId, artistId,
		discNumber, trackNumber,
	});
}

export async function searchAndDownloadBatch(
	queries: SearchDownloadRequest[],
	format?: string,
	quality?: string
): Promise<Download[]> {
	return invoke('download_search_and_start_batch', { queries, format, quality });
}

export async function cancelDownload(id: number): Promise<void> {
	return invoke('download_cancel', { id });
}

/** Cancel every active/queued download across all playlists. */
export async function stopAllDownloads(): Promise<void> {
	return invoke('download_cancel_all');
}

export async function retryDownload(id: number): Promise<Download> {
	return invoke('download_retry', { id });
}

export async function getActiveDownloads(): Promise<Download[]> {
	return invoke('download_get_active');
}

export async function getDownloadHistory(
	offset = 0,
	limit = 50
): Promise<[Download[], number]> {
	return invoke('download_get_history', { offset, limit });
}

export async function clearHistory(): Promise<number> {
	return invoke('download_clear_history');
}

