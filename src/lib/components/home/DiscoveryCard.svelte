<script lang="ts">
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
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
	<Card>
		<CardHeader class="flex flex-row items-center justify-between pb-2">
			<div class="flex items-center gap-2">
				<Icon class="size-4 text-muted-foreground" />
				<CardTitle class="text-base">{title}</CardTitle>
			</div>
			<div class="flex items-center gap-1">
				{#if onRefresh}
					<Button variant="ghost" size="icon-xs" onclick={onRefresh}>
						<RefreshCw class="size-3.5" />
					</Button>
				{/if}
				<Button variant="ghost" size="icon-xs" onclick={playAll} disabled={tracks.length === 0} aria-label="Play all">
					<Play class="size-3.5" />
				</Button>
				<Button variant="ghost" size="icon-xs" onclick={addAllToQueue} disabled={tracks.length === 0} aria-label="Add all to queue">
					<ListPlus class="size-3.5" />
				</Button>
			</div>
		</CardHeader>
		<CardContent class="pb-4">
			{#if loading}
				<div class="flex items-center justify-center py-4">
					<Loader2 class="size-5 animate-spin text-muted-foreground" />
				</div>
			{:else}
				<ScrollArea orientation="horizontal" class="w-full">
					<div class="flex gap-3 pb-2">
						{#each tracks as track (track.id)}
							<button
								class="flex-shrink-0 w-32 group text-left hover:bg-muted/50 rounded-md p-2 transition-colors"
								onclick={() => playTrack(track.id)}
								aria-label={`Play ${track.title || 'track'}`}
							>
								<div class="aspect-square w-full rounded-md overflow-hidden bg-muted mb-2 relative">
									{#if track.cover_art_path}
										<img
											src={assetUrl(track.cover_art_path)}
											alt={track.title}
											class="w-full h-full object-cover"
										/>
									{:else}
										<div class="w-full h-full flex items-center justify-center">
											<Music class="size-8 text-muted-foreground" />
										</div>
									{/if}
									<div class="absolute inset-0 bg-black/0 group-hover:bg-black/30 transition-colors flex items-center justify-center">
										<Play class="size-8 text-white opacity-0 group-hover:opacity-100 transition-opacity" />
									</div>
								</div>
								<p class="text-sm font-medium truncate">{track.title || 'Unknown'}</p>
								<p class="text-xs text-muted-foreground truncate">{track.artist_name || 'Unknown Artist'}</p>
							</button>
						{/each}
					</div>
				</ScrollArea>
			{/if}
		</CardContent>
	</Card>
{/if}
