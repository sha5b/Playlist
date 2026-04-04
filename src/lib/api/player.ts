import { invoke } from '@tauri-apps/api/core';
import type { PlaybackState, QueueTrack } from '$lib/types';

export async function playTrack(trackId: number): Promise<void> {
	return invoke('player_play_track', { trackId });
}

export async function playTracks(trackIds: number[], startIndex: number): Promise<void> {
	return invoke('player_play_tracks', { trackIds, startIndex });
}

export async function pause(): Promise<void> {
	return invoke('player_pause');
}

export async function resume(): Promise<void> {
	return invoke('player_resume');
}

export async function stop(): Promise<void> {
	return invoke('player_stop');
}

export async function next(): Promise<void> {
	return invoke('player_next');
}

export async function prev(): Promise<void> {
	return invoke('player_prev');
}

export async function seek(positionSeconds: number): Promise<void> {
	return invoke('player_seek', { positionSeconds });
}

export async function setVolume(volume: number): Promise<void> {
	return invoke('player_set_volume', { volume });
}

export async function setShuffle(shuffle: boolean): Promise<void> {
	return invoke('player_set_shuffle', { shuffle });
}

export async function setRepeat(mode: import('$lib/types').RepeatMode): Promise<void> {
	return invoke('player_set_repeat', { mode });
}

export async function addToQueue(trackId: number): Promise<void> {
	return invoke('player_add_to_queue', { trackId });
}

export async function addNext(trackId: number): Promise<void> {
	return invoke('player_add_next', { trackId });
}

export async function moveInQueue(fromIndex: number, toIndex: number): Promise<void> {
	return invoke('player_move_in_queue', { fromIndex, toIndex });
}

export async function skipTo(index: number): Promise<void> {
	return invoke('player_skip_to', { index });
}

export async function removeFromQueue(index: number): Promise<void> {
	return invoke('player_remove_from_queue', { index });
}

export async function clearQueue(): Promise<void> {
	return invoke('player_clear_queue');
}

export async function getState(): Promise<PlaybackState> {
	return invoke('player_get_state');
}

export async function getQueue(): Promise<[QueueTrack[], number | null]> {
	return invoke('player_get_queue');
}

export async function getRandomTracks(excludeIds: number[] = [], limit = 1): Promise<number[]> {
	return invoke('player_random_tracks', { excludeIds, limit });
}

export async function recordPlay(trackId: number): Promise<void> {
	return invoke('player_record_play', { trackId });
}

export async function getAudioDevices(): Promise<[string, boolean][]> {
	return invoke('player_get_audio_devices');
}

export async function setAudioDevice(deviceName: string | null): Promise<void> {
	return invoke('player_set_audio_device', { deviceName });
}
