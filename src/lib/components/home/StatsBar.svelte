<script lang="ts">
	import { Card, CardContent } from '$lib/components/ui/card';
	import { Music, Disc, Users, ListMusic } from 'lucide-svelte';
	import { formatDurationLong, formatFileSize } from '$lib/utils/format';
	import type { LibraryStats } from '$lib/types';

	let { stats }: { stats: LibraryStats | null } = $props();

	const items = $derived([
		{ label: 'Tracks', value: stats?.total_tracks, icon: Music },
		{ label: 'Albums', value: stats?.total_albums, icon: Disc },
		{ label: 'Artists', value: stats?.total_artists, icon: Users },
		{ label: 'Playlists', value: stats?.total_playlists, icon: ListMusic },
	]);
</script>

<Card>
	<CardContent class="py-4">
		<div class="flex items-center justify-between gap-4 flex-wrap">
			<div class="flex items-center gap-6">
				{#each items as item}
					<div class="flex items-center gap-2">
						<item.icon class="size-4 text-muted-foreground" />
						<span class="text-sm font-medium">{item.value ?? '...'}</span>
						<span class="text-sm text-muted-foreground">{item.label}</span>
					</div>
				{/each}
			</div>
			{#if stats && stats.total_tracks > 0}
				<div class="text-sm text-muted-foreground">
					{formatDurationLong(stats.total_duration_ms)} &middot; {formatFileSize(stats.total_size_bytes)}
				</div>
			{/if}
		</div>
	</CardContent>
</Card>
