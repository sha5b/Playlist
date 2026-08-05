import { listen } from '@tauri-apps/api/event';
import * as playerApi from '$lib/api/player';
import { getSetting, setSetting } from '$lib/api/library';
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
// Endless play: when playback nears the end of the queue, auto-append random
// tracks from the library so the music never stops.
let endless: boolean = $state(false);
let previousTrack: QueueTrack | null = $state(null);
let playedHistory: Set<number> = new Set();

// --- Sleep timer (frontend-only) ---
export type SleepTimerMode = 'off' | 'end-of-track' | number; // number = minutes

/** Options offered by the sleep-timer dropdowns. */
export const SLEEP_TIMER_OPTIONS: { label: string; value: SleepTimerMode }[] = [
	{ label: 'Off', value: 'off' },
	{ label: '15 minutes', value: 15 },
	{ label: '30 minutes', value: 30 },
	{ label: '45 minutes', value: 45 },
	{ label: '60 minutes', value: 60 },
	{ label: 'End of track', value: 'end-of-track' },
];

/** Compact remaining-time label for an active sleep timer (e.g. "29m", "45s"). */
export function formatSleepRemaining(mode: SleepTimerMode, remainingMs: number): string {
	if (mode === 'off') return '';
	if (mode === 'end-of-track') return 'track end';
	const totalSeconds = Math.ceil(remainingMs / 1000);
	if (totalSeconds >= 60) return `${Math.ceil(totalSeconds / 60)}m`;
	return `${totalSeconds}s`;
}
let sleepTimerMode: SleepTimerMode = $state('off');
let sleepRemainingMs: number = $state(0);
let sleepDeadline: number | null = null;
let sleepInterval: ReturnType<typeof setInterval> | null = null;
let sleepFiring = false;
let sleepRestoreVolume = 0.75;

// --- Queue persistence ---
const PERSIST_KEY = 'player.saved_state';
const POSITION_SAVE_INTERVAL_MS = 5000;
interface SavedPlayerState {
	queueIds: number[];
	currentTrackId: number | null;
	positionMs: number;
	volume: number;
	shuffle: boolean;
	endless: boolean;
	repeat: RepeatMode;
}
// Blocks saves until the saved state has been read back on launch, so an
// early (empty) queue event can't clobber what we're about to restore.
let persistReady = false;
let saveTimeout: ReturnType<typeof setTimeout> | null = null;
let lastPositionSave = 0;
// Suppress play-recording and endless refill while a restore is in flight
// (events from the restore sequence arrive slightly after the invokes return).
let suppressSideEffectsUntil = 0;

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
			// Endless play: if playback stopped naturally before the refill
			// could land, refill and resume.
			if (endless && prevState === 'playing' && state === 'stopped' && Date.now() >= suppressSideEffectsUntil) {
				refillEndlessQueue(true);
			}
			// Proactively top up the queue when nearing its end
			checkEndlessQueue();
			scheduleSave();
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
				// Record the play in the database (fire-and-forget) — but not
				// for the track loaded by the launch restore.
				if (Date.now() >= suppressSideEffectsUntil) {
					playerApi.recordPlay(event.data.id).catch(() => {});
				}
				// Sleep timer set to "end of track": the track just changed,
				// so the old one finished — stop playback now.
				if (sleepTimerMode === 'end-of-track') {
					fireSleepTimer();
				}
			}
			updateMediaSessionMetadata(currentTrack);
			scheduleSave();
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
			// Sleep timer "end of track": start fading ~5s before the track ends
			if (
				sleepTimerMode === 'end-of-track' &&
				!sleepFiring &&
				state === 'playing' &&
				event.data.duration_ms > 0 &&
				event.data.duration_ms - event.data.position_ms <= 5200
			) {
				fireSleepTimer();
			}
			// Persist the playback position periodically while playing
			if (Date.now() - lastPositionSave >= POSITION_SAVE_INTERVAL_MS) {
				lastPositionSave = Date.now();
				scheduleSave(500);
			}
			break;
		case 'queue_updated':
			queueTracks = event.data.tracks;
			queuePosition = event.data.position;
			checkEndlessQueue();
			scheduleSave();
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

		// Fresh engine (real app launch, not a frontend reload while music
		// plays): restore the persisted queue, paused at the saved position.
		if (s.state === 'stopped' && tracks.length === 0) {
			await restoreSavedState();
		}
	} catch {
		// Engine may not be ready yet
	}
	persistReady = true;
}

