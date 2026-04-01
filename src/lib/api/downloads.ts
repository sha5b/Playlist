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

export async function cancelDownload(id: number): Promise<void> {
	return invoke('download_cancel', { id });
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
