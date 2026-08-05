import { listen } from '@tauri-apps/api/event';
import * as playerApi from '$lib/api/player';
import { toast } from 'svelte-sonner';
import type { PlaybackState, QueueTrack, PlayerEvent, RepeatMode, PlayerStateEnum } from '$lib/types';
import {
	initMediaSession,
	updateMediaSessionMetadata,
	updateMediaSessionPlaybackState,
	updateMediaSessionPosition,
} from '$lib/utils/mediaSession';

// --- Reactive state ---
let state: PlayerStateEnum = $state('stopped');
let currentTrack: QueueTrack | null = $state(null);
let positionMs: number = $state(0);
let durationMs: number = $state(0);
let volume: number = $state(0.75);
let shuffle: boolean = $state(false);
let repeat: RepeatMode = $state('off');
let queueTracks: QueueTrack[] = $state([]);
let queuePosition: number | null = $state(null);
let queueOpen: boolean = $state(false);
let autoplay: boolean = $state(false);
let previousTrack: QueueTrack | null = $state(null);
let playedHistory: Set<number> = new Set();

// --- Event listener setup ---
let initialized = false;
let unlisten: (() => void) | null = null;

// Throttle progress updates to display refresh rate to avoid unnecessary re-renders
let progressRafPending = false;
let pendingPositionMs = 0;
let pendingDurationMs = 0;

function handleEvent(event: PlayerEvent) {
	switch (event.kind) {
		case 'state_changed': {
			const prevState = state;
			state = event.data.state;
			currentTrack = event.data.current_track;
			positionMs = event.data.position_ms;
			durationMs = event.data.duration_ms;
			// Also update the pending values so a queued rAF from a pre-seek
			// progress event doesn't overwrite position with stale data.
			pendingPositionMs = positionMs;
			pendingDurationMs = durationMs;
			volume = event.data.volume;
			shuffle = event.data.shuffle;
			repeat = event.data.repeat;
			queuePosition = event.data.queue_position;
			updateMediaSessionPlaybackState(state === 'playing');
			updateMediaSessionMetadata(currentTrack);
			// Autoplay: when playback stops naturally, start a new random track
			if (autoplay && prevState === 'playing' && state === 'stopped') {
				handleAutoplay();
			}
			// Also check if we should proactively queue the next track
			checkAutoplayQueue();
			checkShuffleQueue();
			break;
		}
		case 'track_changed':
			// Save outgoing track as previous (only if it's a different track)
			if (currentTrack && (!event.data || event.data.id !== currentTrack.id)) {
				previousTrack = currentTrack;
			}
			currentTrack = event.data;
			if (event.data) {
				positionMs = 0;
				durationMs = event.data.duration_ms ?? 0;
				// Keep pending values in sync so a queued rAF from the previous
				// track's progress event doesn't restore a stale position.
				pendingPositionMs = 0;
				pendingDurationMs = durationMs;
				playedHistory.add(event.data.id);
				// Record the play in the database (fire-and-forget)
				playerApi.recordPlay(event.data.id).catch(() => {});
			}
			updateMediaSessionMetadata(currentTrack);
			break;
		case 'progress':
			pendingPositionMs = event.data.position_ms;
			pendingDurationMs = event.data.duration_ms;
			if (!progressRafPending) {
				progressRafPending = true;
				requestAnimationFrame(() => {
					positionMs = pendingPositionMs;
					durationMs = pendingDurationMs;
					updateMediaSessionPosition(positionMs, durationMs);
					progressRafPending = false;
				});
			}
			break;
		case 'queue_updated':
			queueTracks = event.data.tracks;
			queuePosition = event.data.position;
			checkAutoplayQueue();
			checkShuffleQueue();
			break;
		case 'error':
			console.error('Player error:', event.data);
			toast.error('Playback error', { description: String(event.data) });
			break;
	}
}

