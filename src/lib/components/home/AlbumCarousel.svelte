<script lang="ts">
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { Play, Disc } from 'lucide-svelte';
	import CoverArt from '$lib/components/shared/CoverArt.svelte';
	import Skeleton from '$lib/components/shared/Skeleton.svelte';
	import SectionHeader from './SectionHeader.svelte';
	import { player } from '$lib/stores/player.svelte';
	import { getAlbumTracks } from '$lib/api/library';
	import { toast } from 'svelte-sonner';
	import type { Album } from '$lib/types';

	let {
		title,
		icon,
		albums,
		loading = false,
		href,
	}: {
		title: string;
		icon: typeof Disc;
		albums: Album[];
		loading?: boolean;
		/** "Show all" destination for the section header. */
		href?: string;
	} = $props();

	async function playAlbum(e: MouseEvent, albumId: number) {
		// Keep the surrounding card link from navigating
		e.preventDefault();
		e.stopPropagation();
		try {
			const tracks = await getAlbumTracks(albumId);
			if (tracks.length > 0) {
				await player.playTracks(tracks.map((t) => t.id), 0);
			}
		} catch {
			toast.error('Failed to play album');
		}
	}
</script>

{#if loading || albums.length > 0}
	<section class="space-y-2.5">
		<SectionHeader {title} {icon} {href} />

		{#if loading}
			<div class="flex gap-2.5 overflow-hidden pb-1">
				{#each Array(7) as _}
					<div class="flex-shrink-0 w-[140px] p-1.5">
						<Skeleton class="aspect-square w-full rounded-md mb-1.5" />
						<Skeleton class="h-3.5 w-4/5 mb-1" />
						<Skeleton class="h-3 w-3/5" />
					</div>
				{/each}
			</div>
		{:else}
			<ScrollArea orientation="horizontal" class="w-full">
				<div class="flex gap-2.5 pb-1">
					{#each albums as album (album.id)}
						<!-- Stretched-link card: the <a> overlays the whole card (navigates to the
							album page); the play button sits above it (z-10) — avoids invalid
							nested-interactive HTML while keeping both actions clickable. -->
						<div class="relative flex-shrink-0 w-[140px] group rounded-lg p-1.5 transition-colors hover:bg-muted/30">
							<div class="aspect-square w-full rounded-md overflow-hidden bg-muted/60 mb-1.5 relative">
								<CoverArt
									src={album.cover_art_path}
									alt={album.title}
									class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
									iconClass="size-7 text-muted-foreground/40"
								/>
								<div class="absolute inset-0 bg-black/0 group-hover:bg-black/25 transition-colors"></div>
								<button
									class="absolute z-10 bottom-1.5 right-1.5 flex items-center justify-center size-9 rounded-full bg-primary text-primary-foreground shadow-lg opacity-0 translate-y-1 group-hover:opacity-100 group-hover:translate-y-0 focus-visible:opacity-100 focus-visible:translate-y-0 transition-all cursor-pointer hover:scale-105"
									onclick={(e) => playAlbum(e, album.id)}
									aria-label={`Play ${album.title || 'album'}`}
								>
									<Play class="size-4 fill-current" />
								</button>
							</div>
							<p class="text-[13px] font-medium truncate leading-tight">{album.title || 'Unknown Album'}</p>
							<p class="text-xs text-muted-foreground/70 truncate">
								{album.artist_name || 'Unknown Artist'}
								{#if album.year}
									&middot; {album.year}
								{/if}
							</p>
							<a
								href="/library/albums/{album.id}"
								class="absolute inset-0 rounded-lg focus-visible:ring-2 focus-visible:ring-ring outline-none"
								aria-label={`View ${album.title || 'album'}`}
							></a>
						</div>
					{/each}
				</div>
			</ScrollArea>
		{/if}
	</section>
{/if}