// --- Queue persistence ---

function scheduleSave(delayMs = 1000) {
	if (!persistReady) return;
	if (saveTimeout) clearTimeout(saveTimeout);
	saveTimeout = setTimeout(() => {
		saveTimeout = null;
		saveNow();
	}, delayMs);
}

async function saveNow() {
	if (!persistReady) return;
	const saved: SavedPlayerState = {
		queueIds: queueTracks.map((t) => t.id),
		currentTrackId: currentTrack?.id ?? null,
		positionMs,
		// While the sleep timer is fading the volume down, persist the volume
		// the user actually chose, not a transient fade step.
		volume: sleepFiring ? sleepRestoreVolume : volume,
		shuffle,
		endless,
		repeat,
	};
	try {
		await setSetting(PERSIST_KEY, JSON.stringify(saved));
	} catch (e) {
		console.warn('Failed to persist player state:', e);
	}
}

async function restoreSavedState() {
	let saved: SavedPlayerState;
	try {
		const raw = await getSetting(PERSIST_KEY);
		if (!raw) return;
		saved = JSON.parse(raw) as SavedPlayerState;
	} catch {
		return;
	}
	try {
		suppressSideEffectsUntil = Date.now() + 5000;
		endless = !!saved.endless;
		const savedVolume = typeof saved.volume === 'number' ? Math.min(Math.max(saved.volume, 0), 1) : 0.75;
		if (saved.repeat === 'all' || saved.repeat === 'one') {
			await playerApi.setRepeat(saved.repeat);
		}
		const queueIds = Array.isArray(saved.queueIds) ? saved.queueIds : [];
		// Skip tracks that were deleted from the library since the last run
		const ids = queueIds.length > 0 ? await playerApi.filterExistingTracks(queueIds) : [];
		if (ids.length === 0) {
			await playerApi.setVolume(savedVolume);
			if (saved.shuffle) await playerApi.setShuffle(true);
			return;
		}
		let startIndex = saved.currentTrackId != null ? ids.indexOf(saved.currentTrackId) : 0;
		if (startIndex < 0) startIndex = 0;
		// Load muted, pause immediately, seek, then restore the volume — the
		// engine has no "load paused" command, so this keeps the restore silent
		// and the app never autoplays on launch.
		await playerApi.setVolume(0);
		await playerApi.playTracks(ids, startIndex);
		await playerApi.pause();
		const resumeSameTrack = saved.currentTrackId != null && ids[startIndex] === saved.currentTrackId;
		const posMs = resumeSameTrack && typeof saved.positionMs === 'number' ? Math.max(0, saved.positionMs) : 0;
		if (posMs > 0) {
			await playerApi.seek(posMs / 1000);
		}
		await playerApi.setVolume(savedVolume);
		// The saved queue order already reflects any earlier shuffling; setting
		// the flag afterwards re-shuffles only the upcoming portion.
		if (saved.shuffle) await playerApi.setShuffle(true);
		for (const id of ids) playedHistory.delete(id);
	} catch (e) {
		console.warn('Failed to restore player state:', e);
	}
}

// --- Endless play auto-refill ---
// With endless play on, the queue never runs dry: whenever playback
// approaches the end of the queue, append more random tracks from the library.
const ENDLESS_REFILL_BATCH = 25;
let endlessRefillPending = false;