async function init() {
	if (initialized) return;
	initialized = true;

	// Register OS media session (media keys, overlay, lock screen)
	initMediaSession({
		togglePlayPause: () => player.togglePlayPause(),
		next: () => player.next(),
		prev: () => player.prev(),
		seek: (seconds) => player.seek(seconds),
	});

	const fn = await listen<PlayerEvent>('player-event', (e) => handleEvent(e.payload));
	if (!initialized) {
		// destroy() ran while listen() was resolving — remove immediately so a
		// later init() doesn't stack a second handler.
		fn();
		return;
	}
	unlisten = fn;
	// Fetch initial state
	try {
		const s = await playerApi.getState();
		state = s.state;
		currentTrack = s.current_track;
		positionMs = s.position_ms;
		durationMs = s.duration_ms;
		volume = s.volume;
		shuffle = s.shuffle;
		repeat = s.repeat;
		queuePosition = s.queue_position;

		const [tracks, pos] = await playerApi.getQueue();
		queueTracks = tracks;
		queuePosition = pos;
	} catch {
		// Engine may not be ready yet
	}
}

let autoplayPending = false; // prevent double-adding

async function handleAutoplay() {
	if (autoplayPending) return;
	autoplayPending = true;
	try {
		const excludeIds = Array.from(playedHistory);
		let ids = await playerApi.getRandomTracks(excludeIds, 1);
		if (ids.length === 0) {
			// All tracks played, reset history and try again
			playedHistory.clear();
			ids = await playerApi.getRandomTracks([], 1);
		}
		if (ids.length > 0) {
			if (state === 'stopped') {
				await playerApi.playTracks(ids, 0);
			} else {
				await playerApi.addToQueue(ids[0]);
			}
		}
	} catch (e) {
		console.warn('Autoplay failed:', e);
	} finally {
		autoplayPending = false;
	}
}

function checkAutoplayQueue() {
	if (!autoplay || autoplayPending) return;
	if (state !== 'playing' && state !== 'paused') return;
	// If we're on the last track in the queue, proactively add the next one
	if (queuePosition !== null && queuePosition >= queueTracks.length - 1) {
		handleAutoplay();
	}
}

// --- Shuffle auto-refill ---
// With shuffle on, the queue should never run dry: whenever playback
// approaches the end of the queue, append more random tracks from the library.
const SHUFFLE_REFILL_BATCH = 25;
let shuffleRefillPending = false;

async function refillShuffleQueue() {
	if (shuffleRefillPending) return;
	shuffleRefillPending = true;
	try {
		// Avoid queuing tracks that are already in the queue or recently played
		const excludeIds = Array.from(new Set([
			...queueTracks.map((t) => t.id),
			...playedHistory,
		]));
		let ids = await playerApi.getRandomTracks(excludeIds, SHUFFLE_REFILL_BATCH);
		if (ids.length === 0) {
			// Library smaller than the queue/history — allow repeats
			playedHistory.clear();
			ids = await playerApi.getRandomTracks(queueTracks.map((t) => t.id), SHUFFLE_REFILL_BATCH);
		}
		for (const id of ids) {
			await playerApi.addToQueue(id);
		}
	} catch (e) {
		console.warn('Shuffle queue refill failed:', e);
	} finally {
		shuffleRefillPending = false;
	}
}

function checkShuffleQueue() {
	if (!shuffle || shuffleRefillPending) return;
	// Repeat modes intentionally loop the existing queue — don't grow it.
	if (repeat !== 'off') return;
	if (state !== 'playing' && state !== 'paused') return;
	// Refill when the current track is the last (or second to last) in the queue
	if (queuePosition !== null && queuePosition >= queueTracks.length - 2) {
		refillShuffleQueue();
	}
}

