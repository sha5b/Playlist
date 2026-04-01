<script lang="ts">
	import { getAlbums } from '$lib/api/library';
	import CardGridSkeleton from '$lib/components/shared/CardGridSkeleton.svelte';
	import { Disc } from 'lucide-svelte';
	import { assetUrl } from '$lib/utils/format';
	import type { Album } from '$lib/types';

	let albums: Album[] = $state([]);
	let total = $state(0);
	let loading = $state(true);

	async function load() {
		loading = true;
		try {
			const [data, count] = await getAlbums(0, 200);
			albums = data;
			total = count;
		} catch (e) {
			console.error('Failed to load albums:', e);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});
</script>

<div class="space-y-6">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Albums</h1>
		<p class="text-muted-foreground mt-1">
			{loading ? 'Loading...' : `${total} album${total !== 1 ? 's' : ''}`}
		</p>
	</div>

	{#if loading}
		<CardGridSkeleton />
	{:else if albums.length === 0}
		<div class="flex items-center justify-center h-48 rounded-lg border border-dashed border-border">
			<p class="text-muted-foreground text-sm">No albums in your library yet</p>
		</div>
	{:else}
		<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
			{#each albums as album}
				<a href="/library/albums/{album.id}" class="group">
					<div class="aspect-square rounded-lg bg-muted flex items-center justify-center overflow-hidden mb-2">
						{#if album.cover_art_path}
							<img
								src={assetUrl(album.cover_art_path)}
								alt={album.title}
								class="size-full object-cover group-hover:scale-105 transition-transform"
								loading="lazy"
							/>
						{:else}
							<Disc class="size-12 text-muted-foreground" />
						{/if}
					</div>
					<p class="text-sm font-medium truncate">{album.title}</p>
					<p class="text-xs text-muted-foreground truncate">
						{album.artist_name ?? 'Unknown Artist'}
						{#if album.year} &middot; {album.year}{/if}
					</p>
				</a>
			{/each}
		</div>
	{/if}
</div>
