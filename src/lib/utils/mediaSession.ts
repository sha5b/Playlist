import { assetUrl } from './format';
import type { QueueTrack } from '$lib/types';

/**
 * Integrates with the browser/OS Media Session API.
 * Provides: media key support (play/pause/next/prev), OS media overlay,
 * lock screen controls, and metadata display.
 */

type PlayerActions = {
	togglePlayPause: () => void;
	next: () => void;
	prev: () => void;
	seek: (seconds: number) => void;
};

let actions: PlayerActions | null = null;

export function initMediaSession(playerActions: PlayerActions) {
	if (!('mediaSession' in navigator)) return;
	actions = playerActions;

	navigator.mediaSession.setActionHandler('play', () => actions?.togglePlayPause());
	navigator.mediaSession.setActionHandler('pause', () => actions?.togglePlayPause());
	navigator.mediaSession.setActionHandler('previoustrack', () => actions?.prev());
	navigator.mediaSession.setActionHandler('nexttrack', () => actions?.next());
	navigator.mediaSession.setActionHandler('seekto', (details) => {
		if (details.seekTime != null) {
			actions?.seek(details.seekTime);
		}
	});
}

export function updateMediaSessionMetadata(track: QueueTrack | null) {
	if (!('mediaSession' in navigator) || !track) {
		if ('mediaSession' in navigator) {
			navigator.mediaSession.metadata = null;
		}
		return;
	}

	const artwork: MediaImage[] = [];
	if (track.cover_art_path) {
		artwork.push({ src: assetUrl(track.cover_art_path), sizes: '512x512', type: 'image/jpeg' });
	}

	navigator.mediaSession.metadata = new MediaMetadata({
		title: track.title,
		artist: track.artist_name ?? 'Unknown Artist',
		album: track.album_title ?? '',
		artwork,
	});
}

export function updateMediaSessionPlaybackState(isPlaying: boolean) {
	if (!('mediaSession' in navigator)) return;
	navigator.mediaSession.playbackState = isPlaying ? 'playing' : 'paused';
}

export function updateMediaSessionPosition(positionMs: number, durationMs: number) {
	if (!('mediaSession' in navigator) || !navigator.mediaSession.setPositionState) return;
	if (durationMs <= 0) return;

	try {
		navigator.mediaSession.setPositionState({
			duration: durationMs / 1000,
			position: Math.min(positionMs / 1000, durationMs / 1000),
			playbackRate: 1,
		});
	} catch {
		// Ignore invalid state errors
	}
}
