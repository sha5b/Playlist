<script lang="ts">
	import { formatDurationLong } from '$lib/utils/format';
	import type { LibraryStats } from '$lib/types';

	let { stats }: { stats: LibraryStats | null } = $props();

	const greeting = $derived.by(() => {
		const hour = new Date().getHours();
		if (hour < 12) return 'Good morning';
		if (hour < 18) return 'Good afternoon';
		return 'Good evening';
	});
</script>

<div class="space-y-1">
	<h1 class="text-2xl font-bold tracking-tight">{greeting}</h1>
	{#if stats && stats.total_tracks > 0}
		<p class="text-sm text-muted-foreground">
			<a href="/library/songs" class="hover:text-foreground hover:underline underline-offset-2 transition-colors">
				{stats.total_tracks.toLocaleString()} tracks
			</a>
			{#if stats.total_albums > 0}
				&middot;
				<a href="/library/albums" class="hover:text-foreground hover:underline underline-offset-2 transition-colors">
					{stats.total_albums.toLocaleString()} albums
				</a>
			{/if}
			{#if stats.total_artists > 0}
				&middot;
				<a href="/library/artists" class="hover:text-foreground hover:underline underline-offset-2 transition-colors">
					{stats.total_artists.toLocaleString()} artists
				</a>
			{/if}
			{#if stats.total_duration_ms > 0}
				&middot; {formatDurationLong(stats.total_duration_ms)}
			{/if}
		</p>
	{/if}
</div>
