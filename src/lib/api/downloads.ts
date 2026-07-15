import { invoke } from '@tauri-apps/api/core';
import type { Download, UrlInfo, DepsStatus } from '$lib/types';

export async function parseUrl(url: string): Promise<UrlInfo> {
	return invoke('download_parse_url', { url });
}

export async function checkDeps(): Promise<DepsStatus> {
	return invoke('download_check_deps');
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

export async function startBatchDownload(
	urls: string[],
	format?: string,
	quality?: string
): Promise<Download[]> {
	return invoke('download_start_batch', { urls, format, quality });
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
	return invoke('download_search_and_start', {
		query, title, artist, format, quality,
		album_id: albumId, artist_id: artistId,
		disc_number: discNumber, track_number: trackNumber,
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

// --- Download Sources ---

export interface SourceStatus {
	platform: string;
	configured: boolean;
	available: boolean;
	max_quality: string;
	error?: string;
}

export async function getSourcesStatus(): Promise<SourceStatus[]> {
	return invoke('download_get_sources_status');
}

export async function setSourceCredentials(
	platform: string,
	credentials: Record<string, string>
): Promise<void> {
	return invoke('download_set_source_credentials', { platform, credentials });
}

export async function testSource(
	platform: string
): Promise<void> {
	return invoke('download_test_source', { platform });
}

export async function refreshSources(): Promise<void> {
	return invoke('download_refresh_sources');
}
