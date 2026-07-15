<script lang="ts">
	import { getAlbums, getAlbumsDownloadStatus, cleanupDuplicateAlbums } from '$lib/api/library';
	import type { AlbumDownloadStatus } from '$lib/api/library';
	import CardGridSkeleton from '$lib/components/shared/CardGridSkeleton.svelte';
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { Disc, Search, X, Check, Sparkles, Loader2 } from 'lucide-svelte';
	import { assetUrl } from '$lib/utils/format';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { toast } from 'svelte-sonner';
	import { useSearch } from '$lib/hooks/useSearch.svelte';
	import { beforeNavigate, afterNavigate } from '$app/navigation';
	import { ui } from '$lib/stores/ui.svelte';
	import type { Album } from '$lib/types';

	let albums: Album[] = $state([]);
	let total = $state(0);
	let loading = $state(true);
	let downloadStatuses: Record<string, AlbumDownloadStatus> = $state({});
	let cleaningDuplicates = $state(false);
	let scrollContainer: HTMLDivElement | undefined = $state();

	beforeNavigate(() => {
		if (scrollContainer) {
			ui.saveScroll('/library/albums', scrollContainer.scrollTop);
		}
	});

	afterNavigate(() => {
		if (scrollContainer) {
			const saved = ui.getScroll('/library/albums');
			if (saved > 0) {
				requestAnimationFrame(() => {
					if (scrollContainer) scrollContainer.scrollTop = saved;
				});
			}
		}
	});

	const search = useSearch(load);

	async function load() {
		loading = true;
		try {
			const [data, count] = await getAlbums(0, 10000, search.query || undefined);
			albums = data;
			total = count;
			// Load download statuses for all albums
			if (data.length > 0) {
				const ids = data.map((a) => a.id);
				downloadStatuses = await getAlbumsDownloadStatus(ids);
			}
		} catch (e) {
			console.error('Failed to load albums:', e);
		} finally {
			loading = false;
		}
	}

	async function handleCleanupDuplicates() {
		cleaningDuplicates = true;
		try {
			const result = await cleanupDuplicateAlbums();
			toast.success('Duplicates cleaned up', { 
				description: `Merged ${result.merged_album_groups} album groups, removed ${result.deleted_duplicate_albums} duplicate albums, ${result.orphaned_albums_removed} orphaned. Merged ${result.merged_track_groups} track groups, removed ${result.deleted_duplicate_tracks} duplicate tracks.` 
			});
			load(); // Reload albums after cleanup
		} catch (e) {
			toast.error('Failed to cleanup duplicates', { description: String(e) });
		} finally {
			cleaningDuplicates = false;
		}
	}

	let lastLoadedVersion = -1;

	$effect(() => {
		const v = libraryStore.version;
		if (v === lastLoadedVersion) return;
		lastLoadedVersion = v;
		load();
	});
</script>

<div bind:this={scrollContainer} class="overflow-y-auto overflow-x-hidden space-y-6">
	<div class="flex items-center justify-between">
		<div>
			<h1 class="text-3xl font-bold tracking-tight">Albums</h1>
			<p class="text-muted-foreground mt-1">
				{loading ? 'Loading...' : `${total} album${total !== 1 ? 's' : ''}${search.query ? ` matching "${search.query}"` : ''}`}
			</p>
		</div>
		<div class="flex items-center gap-2">
			<Button variant="outline" size="sm" onclick={handleCleanupDuplicates} disabled={cleaningDuplicates}>
				{#if cleaningDuplicates}
					<Loader2 class="size-4 animate-spin" />
					Cleaning...
				{:else}
					<Sparkles class="size-4" />
					Cleanup Duplicates
				{/if}
			</Button>
			<div class="relative w-64">
				<Search class="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
				<Input
					placeholder="Search albums..."
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
	</div>

	{#if loading && albums.length === 0}
		<CardGridSkeleton />
	{:else if albums.length === 0}
		<div class="flex items-center justify-center h-48 rounded-xl border border-dashed border-border/60">
			<p class="text-muted-foreground text-sm">
				{search.query ? `No albums matching "${search.query}"` : 'No albums in your library yet'}
			</p>
		</div>
	{:else}
		<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
			{#each albums as album (album.id)}
				{@const dlStatus = downloadStatuses[String(album.id)]}
				<a
					href="/library/albums/{album.id}"
					class="group rounded-lg p-2 -m-2 transition-colors duration-200 hover:bg-muted/30"
					style="content-visibility: auto; contain-intrinsic-size: auto 220px;"
					draggable="true"
					ondragstart={(e) => {
						if (!e.dataTransfer) return;
						e.dataTransfer.setData('application/x-playlist-album', JSON.stringify({ albumId: album.id, title: album.title }));
						e.dataTransfer.setData('text/plain', album.title);
						e.dataTransfer.effectAllowed = 'copyMove';
					}}
				>
					<div class="relative aspect-square rounded-lg bg-muted flex items-center justify-center overflow-hidden mb-2">
						{#if album.cover_art_path}
							<img
								src={assetUrl(album.cover_art_path)}
								alt={album.title}
								class="size-full object-cover group-hover:scale-105 transition-transform duration-300"
								loading="lazy"
								decoding="async"
							/>
						{:else}
							<Disc class="size-12 text-muted-foreground" />
						{/if}
						{#if dlStatus?.status === 'complete'}
							<div class="absolute top-1.5 right-1.5 flex items-center justify-center size-5 rounded-full bg-success text-white">
								<Check class="size-3" />
							</div>
						{:else if dlStatus?.status === 'partial'}
							<div class="absolute top-1.5 right-1.5 rounded-full bg-warning/90 text-white text-[10px] font-bold px-1.5 py-0.5 leading-none">
								{dlStatus.total_local}/{dlStatus.total_expected}
							</div>
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

