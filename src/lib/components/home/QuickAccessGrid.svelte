<script lang="ts">
	import { getRecentlyPlayedAlbums, getRecentlyAddedAlbums, getPlaylists, getAlbumTracks } from '$lib/api/library';
	import { assetUrl } from '$lib/utils/format';
	import { player } from '$lib/stores/player.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { goto } from '$app/navigation';
	import { Music, Disc, ListMusic, Play } from 'lucide-svelte';
	import type { Album, Playlist } from '$lib/types';

	type QuickAccessItem = {
		type: 'album' | 'playlist';
		id: number;
		title: string;
		subtitle: string;
		cover: string | null;
	};

	let items = $state<QuickAccessItem[]>([]);
	let loading = $state(true);

	async function load() {
		loading = true;
		try {
			// Try recently played albums first
			let albums = await getRecentlyPlayedAlbums(6);

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

			// Interleave: albums first, then playlists at the end
			items = [...albumItems, ...playlistItems].slice(0, 8);
		} catch (e) {
			console.error('Failed to load quick access:', e);
		} finally {
			loading = false;
		}
	}

	async function handleClick(item: QuickAccessItem) {
		if (item.type === 'album') {
			// Play album tracks
			try {
				const tracks = await getAlbumTracks(item.id);
				if (tracks.length > 0) {
					await player.playTracks(tracks.map((t) => t.id), 0);
				}
			} catch {
				// Navigate to album page as fallback
				goto(`/library/albums/${item.id}`);
			}
		} else {
			goto(`/library/playlists/${item.id}`);
		}
	}

	$effect(() => {
		libraryStore.version;
		load();
	});
</script>

<!-- Keep previous content visible while reloading (library version bumps) instead
	of unmounting the whole grid on every refresh -->
{#if items.length > 0}
	<section class="space-y-2.5">
		<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-2.5">
			{#each items as item (item.type + item.id)}
				<button
					class="flex items-center gap-3 rounded-lg bg-muted/30 hover:bg-muted/60 transition-colors p-2 text-left group overflow-hidden"
					onclick={() => handleClick(item)}
				>
					<div class="size-12 flex-shrink-0 rounded-md overflow-hidden bg-muted/60 relative">
						{#if item.cover}
							<img
								src={assetUrl(item.cover)}
								alt={item.title}
								class="w-full h-full object-cover"
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
					<div class="opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 pr-1">
						<Play class="size-4 text-foreground" />
					</div>
				</button>
			{/each}
		</div>
	</section>
{/if}
