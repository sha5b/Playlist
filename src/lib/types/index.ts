export interface Track {
	id: number;
	title: string;
	artist_id: number | null;
	album_id: number | null;
	album_artist: string | null;
	duration_ms: number | null;
	track_number: number | null;
	disc_number: number | null;
	genre: string | null;
	year: number | null;
	file_path: string;
	file_size: number | null;
	format: string | null;
	bitrate: number | null;
	sample_rate: number | null;
	channels: number | null;
	cover_art_path: string | null;
	source_platform: string | null;
	source_url: string | null;
	play_count: number;
	last_played_at: string | null;
	date_added: string;
	description: string | null;
	label: string | null;
	release_date: string | null;
	composer: string | null;
	language: string | null;
	metadata_completeness: number;
	tags: string | null;
	lyrics: string | null;
	music_video_url: string | null;
	music_video_path: string | null;
	artist_name: string | null;
	album_title: string | null;
}

export interface TrackPage {
	tracks: Track[];
	total: number;
}

export interface Artist {
	id: number;
	name: string;
	sort_name: string | null;
	musicbrainz_id: string | null;
	image_path: string | null;
	bio: string | null;
	country: string | null;
	begin_year: number | null;
	artist_type: string | null;
	website_url: string | null;
	enriched_discography: string | null;
	track_count: number;
}

export interface Album {
	id: number;
	title: string;
	artist_id: number | null;
	album_artist: string | null;
	year: number | null;
	genre: string | null;
	total_tracks: number | null;
	total_discs: number | null;
	musicbrainz_id: string | null;
	cover_art_path: string | null;
	label: string | null;
	release_date: string | null;
	description: string | null;
	album_type: string | null;
	enriched_tracklist: string | null;
	purchase_url: string | null;
	artist_name: string | null;
	track_count: number;
}

export interface Playlist {
	id: number;
	name: string;
	description: string | null;
	cover_art_path: string | null;
	source_platform: string | null;
	source_url: string | null;
	track_count: number;
	total_duration_ms: number;
	is_synced: boolean;
	last_synced_at: string | null;
	created_at: string;
}

export interface PlaylistDetail {
	playlist: Playlist;
	tracks: Track[];
}

export interface LibraryStats {
	total_tracks: number;
	total_albums: number;
	total_artists: number;
	total_playlists: number;
	total_duration_ms: number;
	total_size_bytes: number;
}

export interface SearchResults {
	tracks: Track[];
	albums: Album[];
	artists: Artist[];
}

// --- Player Types ---

export interface QueueTrack {
	id: number;
	title: string;
	artist_name: string | null;
	album_title: string | null;
	duration_ms: number | null;
	file_path: string;
	cover_art_path: string | null;
}

export type PlayerStateEnum = 'stopped' | 'playing' | 'paused';
export type RepeatMode = 'off' | 'all' | 'one';

export interface PlaybackState {
	state: PlayerStateEnum;
	current_track: QueueTrack | null;
	position_ms: number;
	duration_ms: number;
	volume: number;
	shuffle: boolean;
	repeat: RepeatMode;
	queue_length: number;
	queue_position: number | null;
}

export type PlayerEvent =
	| { kind: 'state_changed'; data: PlaybackState }
	| { kind: 'track_changed'; data: QueueTrack | null }
	| { kind: 'progress'; data: { position_ms: number; duration_ms: number } }
	| { kind: 'queue_updated'; data: { tracks: QueueTrack[]; position: number | null } }
	| { kind: 'error'; data: string };

// --- Download Types ---

export type DownloadStatus = 'queued' | 'downloading' | 'processing' | 'completed' | 'failed' | 'cancelled';

export interface Download {
	id: number;
	url: string;
	title: string | null;
	artist: string | null;
	platform: string;
	status: DownloadStatus;
	progress: number;
	error_message: string | null;
	file_path: string | null;
	track_id: number | null;
	playlist_id: number | null;
	format: string;
	quality: string;
	created_at: string;
	started_at: string | null;
	completed_at: string | null;
	target_album_id: number | null;
	target_artist_id: number | null;
	target_isrc: string | null;
}

export interface UrlInfo {
	platform: string;
	url_type: string;
	clean_url: string;
	title: string | null;
}

export interface DownloadEvent {
	id: number;
	status: DownloadStatus;
	progress: number;
	speed: string | null;
	eta: string | null;
	error: string | null;
	title: string | null;
	track_id: number | null;
}

export interface DepsStatus {
	ytdlp_available: boolean;
	ffmpeg_available: boolean;
	ytdlp_version: string | null;
	ytdlp_path: string | null;
	ffmpeg_path: string | null;
}

export interface SetupProgress {
	step: string;
	status: string;
	progress: number;
	message: string;
}

// --- Manager Types ---

export type MonitoredEntryStatus = 'new' | 'queued' | 'downloading' | 'downloaded' | 'failed' | 'skipped';

export interface MonitoredPlaylist {
	id: number;
	name: string;
	description: string | null;
	cover_art_path: string | null;
	source_platform: string | null;
	source_url: string | null;
	source_id: string | null;
	track_count: number;
	total_duration_ms: number;
	is_synced: boolean;
	last_synced_at: string | null;
	created_at: string;
	new_count: number;
	downloaded_count: number;
	active_count: number;
	total_entries: number;
}

export interface MonitoredEntry {
	id: number;
	playlist_id: number;
	source_url: string;
	title: string | null;
	artist: string | null;
	duration_seconds: number | null;
	thumbnail: string | null;
	status: MonitoredEntryStatus;
	download_id: number | null;
	track_id: number | null;
	position: number;
	first_seen_at: string;
	downloaded_at: string | null;
}

export interface SyncResult {
	playlist_id: number;
	new_count: number;
	total_count: number;
}

export interface BatchDownloadResult {
	queued: number;
	playlist_id: number;
}

export interface ManagerEntryEvent {
	entry_id: number;
	status: string;
}

// --- Metadata Enrichment Types ---

export interface EnrichResult {
	track_id: number;
	fields_updated: number;
	completeness: number;
}

export interface EnrichAlbumResult {
	album_id: number;
	fields_updated: number;
	tracks_added: number;
	tracklist: AlbumTrackInfo[];
}

export interface AlbumTrackInfo {
	disc_number: number;
	track_number: number;
	title: string;
	duration_ms: number | null;
}

export interface ScanMissingResult {
	total_tracks: number;
	enriched: number;
	failed: number;
	completeness_avg: number;
}

export interface MetadataStats {
	total_tracks: number;
	average_completeness: number;
	complete_tracks: number;
	incomplete_tracks: number;
}

export interface MetadataScanProgress {
	current: number;
	total: number;
	track_title: string;
}
