<script lang="ts">
	import { Music, Disc, Users, ListMusic } from 'lucide-svelte';
	import { formatDurationLong, formatFileSize } from '$lib/utils/format';
	import type { LibraryStats } from '$lib/types';

	let { stats }: { stats: LibraryStats | null } = $props();
</script>

{#if stats}
	<div class="flex items-center gap-5 text-sm text-muted-foreground flex-wrap">
		<span class="flex items-center gap-1.5">
			<Music class="size-3.5" />
			<span class="font-medium text-foreground">{stats.total_tracks.toLocaleString()}</span> tracks
		</span>
		<span class="flex items-center gap-1.5">
			<Disc class="size-3.5" />
			<span class="font-medium text-foreground">{stats.total_albums.toLocaleString()}</span> albums
		</span>
		<span class="flex items-center gap-1.5">
			<Users class="size-3.5" />
			<span class="font-medium text-foreground">{stats.total_artists.toLocaleString()}</span> artists
		</span>
		{#if stats.total_playlists > 0}
			<span class="flex items-center gap-1.5">
				<ListMusic class="size-3.5" />
				<span class="font-medium text-foreground">{stats.total_playlists}</span> playlists
			</span>
		{/if}
		{#if stats.total_tracks > 0}
			<span class="ml-auto text-xs">
				{formatDurationLong(stats.total_duration_ms)} &middot; {formatFileSize(stats.total_size_bytes)}
			</span>
		{/if}
	</div>
{/if}
