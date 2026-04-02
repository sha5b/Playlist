import { convertFileSrc } from '@tauri-apps/api/core';

/** Convert a local file path to a URL the webview can load via the asset protocol. */
export function assetUrl(filePath: string | null | undefined): string {
	if (!filePath) return '';
	return convertFileSrc(filePath);
}

export function formatDuration(ms: number | null): string {
	if (!ms) return '--:--';
	const totalSeconds = Math.floor(ms / 1000);
	const minutes = Math.floor(totalSeconds / 60);
	const seconds = totalSeconds % 60;
	return `${minutes}:${seconds.toString().padStart(2, '0')}`;
}

export function formatDurationLong(ms: number | null): string {
	if (!ms) return '--';
	const totalSeconds = Math.floor(ms / 1000);
	const hours = Math.floor(totalSeconds / 3600);
	const minutes = Math.floor((totalSeconds % 3600) / 60);
	if (hours > 0) {
		return `${hours}h ${minutes}m`;
	}
	return `${minutes}m`;
}

export function formatFileSize(bytes: number | null): string {
	if (!bytes) return '--';
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

export function formatDate(dateStr: string | null): string {
	if (!dateStr) return '--';
	const date = new Date(dateStr);
	return date.toLocaleDateString(undefined, {
		year: 'numeric',
		month: 'short',
		day: 'numeric',
	});
}

/** Fisher-Yates shuffle - returns a new shuffled array */
export function shuffleArray<T>(array: T[]): T[] {
	const result = [...array];
	for (let i = result.length - 1; i > 0; i--) {
		const j = Math.floor(Math.random() * (i + 1));
		[result[i], result[j]] = [result[j], result[i]];
	}
	return result;
}

/** Format seconds as m:ss */
export function formatSeconds(seconds: number | null): string {
	if (!seconds) return '--:--';
	const m = Math.floor(seconds / 60);
	const s = Math.round(seconds % 60);
	return `${m}:${s.toString().padStart(2, '0')}`;
}

/** Format a date string as relative time (e.g., "5m ago", "2h ago") */
export function timeAgo(dateStr: string | null): string {
	if (!dateStr) return 'Never';
	const date = new Date(dateStr + 'Z');
	const now = new Date();
	const diff = now.getTime() - date.getTime();
	const minutes = Math.floor(diff / 60000);
	if (minutes < 1) return 'Just now';
	if (minutes < 60) return `${minutes}m ago`;
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours}h ago`;
	const days = Math.floor(hours / 24);
	if (days < 7) return `${days}d ago`;
	return formatDate(dateStr);
}

const PLATFORM_LABELS: Record<string, string> = {
	youtube: 'YouTube',
	spotify: 'Spotify',
	soundcloud: 'SoundCloud',
	bandcamp: 'Bandcamp',
	direct: 'Direct',
	other: 'Other',
};

/** Get human-readable label for a platform */
export function platformLabel(platform: string): string {
	return PLATFORM_LABELS[platform] ?? platform;
}

/** Get badge color variant for a platform */
export function platformColor(platform: string): 'default' | 'secondary' | 'outline' | 'destructive' {
	switch (platform) {
		case 'youtube': return 'destructive';
		case 'spotify': return 'default';
		case 'soundcloud': return 'secondary';
		default: return 'outline';
	}
}
