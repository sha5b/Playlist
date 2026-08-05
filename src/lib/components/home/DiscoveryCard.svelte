<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { Play, ListPlus, Music, RefreshCw } from 'lucide-svelte';
	import CoverArt from '$lib/components/shared/CoverArt.svelte';
	import Skeleton from '$lib/components/shared/Skeleton.svelte';
	import SectionHeader from './SectionHeader.svelte';
	import { player } from '$lib/stores/player.svelte';
	import { toast } from 'svelte-sonner';
	import type { Track } from '$lib/types';

	let {
		title,
		icon,
		tracks,
		loading = false,
		onRefresh,
		href,
	}: {
		title: string;
		icon: typeof Play;
		tracks: Track[];
		loading?: boolean;
		onRefresh?: () => void;
		/** "Show all" destination for the section header. */
		href?: string;
	} = $props();

	async function playAll() {
		if (tracks.length === 0) return;
		const ids = tracks.map((t) => t.id);
		await player.playTracks(ids, 0);
	}

	async function addAllToQueue() {
		if (tracks.length === 0) return;
		for (const track of tracks) {
			await player.addToQueue(track.id);
		}
		toast.success(`Added ${tracks.length} tracks to queue`);
	}

	async function playTrack(trackId: number) {
		await player.playTrack(trackId);
	}
</script>

{#if loading || tracks.length > 0}
	<section class="space-y-2.5">
		<SectionHeader {title} {icon} {href}>
			{#snippet actions()}
				{#if onRefresh}
					<Button variant="ghost" size="icon-xs" onclick={onRefresh} aria-label="Refresh" class="text-muted-foreground hover:text-foreground">
						<RefreshCw class="size-3.5" />
					</Button>
				{/if}
				<Button variant="ghost" size="icon-xs" onclick={playAll} disabled={tracks.length === 0} aria-label="Play all" class="text-muted-foreground hover:text-foreground">
					<Play class="size-3.5" />
				</Button>
				<Button variant="ghost" size="icon-xs" onclick={addAllToQueue} disabled={tracks.length === 0} aria-label="Add all to queue" class="text-muted-foreground hover:text-foreground">
					<ListPlus class="size-3.5" />
				</Button>
			{/snippet}
		</SectionHeader>

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
					{#each tracks as track (track.id)}
						<button
							class="flex-shrink-0 w-[140px] group text-left rounded-lg p-1.5 transition-colors hover:bg-muted/30 cursor-pointer"
							onclick={() => playTrack(track.id)}
							aria-label={`Play ${track.title || 'track'}`}
						>
							<div class="aspect-square w-full rounded-md overflow-hidden bg-muted/60 mb-1.5 relative">
								<CoverArt
									src={track.cover_art_path}
									alt={track.title}
									icon={Music}
									class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
									iconClass="size-7 text-muted-foreground/40"
								/>
								<div class="absolute inset-0 bg-black/0 group-hover:bg-black/25 transition-colors flex items-center justify-center">
									<Play class="size-8 text-white opacity-0 group-hover:opacity-100 transition-opacity drop-shadow-lg" />
								</div>
							</div>
							<p class="text-[13px] font-medium truncate leading-tight">{track.title || 'Unknown'}</p>
							<p class="text-xs text-muted-foreground/70 truncate">{track.artist_name || 'Unknown Artist'}</p>
						</button>
					{/each}
				</div>
			</ScrollArea>
		{/if}
	</section>
{/if}
