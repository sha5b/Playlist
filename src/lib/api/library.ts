import { invoke } from '@tauri-apps/api/core';
import type {
	LibraryStats,
	TrackPage,
	Track,
	Album,
	Artist,
	Playlist,
	PlaylistDetail,
	SearchResults,
} from '$lib/types';

export async function greet(name: string): Promise<string> {
	return invoke('greet', { name });
}

export async function getLibraryStats(): Promise<LibraryStats> {
	return invoke('get_library_stats');
}

// --- Tracks ---

export async function getTracks(
	offset = 0,
	limit = 50,
	sortBy = 'date_added',
	sortDir = 'desc'
): Promise<TrackPage> {
	return invoke('library_get_tracks', {
		offset,
		limit,
		sortBy,
		sortDir,
	});
}

export async function getTrack(id: number): Promise<Track | null> {
	return invoke('library_get_track', { id });
}

export async function deleteTrack(id: number, deleteFile = false): Promise<void> {
	return invoke('library_delete_track', { id, deleteFile });
}

// --- Albums ---

export async function getAlbums(
	offset = 0,
	limit = 50
): Promise<[Album[], number]> {
	return invoke('library_get_albums', { offset, limit });
}

export async function getAlbum(id: number): Promise<Album | null> {
	return invoke('library_get_album', { id });
}

// --- Artists ---

export async function getArtists(
	offset = 0,
	limit = 50
): Promise<[Artist[], number]> {
	return invoke('library_get_artists', { offset, limit });
}

export async function getArtist(id: number): Promise<Artist | null> {
	return invoke('library_get_artist', { id });
}

// --- Playlists ---

export async function getPlaylists(): Promise<Playlist[]> {
	return invoke('library_get_playlists');
}

export async function getPlaylist(id: number): Promise<PlaylistDetail | null> {
	return invoke('library_get_playlist', { id });
}

export async function createPlaylist(
	name: string,
	description?: string
): Promise<Playlist> {
	return invoke('library_create_playlist', { name, description });
}

export async function updatePlaylist(
	id: number,
	name?: string,
	description?: string
): Promise<Playlist> {
	return invoke('library_update_playlist', { id, name, description });
}

export async function deletePlaylist(id: number): Promise<void> {
	return invoke('library_delete_playlist', { id });
}

export async function addToPlaylist(
	playlistId: number,
	trackIds: number[]
): Promise<void> {
	return invoke('library_add_to_playlist', { playlistId, trackIds });
}

export async function removeFromPlaylist(
	playlistId: number,
	trackId: number
): Promise<void> {
	return invoke('library_remove_from_playlist', { playlistId, trackId });
}

export async function reorderPlaylist(
	playlistId: number,
	from: number,
	to: number
): Promise<void> {
	return invoke('library_reorder_playlist', { playlistId, from, to });
}

// --- Detail Pages ---

export async function getAlbumTracks(albumId: number): Promise<Track[]> {
	return invoke('library_get_album_tracks', { albumId });
}

export async function getArtistTracks(artistId: number): Promise<Track[]> {
	return invoke('library_get_artist_tracks', { artistId });
}

export async function getArtistAlbums(artistId: number): Promise<Album[]> {
	return invoke('library_get_artist_albums', { artistId });
}

// --- Settings ---

export async function getSetting(key: string): Promise<string | null> {
	return invoke('settings_get', { key });
}

export async function setSetting(key: string, value: string): Promise<void> {
	return invoke('settings_set', { key, value });
}

export async function getAllSettings(): Promise<[string, string][]> {
	return invoke('settings_get_all');
}

// --- Search ---

export async function search(
	query: string,
	limit?: number
): Promise<SearchResults> {
	return invoke('search', { query, limit });
}

// --- Import ---

export async function importFolder(path: string): Promise<number> {
	return invoke('library_import_folder', { path });
}
