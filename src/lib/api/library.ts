import { invoke } from '@tauri-apps/api/core';
import type { LibraryStats } from '$lib/types';

export async function greet(name: string): Promise<string> {
	return invoke('greet', { name });
}

export async function getLibraryStats(): Promise<LibraryStats> {
	return invoke('get_library_stats');
}
