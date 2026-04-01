<script lang="ts">
	import { getAlbums } from '$lib/api/library';
	import CardGridSkeleton from '$lib/components/shared/CardGridSkeleton.svelte';
	import { Input } from '$lib/components/ui/input';
	import { Disc, Search, X } from 'lucide-svelte';
	import { assetUrl } from '$lib/utils/format';
	import { libraryStore } from '$lib/stores/library.svelte';
	import type { Album } from '$lib/types';

	let albums: Album[] = $state([]);
	let total = $state(0);
	let loading = $state(true);
	let searchQuery = $state('');
	let debounceTimer: ReturnType<typeof setTimeout>;

	async function load() {
		loading = true;
		try {
			const [data, count] = await getAlbums(0, 200, searchQuery || undefined);
			albums = data;
			total = count;
		} catch (e) {
			console.error('Failed to load albums:', e);
		} finally {
			loading = false;
		}
	}

	function handleSearch(e: Event) {
		searchQuery = (e.target as HTMLInputElement).value;
		clearTimeout(debounceTimer);
		debounceTimer = setTimeout(() => load(), 250);
	}

	function clearSearch() {
		searchQuery = '';
		load();
	}

	$effect(() => {
		libraryStore.version;
		load();
	});
</script>

<div class="flex flex-col flex-1 min-h-0 gap-6 overflow-y-auto">
	<div class="flex items-center justify-between shrink-0">
		<div>
			<h1 class="text-3xl font-bold tracking-tight">Albums</h1>
			<p class="text-muted-foreground mt-1">
				{loading ? 'Loading...' : `${total} album${total !== 1 ? 's' : ''}${searchQuery ? ` matching "${searchQuery}"` : ''}`}
			</p>
		</div>
		<div class="relative w-64">
			<Search class="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
			<Input
				placeholder="Search albums..."
				class="pl-9 pr-8"
				value={searchQuery}
				oninput={handleSearch}
			/>
			{#if searchQuery}
				<button class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground" onclick={clearSearch}>
					<X class="size-4" />
				</button>
			{/if}
		</div>
	</div>

	{#if loading && albums.length === 0}
		<CardGridSkeleton />
	{:else if albums.length === 0}
		<div class="flex items-center justify-center h-48 rounded-lg border border-dashed border-border">
			<p class="text-muted-foreground text-sm">
				{searchQuery ? `No albums matching "${searchQuery}"` : 'No albums in your library yet'}
			</p>
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
