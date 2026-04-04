import { invoke } from '@tauri-apps/api/core';
import type { ScannedDevice, Device, DeviceDetail } from '$lib/types';

export async function scanDevices(): Promise<ScannedDevice[]> {
	return invoke('devices_scan');
}

export async function getDevices(): Promise<Device[]> {
	return invoke('devices_get_all');
}

export async function getDeviceDetail(deviceId: number): Promise<DeviceDetail> {
	return invoke('devices_get_detail', { deviceId });
}

export async function configureDevice(
	deviceId: number,
	musicDir: string,
	outputFormat: string,
	outputBitrate: string,
	generateM3u: boolean
): Promise<void> {
	return invoke('devices_configure', { deviceId, musicDir, outputFormat, outputBitrate, generateM3u });
}

export async function addDevicePlaylist(deviceId: number, playlistId: number): Promise<void> {
	return invoke('devices_add_playlist', { deviceId, playlistId });
}

export async function removeDevicePlaylist(deviceId: number, playlistId: number): Promise<void> {
	return invoke('devices_remove_playlist', { deviceId, playlistId });
}

export async function syncDevice(deviceId: number, playlistId: number): Promise<void> {
	return invoke('devices_sync', { deviceId, playlistId });
}

export async function cancelDeviceSync(): Promise<void> {
	return invoke('devices_cancel_sync');
}

export async function clearDeviceSyncHistory(deviceId: number): Promise<number> {
	return invoke('devices_clear_history', { deviceId });
}
