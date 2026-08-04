<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { X, Music, Trash2, ListPlus, SkipBack } from 'lucide-svelte';
	import { player } from '$lib/stores/player.svelte';
	import { formatDuration } from '$lib/utils/format';
	import CoverArt from '$lib/components/shared/CoverArt.svelte';
	import { hasDragData, processQueueDrop } from '$lib/utils/queueDrop';
	import DndQueueList from '$lib/components/player/DndQueueList.svelte';

	let dragOver = $state(false);

	// The queue-order previous track — this is what player.prev() will actually
	// play, so display and action always agree (player.previousTrack can differ
	// under shuffle).
	const queuePrevTrack = $derived(
		player.queuePosition !== null && player.queuePosition > 0
			? player.queueTracks[player.queuePosition - 1]
			: null
	);

	// Count the rows the panel actually renders (now playing + up next)
	const visibleCount = $derived(
		(player.currentTrack ? 1 : 0) +
			(player.queuePosition !== null
				? Math.max(0, player.queueTracks.length - player.queuePosition - 1)
				: player.queueTracks.length)
	);

	function handleDragOver(e: DragEvent) {
		if (hasDragData(e.dataTransfer)) {
			e.preventDefault();
			e.dataTransfer!.dropEffect = 'copy';
			dragOver = true;
		}
	}

	function handleDragLeave(e: DragEvent) {
		if (!e.currentTarget || !(e.currentTarget as HTMLElement).contains(e.relatedTarget as Node)) {
			dragOver = false;
		}
	}

	async function processDrop(e: DragEvent) {
		dragOver = false;
		e.preventDefault();
		await processQueueDrop(e.dataTransfer);
	}
</script>

<!-- Edge drop zone when queue is closed: always 12px wide to be hittable -->
{#if !player.queueOpen}
	<div
		role="region"
		aria-label="Drop zone to add tracks to queue"
		class="h-full shrink-0 transition-all
			{dragOver ? 'w-48 bg-primary/10 border-l-2 border-primary' : 'w-3 hover:w-6 hover:bg-muted/20'}"
		ondragover={(e) => {
			if (hasDragData(e.dataTransfer)) {
				e.preventDefault();
				e.dataTransfer!.dropEffect = 'copy';
				dragOver = true;
			}
		}}
		ondragleave={(e) => {
			if (!e.currentTarget || !(e.currentTarget as HTMLElement).contains(e.relatedTarget as Node)) {
				dragOver = false;
			}
		}}
		ondrop={(e) => {
			processDrop(e);
			if (!player.queueOpen) player.toggleQueuePanel();
		}}
	>
		{#if dragOver}
			<div class="flex items-center justify-center h-full">
				<div class="flex flex-col items-center gap-2 text-primary">
					<ListPlus class="size-6" />
					<p class="text-xs font-medium">Drop here</p>
				</div>
			</div>
		{/if}
	</div>
{/if}

{#if player.queueOpen}
	<aside
		class="flex h-full w-80 flex-col border-l shrink-0 transition-colors
			{dragOver ? 'border-primary bg-primary/5' : 'border-border bg-card'}"
		ondragover={handleDragOver}
		ondragleave={handleDragLeave}
		ondrop={processDrop}
	>
		<div class="flex items-center justify-between px-4 h-14 border-b border-border">
			<h2 class="text-sm font-semibold">
				Queue{#if visibleCount > 0}<span class="text-muted-foreground font-normal ml-1">({visibleCount})</span>{/if}
			</h2>
			<div class="flex items-center gap-1">
				{#if player.queueTracks.length > 0}
					<Button
						variant="ghost"
						size="icon-sm"
						class="text-muted-foreground hover:text-foreground"
						onclick={() => player.clearQueue()}
						aria-label="Clear queue"
					>
						<Trash2 class="size-3.5" />
					</Button>
				{/if}
				<Button
					variant="ghost"
					size="icon-sm"
					class="text-muted-foreground hover:text-foreground"
					onclick={() => player.toggleQueuePanel()}
					aria-label="Close queue panel"
				>
					<X class="size-4" />
				</Button>
			</div>
		</div>

		<div class="flex-1 overflow-y-auto">
			{#if player.queueTracks.length === 0 && !dragOver}
				<div class="flex items-center justify-center h-32">
					<p class="text-sm text-muted-foreground">Queue is empty</p>
				</div>
			{:else if player.queueTracks.length === 0 && dragOver}
				<div class="flex flex-col items-center justify-center h-32 gap-2">
					<ListPlus class="size-8 text-primary" />
					<p class="text-sm text-primary font-medium">Drop to add to queue</p>
				</div>
			{:else}
				<div class="p-2">
					{#if queuePrevTrack}
						<div class="px-2 py-1.5 mb-1">
							<p class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Previously Played</p>
						</div>
						<button
							class="w-full flex items-center gap-2 px-2 py-1.5 rounded-md bg-muted/30 hover:bg-muted/50 transition-colors text-left"
							onclick={() => player.prev()}
						>
							<div class="size-8 shrink-0 rounded bg-muted flex items-center justify-center overflow-hidden">
								<CoverArt
									src={queuePrevTrack.cover_art_path}
									alt=""
									class="size-full object-cover"
									iconClass="size-3.5 text-muted-foreground"
									icon={Music}
								/>
							</div>
							<div class="min-w-0 flex-1">
								<p class="text-xs font-medium truncate text-muted-foreground">
									{queuePrevTrack.title}
								</p>
								<p class="text-[10px] text-muted-foreground/60 truncate">
									{queuePrevTrack.artist_name ?? 'Unknown'}
								</p>
							</div>
							<SkipBack class="size-3 text-muted-foreground shrink-0" />
						</button>
					{/if}

					{#if player.currentTrack}
						<div class="px-2 py-1.5 mb-1 {queuePrevTrack ? 'mt-3' : ''}">
							<p class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Now Playing</p>
						</div>
						<div class="flex items-center gap-2 px-2 py-1.5 rounded-md bg-primary/10">
							<div class="size-8 shrink-0 rounded bg-muted flex items-center justify-center overflow-hidden">
								<CoverArt
									src={player.currentTrack.cover_art_path}
									alt=""
									class="size-full object-cover"
									iconClass="size-3.5 text-muted-foreground"
									icon={Music}
								/>
							</div>
							<div class="min-w-0 flex-1">
								<p class="text-xs font-medium truncate text-primary">
									{player.currentTrack.title}
								</p>
								<p class="text-[10px] text-muted-foreground truncate">
									{player.currentTrack.artist_name ?? 'Unknown'}
								</p>
							</div>
							<span class="text-[10px] text-muted-foreground tabular-nums shrink-0">
								{formatDuration(player.currentTrack.duration_ms)}
							</span>
						</div>
					{/if}

					{#if player.queuePosition !== null && player.queuePosition < player.queueTracks.length - 1}
						<div class="px-2 py-1.5 mt-3 mb-1">
							<p class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Up Next</p>
						</div>
					{/if}
					<DndQueueList compact />

					{#if dragOver}
						<div class="flex items-center justify-center h-12 mt-2 rounded-md border border-dashed border-primary/50 bg-primary/5">
							<p class="text-xs text-primary font-medium">Drop to add to queue</p>
						</div>
					{/if}
				</div>
			{/if}
		</div>
	</aside>
{/if}
