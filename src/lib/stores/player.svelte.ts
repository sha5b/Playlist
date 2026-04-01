import { listen } from '@tauri-apps/api/event';
import * as playerApi from '$lib/api/player';
import type { PlaybackState, QueueTrack, PlayerEvent, RepeatMode, PlayerStateEnum } from '$lib/types';

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

// --- Event listener setup ---
let initialized = false;

// Throttle progress updates to display refresh rate to avoid unnecessary re-renders
let progressRafPending = false;
let pendingPositionMs = 0;
let pendingDurationMs = 0;

function handleEvent(event: PlayerEvent) {
	switch (event.kind) {
		case 'state_changed':
			state = event.data.state;
			currentTrack = event.data.current_track;
			positionMs = event.data.position_ms;
			durationMs = event.data.duration_ms;
			volume = event.data.volume;
			shuffle = event.data.shuffle;
			repeat = event.data.repeat;
			queuePosition = event.data.queue_position;
			break;
		case 'track_changed':
			currentTrack = event.data;
			if (event.data) {
				positionMs = 0;
				durationMs = event.data.duration_ms ?? 0;
			}
			break;
		case 'progress':
			pendingPositionMs = event.data.position_ms;
			pendingDurationMs = event.data.duration_ms;
			if (!progressRafPending) {
				progressRafPending = true;
				requestAnimationFrame(() => {
					positionMs = pendingPositionMs;
					durationMs = pendingDurationMs;
					progressRafPending = false;
				});
			}
			break;
		case 'queue_updated':
			queueTracks = event.data.tracks;
			queuePosition = event.data.position;
			break;
		case 'error':
			console.error('Player error:', event.data);
			break;
	}
}

async function init() {
	if (initialized) return;
	initialized = true;
	await listen<PlayerEvent>('player-event', (e) => handleEvent(e.payload));
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

// --- Public API ---
export const player = {
	get state() { return state; },
	get currentTrack() { return currentTrack; },
	get positionMs() { return positionMs; },
	get durationMs() { return durationMs; },
	get volume() { return volume; },
	get shuffle() { return shuffle; },
	get repeat() { return repeat; },
	get queueTracks() { return queueTracks; },
	get queuePosition() { return queuePosition; },
	get queueOpen() { return queueOpen; },
	get isPlaying() { return state === 'playing'; },
	get isPaused() { return state === 'paused'; },
	get isStopped() { return state === 'stopped'; },

	init,

	toggleQueuePanel() {
		queueOpen = !queueOpen;
	},

	async playTrack(trackId: number) {
		await playerApi.playTrack(trackId);
	},

	async playTracks(trackIds: number[], startIndex = 0) {
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
		await playerApi.setShuffle(!shuffle);
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

	async removeFromQueue(index: number) {
		await playerApi.removeFromQueue(index);
	},

	async clearQueue() {
		await playerApi.clearQueue();
	},
};
