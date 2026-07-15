<script lang="ts">
	import { getArtists } from '$lib/api/library';
	import CardGridSkeleton from '$lib/components/shared/CardGridSkeleton.svelte';
	import { Input } from '$lib/components/ui/input';
	import { Users, Search, X } from 'lucide-svelte';
	import { assetUrl } from '$lib/utils/format';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { useSearch } from '$lib/hooks/useSearch.svelte';
	import type { Artist } from '$lib/types';

	let artists: Artist[] = $state([]);
	let total = $state(0);
	let loading = $state(true);

	const search = useSearch(load);

	async function load() {
		loading = true;
		try {
			const [data, count] = await getArtists(0, 200, search.query || undefined);
			artists = data;
			total = count;
		} catch (e) {
			console.error('Failed to load artists:', e);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		libraryStore.version;
		load();
	});
</script>

<div class="overflow-y-auto overflow-x-hidden space-y-6">
	<div class="flex items-center justify-between">
		<div>
			<h1 class="text-3xl font-bold tracking-tight">Artists</h1>
			<p class="text-muted-foreground mt-1">
				{loading ? 'Loading...' : `${total} artist${total !== 1 ? 's' : ''}${search.query ? ` matching "${search.query}"` : ''}`}
			</p>
		</div>
		<div class="relative w-64">
			<Search class="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
			<Input
				placeholder="Search artists..."
				class="pl-9 pr-8"
				value={search.query}
				oninput={search.handleSearch}
			/>
			{#if search.query}
				<button class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground" onclick={search.clearSearch}>
					<X class="size-4" />
				</button>
			{/if}
		</div>
	</div>

	{#if loading && artists.length === 0}
		<CardGridSkeleton rounded="full" />
	{:else if artists.length === 0}
		<div class="flex items-center justify-center h-48 rounded-xl border border-dashed border-border/60">
			<p class="text-muted-foreground text-sm">
				{search.query ? `No artists matching "${search.query}"` : 'No artists in your library yet'}
			</p>
		</div>
	{:else}
		<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
			{#each artists as artist}
				<a
					href="/library/artists/{artist.id}"
					class="group text-center rounded-lg p-2 -m-2 transition-colors hover:bg-muted/30"
					draggable="true"
					ondragstart={(e) => {
						if (!e.dataTransfer) return;
						e.dataTransfer.setData('application/x-playlist-artist', JSON.stringify({ artistId: artist.id, name: artist.name }));
						e.dataTransfer.setData('text/plain', artist.name);
						e.dataTransfer.effectAllowed = 'copyMove';
					}}
				>
					<div class="aspect-square rounded-full bg-muted flex items-center justify-center overflow-hidden mb-2 mx-auto max-w-32">
						{#if artist.image_path}
							<img
								src={assetUrl(artist.image_path)}
								alt={artist.name}
								class="size-full object-cover group-hover:scale-105 transition-transform"
								loading="lazy"
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
