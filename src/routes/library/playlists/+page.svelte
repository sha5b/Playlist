<script lang="ts">
	import { getPlaylists, createPlaylist } from '$lib/api/library';
	import { Button } from '$lib/components/ui/button';
	import { Card } from '$lib/components/ui/card';
	import CardGridSkeleton from '$lib/components/shared/CardGridSkeleton.svelte';
	import { ListMusic, Plus } from 'lucide-svelte';
	import { formatDurationLong, assetUrl } from '$lib/utils/format';
	import type { Playlist } from '$lib/types';

	let playlists: Playlist[] = $state([]);
	let loading = $state(true);

	async function load() {
		loading = true;
		try {
			playlists = await getPlaylists();
		} catch (e) {
			console.error('Failed to load playlists:', e);
		} finally {
			loading = false;
		}
	}

	async function handleCreate() {
		const name = prompt('Playlist name:');
		if (!name) return;
		try {
			await createPlaylist(name);
			await load();
		} catch (e) {
			console.error('Failed to create playlist:', e);
		}
	}

	$effect(() => {
		load();
	});
</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
	<div class="flex items-center justify-between">
		<div>
			<h1 class="text-3xl font-bold tracking-tight">Playlists</h1>
			<p class="text-muted-foreground mt-1">
				{loading ? 'Loading...' : `${playlists.length} playlist${playlists.length !== 1 ? 's' : ''}`}
			</p>
		</div>
		<Button onclick={handleCreate}>
			<Plus class="size-4" />
			New Playlist
		</Button>
	</div>

	{#if loading}
		<CardGridSkeleton />
	{:else if playlists.length === 0}
		<div class="flex flex-col items-center justify-center h-48 rounded-lg border border-dashed border-border gap-3">
			<p class="text-muted-foreground text-sm">No playlists created yet</p>
			<Button variant="outline" size="sm" onclick={handleCreate}>
				<Plus class="size-4" />
				Create your first playlist
			</Button>
		</div>
	{:else}
		<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4">
			{#each playlists as playlist}
				<a href="/library/playlists/{playlist.id}" class="group rounded-lg p-2 -m-2 transition-colors hover:bg-muted/30">
					<div class="aspect-square rounded-lg bg-muted flex items-center justify-center overflow-hidden mb-2">
						{#if playlist.cover_art_path}
							<img
								src={assetUrl(playlist.cover_art_path)}
								alt={playlist.name}
								class="size-full object-cover group-hover:scale-105 transition-transform"
								loading="lazy"
							/>
						{:else}
							<ListMusic class="size-12 text-muted-foreground" />
						{/if}
					</div>
					<p class="text-sm font-medium truncate">{playlist.name}</p>
					<p class="text-xs text-muted-foreground">
						{playlist.track_count} track{playlist.track_count !== 1 ? 's' : ''}
						{#if playlist.total_duration_ms > 0}
							&middot; {formatDurationLong(playlist.total_duration_ms)}
						{/if}
					</p>
				</a>
			{/each}
		</div>
	{/if}
</div>
