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
	EnrichResult,
	EnrichAlbumResult,
	ScanMissingResult,
	MetadataStats,
	Download,
} from '$lib/types';

// --- Simple TTL cache for frequently accessed data ---
const CACHE_TTL = 30_000; // 30 seconds
const cache = new Map<string, { data: unknown; ts: number }>();

function getCached<T>(key: string): T | null {
	const entry = cache.get(key);
	if (entry && Date.now() - entry.ts < CACHE_TTL) return entry.data as T;
	return null;
}

function setCache(key: string, data: unknown) {
	cache.set(key, { data, ts: Date.now() });
}

/** Invalidate all cached data (call after imports, downloads, deletions) */
export function invalidateCache() {
	cache.clear();
}

export async function getLibraryStats(): Promise<LibraryStats> {
	const cached = getCached<LibraryStats>('stats');
	if (cached) return cached;
	const result: LibraryStats = await invoke('get_library_stats');
	setCache('stats', result);
	return result;
}

// --- Tracks ---

export async function getTracks(
	offset = 0,
	limit = 50,
	sortBy = 'date_added',
	sortDir = 'desc',
	search?: string
): Promise<TrackPage> {
	return invoke('library_get_tracks', {
		offset,
		limit,
		sortBy,
		sortDir,
		search: search || null,
	});
}

export async function getTrack(id: number): Promise<Track | null> {
	return invoke('library_get_track', { id });
}

export async function getGenres(): Promise<string[]> {
	const cached = getCached<string[]>('genres');
	if (cached) return cached;
	const result: string[] = await invoke('library_get_genres');
	setCache('genres', result);
	return result;
}

export async function getTracksByGenre(genre: string, limit = 20): Promise<Track[]> {
	return invoke('library_get_tracks_by_genre', { genre, limit });
}

export async function deleteTrack(id: number, deleteFile = false): Promise<void> {
	const result = await invoke<void>('library_delete_track', { id, deleteFile });
	invalidateCache();
	return result;
}

export async function resetLibrary(deleteFiles = true): Promise<void> {
	const result = await invoke<void>('library_reset', { deleteFiles });
	invalidateCache();
	return result;
}

// --- Albums ---

export async function getAlbums(
	offset = 0,
	limit = 50,
	search?: string
): Promise<[Album[], number]> {
	const key = `albums:${offset}:${limit}:${search || ''}`;
	const cached = getCached<[Album[], number]>(key);
	if (cached) return cached;
	const result: [Album[], number] = await invoke('library_get_albums', { offset, limit, search: search || null });
	setCache(key, result);
	return result;
}

export async function getAlbum(id: number): Promise<Album | null> {
	return invoke('library_get_album', { id });
}

// --- Artists ---

export async function getArtists(
	offset = 0,
	limit = 50,
	search?: string
): Promise<[Artist[], number]> {
	const key = `artists:${offset}:${limit}:${search || ''}`;
	const cached = getCached<[Artist[], number]>(key);
	if (cached) return cached;
	const result: [Artist[], number] = await invoke('library_get_artists', { offset, limit, search: search || null });
	setCache(key, result);
	return result;
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

export async function getPlaylistTracks(
	playlistId: number,
	offset: number,
	limit: number
): Promise<TrackPage> {
	return invoke('library_get_playlist_tracks', { playlistId, offset, limit });
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
	const result = await invoke<number>('library_import_folder', { path });
	invalidateCache();
	return result;
}

// --- Metadata Enrichment ---

export async function enrichAlbum(albumId: number): Promise<EnrichAlbumResult> {
	const result = await invoke<EnrichAlbumResult>('enrich_album', { albumId });
	invalidateCache();
	return result;
}

export async function enrichTrack(trackId: number): Promise<EnrichResult> {
	const result = await invoke<EnrichResult>('enrich_track', { trackId });
	invalidateCache();
	return result;
}

export async function downloadMusicVideo(trackId: number): Promise<string> {
	const result = await invoke<string>('download_music_video', { trackId });
	invalidateCache();
	return result;
}

export async function scanMissingMetadata(): Promise<ScanMissingResult> {
	const result = await invoke<ScanMissingResult>('scan_missing_metadata');
	invalidateCache();
	return result;
}

export async function getMetadataStats(): Promise<MetadataStats> {
	return invoke<MetadataStats>('get_metadata_stats');
}

export async function stopMetadataScan(): Promise<void> {
	return invoke('metadata_stop_scan');
}

export async function deleteAllMetadata(): Promise<void> {
	await invoke('metadata_delete_all');
	invalidateCache();
}

export interface CleanupResult {
	merged_album_groups: number;
	deleted_duplicate_albums: number;
	orphaned_albums_removed: number;
	merged_track_groups: number;
	deleted_duplicate_tracks: number;
}

export async function cleanupDuplicateAlbums(): Promise<CleanupResult> {
	const result = await invoke('metadata_cleanup_duplicates');
	invalidateCache();
	return result as CleanupResult;
}

export interface TrackCleanupResult {
	merged_track_groups: number;
	deleted_duplicate_tracks: number;
}

export async function cleanupDuplicateTracks(): Promise<TrackCleanupResult> {
	const result = await invoke('metadata_cleanup_duplicate_tracks');
	invalidateCache();
	return result as TrackCleanupResult;
}

// --- Album Download Status ---

export interface AlbumDownloadStatus {
	total_expected: number;
	total_local: number;
	status: 'complete' | 'partial' | 'none' | 'unknown';
}

export async function getAlbumDownloadStatus(albumId: number): Promise<AlbumDownloadStatus> {
	return invoke('library_get_album_download_status', { albumId });
}

export async function getAlbumsDownloadStatus(albumIds: number[]): Promise<Record<string, AlbumDownloadStatus>> {
	return invoke('library_get_albums_download_status', { albumIds });
}

// --- Artist Enrichment ---

export interface ArtistDiscographyEntry {
	mbid: string;
	title: string;
	album_type: string | null;
	year: number | null;
}

export interface EnrichArtistResult {
	artist_id: number;
	mbid: string;
	total_releases: number;
	discography: ArtistDiscographyEntry[];
}

export interface ArtistMissingResult {
	missing: ArtistDiscographyEntry[];
	total_discography: number;
}

export async function enrichArtist(artistId: number): Promise<EnrichArtistResult> {
	const result = await invoke<EnrichArtistResult>('enrich_artist', { artistId });
	invalidateCache();
	return result;
}

export async function getArtistMissingAlbums(artistId: number): Promise<ArtistMissingResult> {
	return invoke('library_get_artist_missing_albums', { artistId });
}

export async function downloadArtistMissing(artistId: number, albumMbids: string[]): Promise<Download[]> {
	return invoke('download_artist_missing', { artistId, albumMbids });
}
