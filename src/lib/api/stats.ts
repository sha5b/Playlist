import { invoke } from '@tauri-apps/api/core';
import type { StatsOverview, StatsPeriod, StatsTop } from '$lib/types';

export async function getStatsOverview(): Promise<StatsOverview> {
	return invoke('stats_overview');
}

export async function getStatsTop(period: StatsPeriod, limit?: number): Promise<StatsTop> {
	return invoke('stats_top', { period, limit });
}
