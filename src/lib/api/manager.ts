import { invoke } from '@tauri-apps/api/core';
import type { Download, MonitoredPlaylist, MonitoredEntry, SyncResult, BatchDownloadResult } from '$lib/types';

export async function getMonitoredPlaylists(): Promise<MonitoredPlaylist[]> {
	return invoke('manager_get_playlists');
}

export async function getEntries(playlistId: number): Promise<MonitoredEntry[]> {
	return invoke('manager_get_entries', { playlistId });
}

export async function getNewEntries(playlistId: number): Promise<MonitoredEntry[]> {
	return invoke('manager_get_new_entries', { playlistId });
}

export async function addPlaylist(url: string): Promise<MonitoredPlaylist> {
	return invoke('manager_add_playlist', { url });
}

export async function syncPlaylist(playlistId: number): Promise<SyncResult> {
	return invoke('manager_sync_playlist', { playlistId });
}

export async function downloadEntry(
	entryId: number,
	format?: string,
	quality?: string
): Promise<Download> {
	return invoke('manager_download_entry', { entryId, format, quality });
}

export async function downloadNew(
	playlistId: number,
	format?: string,
	quality?: string
): Promise<BatchDownloadResult> {
	return invoke('manager_download_new', { playlistId, format, quality });
}

export async function skipEntry(entryId: number): Promise<void> {
	return invoke('manager_skip_entry', { entryId });
}

export async function cancelEntry(entryId: number): Promise<void> {
	return invoke('manager_cancel_entry', { entryId });
}

export async function cancelAllDownloads(playlistId: number): Promise<number> {
	return invoke('manager_cancel_all', { playlistId });
}

export async function removePlaylist(playlistId: number): Promise<void> {
	return invoke('manager_remove_playlist', { playlistId });
}
