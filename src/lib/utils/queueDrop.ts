import { getAlbumTracks, getArtistTracks } from '$lib/api/library';
import { player } from '$lib/stores/player.svelte';

/** Supported drag-and-drop data types for queue operations */
export const DRAG_TYPES = [
	'application/x-playlist-track',
	'application/x-playlist-album',
	'application/x-playlist-artist',
] as const;

/** Check if a DataTransfer contains any of our supported drag types */
export function hasDragData(dt: DataTransfer | null): boolean {
	if (!dt) return false;
	const types = Array.from(dt.types);
	return DRAG_TYPES.some((t) => types.includes(t));
}

export interface QueueDropResult {
	success: boolean;
	type: 'track' | 'album' | 'artist' | null;
	trackIds: number[];
}

/**
 * Process a drop event and add tracks to the queue.
 * Handles track, album, and artist drops.
 * If queue is empty or stopped, starts playback; otherwise adds to queue.
 */
export async function processQueueDrop(
	dataTransfer: DataTransfer | null
): Promise<QueueDropResult> {
	if (!dataTransfer) {
		return { success: false, type: null, trackIds: [] };
	}

	const shouldStartPlayback = player.queueTracks.length === 0 || player.isStopped;

	// Track drop
	const trackData = dataTransfer.getData('application/x-playlist-track');
	if (trackData) {
		try {
			const { trackId } = JSON.parse(trackData);
			if (shouldStartPlayback) {
				await player.playTrack(trackId);
			} else {
				await player.addToQueue(trackId);
			}
			return { success: true, type: 'track', trackIds: [trackId] };
		} catch {
			return { success: false, type: 'track', trackIds: [] };
		}
	}

	// Album drop
	const albumData = dataTransfer.getData('application/x-playlist-album');
	if (albumData) {
		try {
			const { albumId } = JSON.parse(albumData);
			const tracks = await getAlbumTracks(albumId);
			if (tracks.length === 0) {
				return { success: false, type: 'album', trackIds: [] };
			}
			const ids = tracks.map((t) => t.id);
			if (shouldStartPlayback) {
				await player.playTracks(ids, 0);
			} else {
				for (const id of ids) {
					await player.addToQueue(id);
				}
			}
			return { success: true, type: 'album', trackIds: ids };
		} catch {
			return { success: false, type: 'album', trackIds: [] };
		}
	}

	// Artist drop
	const artistData = dataTransfer.getData('application/x-playlist-artist');
	if (artistData) {
		try {
			const { artistId } = JSON.parse(artistData);
			const tracks = await getArtistTracks(artistId);
			if (tracks.length === 0) {
				return { success: false, type: 'artist', trackIds: [] };
			}
			const ids = tracks.map((t) => t.id);
			if (shouldStartPlayback) {
				await player.playTracks(ids, 0);
			} else {
				for (const id of ids) {
					await player.addToQueue(id);
				}
			}
			return { success: true, type: 'artist', trackIds: ids };
		} catch {
			return { success: false, type: 'artist', trackIds: [] };
		}
	}

	return { success: false, type: null, trackIds: [] };
}

/** Common drag over handler - prevents default and sets drop effect if valid drag data */
export function handleQueueDragOver(e: DragEvent): boolean {
	if (hasDragData(e.dataTransfer)) {
		e.preventDefault();
		e.dataTransfer!.dropEffect = 'copy';
		return true;
	}
	return false;
}
