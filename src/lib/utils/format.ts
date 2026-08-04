import { convertFileSrc } from '@tauri-apps/api/core';

/** Convert a local file path to a URL the webview can load via the asset protocol. */
export function assetUrl(filePath: string | null | undefined): string {
	if (!filePath) return '';
	return convertFileSrc(filePath);
}

export function formatDuration(ms: number | null | undefined): string {
	if (ms == null || !Number.isFinite(ms) || ms < 0) return '--:--';
	const totalSeconds = Math.floor(ms / 1000);
	const hours = Math.floor(totalSeconds / 3600);
	const minutes = Math.floor((totalSeconds % 3600) / 60);
	const seconds = totalSeconds % 60;
	if (hours > 0) {
		return `${hours}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
	}
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

/** Parse a SQLite datetime string, which is UTC ("YYYY-MM-DD HH:MM:SS"). */
function parseUtcDate(dateStr: string): Date {
	// Already ISO with timezone info — parse as-is
	if (dateStr.endsWith('Z') || /[+-]\d{2}:?\d{2}$/.test(dateStr)) {
		return new Date(dateStr);
	}
	// SQLite format: make it ISO and mark it UTC
	return new Date(dateStr.replace(' ', 'T') + 'Z');
}

export function formatDate(dateStr: string | null): string {
	if (!dateStr) return '--';
	const date = parseUtcDate(dateStr);
	if (Number.isNaN(date.getTime())) return '--';
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

/** Format seconds as m:ss (or h:mm:ss) */
export function formatSeconds(seconds: number | null | undefined): string {
	if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return '--:--';
	// Round the TOTAL first — rounding the remainder alone rendered "3:60"
	// for values like 239.6s.
	return formatDuration(Math.round(seconds) * 1000);
}

/** Format a date string as relative time (e.g., "5m ago", "2h ago") */
export function timeAgo(dateStr: string | null): string {
	if (!dateStr) return 'Never';
	const date = parseUtcDate(dateStr);
	if (Number.isNaN(date.getTime())) return 'Never';
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
