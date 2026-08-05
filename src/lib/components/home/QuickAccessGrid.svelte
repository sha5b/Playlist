<script lang="ts">
	import {
		getRecentlyPlayedAlbums,
		getRecentlyAddedAlbums,
		getPlaylists,
		getAlbumTracks,
		getPlaylistTracks,
	} from '$lib/api/library';
	import { assetUrl } from '$lib/utils/format';
	import { player } from '$lib/stores/player.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import Skeleton from '$lib/components/shared/Skeleton.svelte';
	import SectionHeader from './SectionHeader.svelte';
	import { toast } from 'svelte-sonner';
	import { Disc, ListMusic, Play, History } from 'lucide-svelte';

	type QuickAccessItem = {
		type: 'album' | 'playlist';
		id: number;
		title: string;
		subtitle: string;
		cover: string | null;
	};

	let items = $state<QuickAccessItem[]>([]);
	let loading = $state(true);
	// True when the grid shows recently *played* albums (vs recently added fallback)
	let hasPlayHistory = $state(false);

	async function load() {
		loading = true;
		try {
			// Try recently played albums first
			let albums = await getRecentlyPlayedAlbums(6);
			hasPlayHistory = albums.length > 0;

			// Fallback to recently added if no play history
			if (albums.length === 0) {
				albums = await getRecentlyAddedAlbums(6);
			}

			const playlists = await getPlaylists();

			const albumItems: QuickAccessItem[] = albums.map((a) => ({
				type: 'album',
				id: a.id,
				title: a.title,
				subtitle: a.artist_name || 'Unknown Artist',
				cover: a.cover_art_path,
			}));

			const playlistItems: QuickAccessItem[] = playlists.slice(0, 2).map((p) => ({
				type: 'playlist',
				id: p.id,
				title: p.name,
				subtitle: `${p.track_count} track${p.track_count !== 1 ? 's' : ''}`,
				cover: p.cover_art_path,
			}));

			// Albums first, then playlists at the end
			items = [...albumItems, ...playlistItems].slice(0, 8);
		} catch (e) {
			console.error('Failed to load quick access:', e);
		} finally {
			loading = false;
		}
	}

	function hrefFor(item: QuickAccessItem) {
		return item.type === 'album'
			? `/library/albums/${item.id}`
			: `/library/playlists/${item.id}`;
	}

	async function playItem(e: MouseEvent, item: QuickAccessItem) {
		// Keep the surrounding card link from navigating
		e.preventDefault();
		e.stopPropagation();
		try {
			const ids =
				item.type === 'album'
					? (await getAlbumTracks(item.id)).map((t) => t.id)
					: (await getPlaylistTracks(item.id, 0, 500)).tracks.map((t) => t.id);
			if (ids.length > 0) {
				await player.playTracks(ids, 0);
			}
		} catch {
			toast.error(`Failed to play ${item.type}`);
		}
	}

	$effect(() => {
		libraryStore.version;
		load();
	});
</script>

<!-- Keep previous content visible while reloading (library version bumps) instead
	of unmounting the whole grid on every refresh -->
{#if loading && items.length === 0}
	<section class="space-y-2.5">
		<SectionHeader title="Jump back in" icon={History} />
		<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-2.5">
			{#each Array(8) as _}
				<div class="flex items-center gap-3 rounded-lg bg-muted/30 p-2">
					<Skeleton class="size-12 flex-shrink-0 rounded-md" />
					<div class="min-w-0 flex-1 space-y-1.5">
						<Skeleton class="h-3.5 w-4/5" />
						<Skeleton class="h-3 w-3/5" />
					</div>
				</div>
			{/each}
		</div>
	</section>
{:else if items.length > 0}
	<section class="space-y-2.5">
		<SectionHeader
			title={hasPlayHistory ? 'Jump back in' : 'From your library'}
			icon={History}
			href="/library/albums"
		/>
		<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-2.5">
			{#each items as item (item.type + item.id)}
				<!-- Stretched-link card: the <a> overlays the whole card (navigates to the
					album/playlist page); the play button sits above it (z-10) — avoids
					invalid nested-interactive HTML while keeping both actions clickable. -->
				<div
					class="relative flex items-center gap-3 rounded-lg bg-muted/30 hover:bg-muted/60 transition-colors p-2 text-left group overflow-hidden"
				>
					<div class="size-12 flex-shrink-0 rounded-md overflow-hidden bg-muted/60 relative">
						{#if item.cover}
							<img
								src={assetUrl(item.cover)}
								alt={item.title}
								class="w-full h-full object-cover"
								loading="lazy"
							/>
						{:else if item.type === 'playlist'}
							<div class="w-full h-full flex items-center justify-center">
								<ListMusic class="size-5 text-muted-foreground/40" />
							</div>
						{:else}
							<div class="w-full h-full flex items-center justify-center">
								<Disc class="size-5 text-muted-foreground/40" />
							</div>
						{/if}
					</div>
					<div class="min-w-0 flex-1">
						<p class="text-sm font-medium truncate">{item.title}</p>
						<p class="text-xs text-muted-foreground/70 truncate">{item.subtitle}</p>
					</div>
					<button
						class="relative z-10 opacity-0 group-hover:opacity-100 focus-visible:opacity-100 transition-opacity flex-shrink-0 mr-1 flex items-center justify-center size-8 rounded-full bg-primary text-primary-foreground shadow cursor-pointer hover:scale-105"
						onclick={(e) => playItem(e, item)}
						aria-label={`Play ${item.title}`}
					>
						<Play class="size-3.5 fill-current" />
					</button>
					<a
						href={hrefFor(item)}
						class="absolute inset-0 rounded-lg focus-visible:ring-2 focus-visible:ring-ring outline-none"
						aria-label={`Open ${item.title}`}
					></a>
				</div>
			{/each}
		</div>
	</section>
{/if}
