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
}

export interface Artist {
	id: number;
	name: string;
	sort_name: string | null;
	musicbrainz_id: string | null;
	image_path: string | null;
	bio: string | null;
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

export interface Download {
	id: number;
	url: string;
	title: string | null;
	artist: string | null;
	platform: string;
	status: string;
	progress: number;
	error_message: string | null;
	file_path: string | null;
	track_id: number | null;
	format: string | null;
	quality: string | null;
	created_at: string;
}

export interface LibraryStats {
	total_tracks: number;
	total_albums: number;
	total_artists: number;
	total_playlists: number;
	total_duration_ms: number;
	total_size_bytes: number;
}
