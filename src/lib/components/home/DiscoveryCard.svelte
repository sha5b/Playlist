<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { Play, ListPlus, Music, RefreshCw, Loader2 } from 'lucide-svelte';
	import { assetUrl } from '$lib/utils/format';
	import { player } from '$lib/stores/player.svelte';
	import { toast } from 'svelte-sonner';
	import type { Track } from '$lib/types';

	let {
		title,
		icon,
		tracks,
		loading = false,
		onRefresh,
	}: {
		title: string;
		icon: typeof Play;
		tracks: Track[];
		loading?: boolean;
		onRefresh?: () => void;
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

	const Icon = $derived(icon);
</script>

{#if loading || tracks.length > 0}
	<section class="space-y-2.5">
		<div class="flex items-center justify-between">
			<div class="flex items-center gap-2">
				<Icon class="size-4 text-muted-foreground/60" />
				<h2 class="text-sm font-semibold tracking-wide">{title}</h2>
			</div>
			<div class="flex items-center gap-0.5">
				{#if onRefresh}
					<Button variant="ghost" size="icon-xs" onclick={onRefresh} class="text-muted-foreground hover:text-foreground">
						<RefreshCw class="size-3.5" />
					</Button>
				{/if}
				<Button variant="ghost" size="icon-xs" onclick={playAll} disabled={tracks.length === 0} aria-label="Play all" class="text-muted-foreground hover:text-foreground">
					<Play class="size-3.5" />
				</Button>
				<Button variant="ghost" size="icon-xs" onclick={addAllToQueue} disabled={tracks.length === 0} aria-label="Add all to queue" class="text-muted-foreground hover:text-foreground">
					<ListPlus class="size-3.5" />
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
					{#each tracks as track (track.id)}
						<button
							class="flex-shrink-0 w-[130px] group text-left rounded-lg p-1.5 transition-colors hover:bg-muted/30"
							onclick={() => playTrack(track.id)}
							aria-label={`Play ${track.title || 'track'}`}
						>
							<div class="aspect-square w-full rounded-md overflow-hidden bg-muted/60 mb-1.5 relative">
								{#if track.cover_art_path}
									<img
										src={assetUrl(track.cover_art_path)}
										alt={track.title}
										class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
									/>
								{:else}
									<div class="w-full h-full flex items-center justify-center">
										<Music class="size-7 text-muted-foreground/40" />
									</div>
								{/if}
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
