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

/** Editable tag fields for the tag editor. Omitted/blank fields keep the current value. */
export interface TrackTagUpdate {
	title?: string;
	artist?: string;
	album?: string;
	album_artist?: string;
	genre?: string;
	year?: number;
	track_number?: number;
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
	has_enriched_discography: boolean;
	track_count: number;
	/** Cover art of one of the artist's albums — fallback when image_path is null. */
	fallback_cover_path: string | null;
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
	/** Rule-based auto-updating playlist (tracks computed from `rules`). */
	is_smart: boolean;
	/** JSON-encoded SmartRules for smart playlists. */
	rules: string | null;
}

// --- Smart Playlist Rules ---

export type SmartRuleField =
	| 'title'
	| 'artist'
	| 'album'
	| 'genre'
	| 'format'
	| 'year'
	| 'duration_ms'
	| 'play_count'
	| 'last_played_at'
	| 'created_at';

export type SmartRuleOp =
	| 'contains'
	| 'equals'
	| 'not_equals'
	| 'gt'
	| 'lt'
	| 'in_last_days'
	| 'not_in_last_days'
	| 'is_null';

export interface SmartRule {
	field: SmartRuleField;
	op: SmartRuleOp;
	value?: string | number | null;
}

export interface SmartRules {
	match: 'all' | 'any';
	rules: SmartRule[];
	sort?: { field: SmartRuleField; dir: 'asc' | 'desc' } | null;
	limit?: number | null;
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
	album_id?: number | null;
	artist_id?: number | null;
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
	target_disc_number: number | null;
	target_track_number: number | null;
	target_duration_ms: number | null;
	target_album_name: string | null;
	target_recording_mbid: string | null;
	/** Live transfer stats from download events (not persisted in the DB). */
	speed?: string | null;
	eta?: string | null;
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

// --- Device Sync Types ---

export interface ScannedDevice {
	device_uid: string;
	name: string;
	mount_path: string;
	capacity_bytes: number | null;
	free_bytes: number | null;
	vendor: string | null;
	model: string | null;
	is_known: boolean;
	device_id: number | null;
}

export interface Device {
	id: number;
	device_uid: string;
	name: string;
	device_type: string;
	mount_path: string | null;
	capacity_bytes: number | null;
	music_dir: string;
	output_format: string;
	output_bitrate: string;
	generate_m3u: boolean;
	first_seen_at: string;
	last_seen_at: string;
}

export interface DevicePlaylistLink {
	playlist_id: number;
	playlist_name: string;
	enabled: boolean;
	last_synced_at: string | null;
	total_tracks: number;
	synced_tracks: number;
	pending_changes: number;
}

export interface DeviceDetail {
	device: Device;
	playlists: DevicePlaylistLink[];
	synced_track_count: number;
}

export interface DeviceSyncProgress {
	device_id: number;
	playlist_id: number;
	current: number;
	total: number;
	track_title: string;
	status: 'copying' | 'converting' | 'generating_playlist' | 'done' | 'error' | 'cancelled';
	error: string | null;
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
	/** Size of the discovered canonical tracklist (not tracks inserted into the library). */
	tracklist_size: number;
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

export interface TrackMismatch {
	track_id: number;
	track_title: string;
	album_title: string;
	album_id: number;
	reasons: string[];
	track_genre: string | null;
	album_genre: string | null;
	track_artist: string | null;
	album_artist: string | null;
}

export interface M3uImportResult {
	playlist_id: number;
	playlist_name: string;
	matched: number;
	unmatched: number;
	unmatched_entries: string[];
}

// ── Listening stats ─────────────────────────────────────────────────────

export interface DayPlays {
	day: string;
	count: number;
}

export interface StatsOverview {
	total_plays: number;
	total_listening_ms: number;
	distinct_tracks: number;
	distinct_artists: number;
	distinct_albums: number;
	plays_per_day: DayPlays[];
}

export interface TopTrack {
	id: number;
	title: string;
	artist_id: number | null;
	artist_name: string | null;
	cover_art_path: string | null;
	play_count: number;
}

export interface TopArtist {
	id: number;
	name: string;
	image_path: string | null;
	play_count: number;
}

export interface TopAlbum {
	id: number;
	title: string;
	artist_name: string | null;
	cover_art_path: string | null;
	play_count: number;
}

export interface StatsTop {
	tracks: TopTrack[];
	artists: TopArtist[];
	albums: TopAlbum[];
}

export type StatsPeriod = 'week' | 'month' | 'year' | 'all';

// ── Last.fm scrobbling ──────────────────────────────────────────────────

export interface LastfmAuth {
	token: string;
	url: string;
}

export interface LastfmStatus {
	connected: boolean;
	username: string | null;
	scrobbling_enabled: boolean;
	pending_scrobbles: number;
}