// --- Public API ---
export const player = {
	get state() { return state; },
	get currentTrack() { return currentTrack; },
	get positionMs() { return positionMs; },
	get durationMs() {
		// The engine's decoder can't always determine a duration (it then
		// reports 0). Fall back to the track's DB duration so the UI always
		// shows a length when one is known.
		if (durationMs > 0) return durationMs;
		return currentTrack?.duration_ms ?? 0;
	},
	get volume() { return volume; },
	get shuffle() { return shuffle; },
	get repeat() { return repeat; },
	get queueTracks() { return queueTracks; },
	get queuePosition() { return queuePosition; },
	get queueOpen() { return queueOpen; },
	get autoplay() { return autoplay; },
	get previousTrack() { return previousTrack; },
	get isPlaying() { return state === 'playing'; },
	get isPaused() { return state === 'paused'; },
	get isStopped() { return state === 'stopped'; },

	init,

	destroy() {
		if (unlisten) {
			unlisten();
			unlisten = null;
		}
		initialized = false;
	},

	toggleQueuePanel() {
		queueOpen = !queueOpen;
	},

	toggleAutoplay() {
		autoplay = !autoplay;
		if (autoplay) {
			playedHistory.clear();
			// Add current queue tracks to history
			for (const t of queueTracks) {
				playedHistory.add(t.id);
			}
		}
	},

	async playRandom() {
		try {
			const ids = await playerApi.getRandomTracks([], 50);
			if (ids.length > 0) {
				playedHistory.clear();
				await playerApi.playTracks(ids, 0);
				// Auto-open queue so user sees the tracks
				if (!queueOpen) queueOpen = true;
			}
		} catch (e) { console.warn('playRandom failed:', e); }
	},

	async playTrack(trackId: number) {
		await playerApi.playTracks([trackId], 0);
	},

	async playTracks(trackIds: number[], startIndex = 0) {
		playedHistory.clear();
		await playerApi.playTracks(trackIds, startIndex);
	},

	async togglePlayPause() {
		if (state === 'playing') {
			await playerApi.pause();
		} else if (state === 'paused') {
			await playerApi.resume();
		}
	},

	async pause() {
		await playerApi.pause();
	},

	async resume() {
		await playerApi.resume();
	},

	async stop() {
		await playerApi.stop();
	},

	async next() {
		await playerApi.next();
	},

	async prev() {
		await playerApi.prev();
	},

	async seek(seconds: number) {
		await playerApi.seek(seconds);
	},

	async setVolume(vol: number) {
		await playerApi.setVolume(vol);
	},

	async toggleShuffle() {
		try {
			const enabling = !shuffle;
			if (enabling && (state === 'stopped' || queueTracks.length === 0)) {
				// Turning shuffle on with nothing playing — load random tracks
				const ids = await playerApi.getRandomTracks([], 50);
				if (ids.length > 0) {
					playedHistory.clear();
					await playerApi.playTracks(ids, 0);
					await playerApi.setShuffle(true);
					if (!queueOpen) queueOpen = true;
				}
			} else {
				await playerApi.setShuffle(enabling);
				// Turning shuffle on with a queue that has no (or almost no)
				// upcoming tracks — top it up so there is always something next.
				if (
					enabling &&
					queuePosition !== null &&
					queuePosition >= queueTracks.length - 1
				) {
					await refillShuffleQueue();
				}
			}
		} catch (e) {
			console.error('Failed to toggle shuffle:', e);
		}
	},

	async cycleRepeat() {
		const modes: RepeatMode[] = ['off', 'all', 'one'];
		const idx = modes.indexOf(repeat);
		const next = modes[(idx + 1) % modes.length];
		await playerApi.setRepeat(next);
	},

	async addToQueue(trackId: number) {
		await playerApi.addToQueue(trackId);
	},

	async addNext(trackId: number) {
		await playerApi.addNext(trackId);
	},

	async skipTo(index: number) {
		await playerApi.skipTo(index);
	},

	async moveInQueue(fromIndex: number, toIndex: number) {
		// Optimistic update for smooth DnD (save state for rollback)
		const prevTracks = [...queueTracks];
		const prevPos = queuePosition;
		const newTracks = [...queueTracks];
		const [moved] = newTracks.splice(fromIndex, 1);
		newTracks.splice(toIndex, 0, moved);
		queueTracks = newTracks;
		if (queuePosition !== null) {
			if (fromIndex === queuePosition) {
				queuePosition = toIndex;
			} else if (fromIndex < queuePosition && toIndex >= queuePosition) {
				queuePosition = queuePosition - 1;
			} else if (fromIndex > queuePosition && toIndex <= queuePosition) {
				queuePosition = queuePosition + 1;
			}
		}
		try {
			await playerApi.moveInQueue(fromIndex, toIndex);
		} catch {
			// Rollback on failure
			queueTracks = prevTracks;
			queuePosition = prevPos;
			toast.error('Failed to reorder queue');
		}
	},

	async removeFromQueue(index: number) {
		await playerApi.removeFromQueue(index);
	},

	async clearQueue() {
		await playerApi.clearQueue();
	},
};
