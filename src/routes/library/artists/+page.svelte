<script lang="ts">
	import { getArtists } from '$lib/api/library';
	import CardGridSkeleton from '$lib/components/shared/CardGridSkeleton.svelte';
	import { Users } from 'lucide-svelte';
	import type { Artist } from '$lib/types';

	let artists: Artist[] = $state([]);
	let total = $state(0);
	let loading = $state(true);

	async function load() {
		loading = true;
		try {
			const [data, count] = await getArtists(0, 200);
			artists = data;
			total = count;
		} catch (e) {
			console.error('Failed to load artists:', e);
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
		<h1 class="text-3xl font-bold tracking-tight">Artists</h1>
		<p class="text-muted-foreground mt-1">
			{loading ? 'Loading...' : `${total} artist${total !== 1 ? 's' : ''}`}
		</p>
	</div>

	{#if loading}
		<CardGridSkeleton rounded="full" />
	{:else if artists.length === 0}
		<div class="flex items-center justify-center h-48 rounded-lg border border-dashed border-border">
			<p class="text-muted-foreground text-sm">No artists in your library yet</p>
		</div>
	{:else}
		<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
			{#each artists as artist}
				<a href="/library/artists/{artist.id}" class="group text-center">
					<div class="aspect-square rounded-full bg-muted flex items-center justify-center overflow-hidden mb-2 mx-auto max-w-32">
						{#if artist.image_path}
							<img
								src="https://asset.localhost/{artist.image_path}"
								alt={artist.name}
								class="size-full object-cover group-hover:scale-105 transition-transform"
							/>
						{:else}
							<Users class="size-10 text-muted-foreground" />
						{/if}
					</div>
					<p class="text-sm font-medium truncate">{artist.name}</p>
					<p class="text-xs text-muted-foreground">
						{artist.track_count} track{artist.track_count !== 1 ? 's' : ''}
					</p>
				</a>
			{/each}
		</div>
	{/if}
</div>
