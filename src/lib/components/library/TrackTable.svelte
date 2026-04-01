<script lang="ts">
	import { Music, Clock, Hash, Play, ListPlus, ListStart, Trash2, MoreHorizontal } from 'lucide-svelte';
	import { formatDuration, assetUrl } from '$lib/utils/format';
	import { player } from '$lib/stores/player.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Button } from '$lib/components/ui/button';
	import { toast } from 'svelte-sonner';
	import type { Track } from '$lib/types';

	const ROW_HEIGHT = 44; // px — matches py-2.5 + content
	const BUFFER_ROWS = 10; // extra rows rendered above/below viewport

	let {
		tracks,
		onplay,
		ondelete,
	}: {
		tracks: Track[];
		onplay?: (track: Track) => void;
		ondelete?: (track: Track) => void;
	} = $props();

	let scrollContainer: HTMLDivElement | undefined = $state();
	let scrollTop = $state(0);
	let containerHeight = $state(600);

	// Virtualization: compute visible range
	let startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - BUFFER_ROWS));
	let endIndex = $derived(Math.min(tracks.length, Math.ceil((scrollTop + containerHeight) / ROW_HEIGHT) + BUFFER_ROWS));
	let visibleTracks = $derived(tracks.slice(startIndex, endIndex));
	let totalHeight = $derived(tracks.length * ROW_HEIGHT);
	let offsetY = $derived(startIndex * ROW_HEIGHT);

	function onScroll() {
		if (scrollContainer) {
			scrollTop = scrollContainer.scrollTop;
		}
	}

	function handlePlay(track: Track, index: number) {
		if (onplay) {
			onplay(track);
		} else {
			const ids = tracks.map((t) => t.id);
			player.playTracks(ids, index);
		}
	}

	async function handleAddToQueue(track: Track) {
		await player.addToQueue(track.id);
		toast.success(`Added "${track.title}" to queue`);
	}

	async function handlePlayNext(track: Track) {
		await player.addNext(track.id);
		toast.success(`"${track.title}" will play next`);
	}
</script>

{#if tracks.length === 0}
	<div class="flex items-center justify-center h-48 rounded-lg border border-dashed border-border">
		<p class="text-muted-foreground text-sm">No tracks found</p>
	</div>
{:else}
	<div class="rounded-lg border border-border overflow-hidden flex flex-col" style="max-height: 80vh;">
		<!-- Fixed header -->
		<div class="border-b border-border bg-muted/30 text-xs text-muted-foreground uppercase tracking-wider flex items-center shrink-0">
			<div class="w-12 px-4 py-3 text-center">
				<Hash class="size-3 inline" />
			</div>
			<div class="flex-1 px-4 py-3 text-left">Title</div>
			<div class="w-40 px-4 py-3 text-left">Artist</div>
			<div class="w-40 px-4 py-3 text-left hidden lg:block">Album</div>
			<div class="w-20 px-4 py-3 text-right">
				<Clock class="size-3 inline" />
			</div>
			<div class="w-10"></div>
		</div>
		<!-- Virtualized scrollable body -->
		<div
			bind:this={scrollContainer}
			bind:clientHeight={containerHeight}
			onscroll={onScroll}
			class="overflow-y-auto flex-1"
		>
			<div style="height: {totalHeight}px; position: relative;">
				<div style="transform: translateY({offsetY}px);">
					{#each visibleTracks as track, vi (track.id)}
						{@const i = startIndex + vi}
						{@const isCurrentTrack = player.currentTrack?.id === track.id}
						<div
							role="row"
							tabindex="0"
							class="border-b border-border/50 hover:bg-muted/50 transition-colors cursor-pointer group flex items-center
								{isCurrentTrack ? 'bg-primary/5' : ''}"
							style="height: {ROW_HEIGHT}px;"
							ondblclick={() => handlePlay(track, i)}
						>
							<div
								class="w-12 px-4 text-center text-sm tabular-nums
								{isCurrentTrack ? 'text-primary' : 'text-muted-foreground'}"
							>
								<span class="group-hover:hidden">{i + 1}</span>
								<button
									class="hidden group-hover:inline-flex items-center justify-center"
									onclick={() => handlePlay(track, i)}
								>
									<Play class="size-3.5" fill="currentColor" />
								</button>
							</div>
							<div class="flex-1 px-4">
								<div class="flex items-center gap-3 min-w-0">
									{#if track.cover_art_path}
										<img
											src={assetUrl(track.cover_art_path)}
											alt=""
											class="size-9 rounded object-cover shrink-0"
											loading="lazy"
										/>
									{:else}
										<div class="size-9 rounded bg-muted flex items-center justify-center shrink-0">
											<Music class="size-4 text-muted-foreground" />
										</div>
									{/if}
									<div class="min-w-0">
										<p class="text-sm font-medium truncate {isCurrentTrack ? 'text-primary' : ''}">
											{track.title}
										</p>
									</div>
								</div>
							</div>
							<div class="w-40 px-4 text-sm text-muted-foreground truncate">
								{track.artist_name ?? 'Unknown Artist'}
							</div>
							<div class="w-40 px-4 text-sm text-muted-foreground truncate hidden lg:block">
								{track.album_title ?? '--'}
							</div>
							<div class="w-20 px-4 text-sm text-muted-foreground text-right tabular-nums">
								{formatDuration(track.duration_ms)}
							</div>
							<div class="w-10 px-2">
								<DropdownMenu.Root>
									<DropdownMenu.Trigger>
										<Button
											variant="ghost"
											size="icon-sm"
											class="opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-foreground size-7"
										>
											<MoreHorizontal class="size-4" />
										</Button>
									</DropdownMenu.Trigger>
									<DropdownMenu.Content align="end" class="w-48">
										<DropdownMenu.Item onclick={() => handlePlay(track, i)}>
											<Play class="size-4" />
											Play
										</DropdownMenu.Item>
										<DropdownMenu.Item onclick={() => handlePlayNext(track)}>
											<ListStart class="size-4" />
											Play Next
										</DropdownMenu.Item>
										<DropdownMenu.Item onclick={() => handleAddToQueue(track)}>
											<ListPlus class="size-4" />
											Add to Queue
										</DropdownMenu.Item>
										{#if ondelete}
											<DropdownMenu.Separator />
											<DropdownMenu.Item
												class="text-destructive focus:text-destructive"
												onclick={() => ondelete?.(track)}
											>
												<Trash2 class="size-4" />
												Remove
											</DropdownMenu.Item>
										{/if}
									</DropdownMenu.Content>
								</DropdownMenu.Root>
							</div>
						</div>
					{/each}
				</div>
			</div>
		</div>
	</div>
{/if}