async function refillEndlessQueue(startPlayback: boolean) {
	if (endlessRefillPending) return;
	endlessRefillPending = true;
	try {
		// Avoid queuing tracks that are already in the queue or recently played
		const excludeIds = Array.from(new Set([
			...queueTracks.map((t) => t.id),
			...playedHistory,
		]));
		let ids = await playerApi.getRandomTracks(excludeIds, ENDLESS_REFILL_BATCH);
		if (ids.length === 0) {
			// Library smaller than the queue/history — allow repeats
			playedHistory.clear();
			ids = await playerApi.getRandomTracks(queueTracks.map((t) => t.id), ENDLESS_REFILL_BATCH);
		}
		if (ids.length === 0) return;
		if (startPlayback && (state === 'stopped' || queueTracks.length === 0)) {
			await playerApi.playTracks(ids, 0);
		} else {
			for (const id of ids) {
				await playerApi.addToQueue(id);
			}
		}
	} catch (e) {
		console.warn('Endless play refill failed:', e);
	} finally {
		endlessRefillPending = false;
	}
}

function checkEndlessQueue() {
	if (!endless || endlessRefillPending) return;
	if (Date.now() < suppressSideEffectsUntil) return;
	// Repeat modes intentionally loop the existing queue — don't grow it.
	if (repeat !== 'off') return;
	if (state !== 'playing' && state !== 'paused') return;
	// Refill when the current track is the last (or second to last) in the queue
	if (queuePosition !== null && queuePosition >= queueTracks.length - 2) {
		refillEndlessQueue(false);
	}
}

// --- Sleep timer ---

function setSleepTimer(mode: SleepTimerMode) {
	sleepTimerMode = mode;
	if (sleepInterval) {
		clearInterval(sleepInterval);
		sleepInterval = null;
	}
	sleepDeadline = null;
	sleepRemainingMs = 0;
	if (mode === 'off' || mode === 'end-of-track') return;
	sleepDeadline = Date.now() + mode * 60_000;
	sleepRemainingMs = mode * 60_000;
	sleepInterval = setInterval(() => {
		if (sleepDeadline === null) return;
		sleepRemainingMs = Math.max(0, sleepDeadline - Date.now());
		if (sleepRemainingMs <= 0) {
			fireSleepTimer();
		}
	}, 1000);
}

// Fade the volume down over ~5s, pause, then restore the volume setting.
async function fireSleepTimer() {
	if (sleepFiring) return;
	sleepFiring = true;
	sleepTimerMode = 'off';
	if (sleepInterval) {
		clearInterval(sleepInterval);
		sleepInterval = null;
	}
	sleepDeadline = null;
	sleepRemainingMs = 0;
	try {
		if (volume > 0) sleepRestoreVolume = volume;
		if (state === 'playing') {
			const startVol = volume;
			const steps = 20;
			for (let i = 1; i <= steps; i++) {
				await playerApi.setVolume(startVol * (1 - i / steps));
				await new Promise((r) => setTimeout(r, 250));
			}
		}
		await playerApi.pause();
		await playerApi.setVolume(sleepRestoreVolume);
	} catch (e) {
		console.warn('Sleep timer failed:', e);
	} finally {
		sleepFiring = false;
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
	get endless() { return endless; },
	get sleepTimerMode() { return sleepTimerMode; },
	get sleepRemainingMs() { return sleepRemainingMs; },
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
		if (saveTimeout) {
			clearTimeout(saveTimeout);
			saveTimeout = null;
		}
		if (sleepInterval) {
			clearInterval(sleepInterval);
			sleepInterval = null;
		}
		initialized = false;
		persistReady = false;
	},

	toggleQueuePanel() {
		queueOpen = !queueOpen;
	},

	async toggleEndless() {
		endless = !endless;
		if (endless) {
			playedHistory.clear();
			// Add current queue tracks to history
			for (const t of queueTracks) {
				playedHistory.add(t.id);
			}
			if (state === 'stopped' || queueTracks.length === 0) {
				// Nothing playing — fill the queue and start
				await refillEndlessQueue(true);
				if (!queueOpen) queueOpen = true;
			} else {
				checkEndlessQueue();
			}
		}
		scheduleSave();
	},

	setSleepTimer,

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
		// Shuffle only reorders the EXISTING queue: upcoming tracks are
		// shuffled, the current track stays current. Toggling off restores
		// the original order. (Auto-refilling lives in endless play.)
		try {
			await playerApi.setShuffle(!shuffle);
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
