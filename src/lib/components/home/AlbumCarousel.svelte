<script lang="ts">
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { Button } from '$lib/components/ui/button';
	import { Play, ListPlus, Disc, Loader2 } from 'lucide-svelte';
	import { assetUrl } from '$lib/utils/format';
	import { player } from '$lib/stores/player.svelte';
	import { getAlbumTracks } from '$lib/api/library';
	import { toast } from 'svelte-sonner';
	import type { Album } from '$lib/types';

	let {
		title,
		icon,
		albums,
		loading = false,
	}: {
		title: string;
		icon: typeof Disc;
		albums: Album[];
		loading?: boolean;
	} = $props();

	async function playAlbum(albumId: number) {
		try {
			const tracks = await getAlbumTracks(albumId);
			if (tracks.length > 0) {
				await player.playTracks(tracks.map((t) => t.id), 0);
			}
		} catch (e) {
			toast.error('Failed to play album');
		}
	}

	async function playAll() {
		if (albums.length === 0) return;
		// Play all tracks from the first album
		try {
			const tracks = await getAlbumTracks(albums[0].id);
			if (tracks.length > 0) {
				await player.playTracks(tracks.map((t) => t.id), 0);
			}
		} catch {
			toast.error('Failed to play');
		}
	}

	const Icon = $derived(icon);
</script>

{#if loading || albums.length > 0}
	<section class="space-y-2.5">
		<div class="flex items-center justify-between">
			<div class="flex items-center gap-2">
				<Icon class="size-4 text-muted-foreground/60" />
				<h2 class="text-sm font-semibold tracking-wide">{title}</h2>
			</div>
			<div class="flex items-center gap-0.5">
				<Button variant="ghost" size="icon-xs" onclick={playAll} disabled={albums.length === 0} aria-label="Play all" class="text-muted-foreground hover:text-foreground">
					<Play class="size-3.5" />
				</Button>
			</div>
		</div>

		{#if loading}
			<div class="flex items-center justify-center py-6">
				<Loader2 class="size-5 animate-spin text-muted-foreground" />
			</div>
		{:else}
			<ScrollArea orientation="horizontal" class="w-full">
				<div class="flex gap-2.5 pb-1">
					{#each albums as album (album.id)}
						<button
							class="flex-shrink-0 w-[140px] group text-left rounded-lg p-1.5 transition-colors hover:bg-muted/40"
							onclick={() => playAlbum(album.id)}
							aria-label={`Play ${album.title || 'album'}`}
						>
							<div class="aspect-square w-full rounded-md overflow-hidden bg-muted/60 mb-1.5 relative">
								{#if album.cover_art_path}
									<img
										src={assetUrl(album.cover_art_path)}
										alt={album.title}
										class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
									/>
								{:else}
									<div class="w-full h-full flex items-center justify-center">
										<Disc class="size-7 text-muted-foreground/40" />
									</div>
								{/if}
								<div class="absolute inset-0 bg-black/0 group-hover:bg-black/25 transition-colors flex items-center justify-center">
									<Play class="size-8 text-white opacity-0 group-hover:opacity-100 transition-opacity drop-shadow-lg" />
								</div>
							</div>
							<p class="text-[13px] font-medium truncate leading-tight">{album.title || 'Unknown Album'}</p>
							<p class="text-xs text-muted-foreground/70 truncate">
								{album.artist_name || 'Unknown Artist'}
								{#if album.year}
									&middot; {album.year}
								{/if}
							</p>
						</button>
					{/each}
				</div>
			</ScrollArea>
		{/if}
	</section>
{/if}
