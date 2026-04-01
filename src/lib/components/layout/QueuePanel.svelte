<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { X, Music, Trash2 } from 'lucide-svelte';
	import { player } from '$lib/stores/player.svelte';
	import { formatDuration } from '$lib/utils/format';
</script>

{#if player.queueOpen}
	<aside class="flex h-full w-80 flex-col border-l border-border bg-card shrink-0">
		<div class="flex items-center justify-between px-4 h-14 border-b border-border">
			<h2 class="text-sm font-semibold">Queue</h2>
			<div class="flex items-center gap-1">
				{#if player.queueTracks.length > 0}
					<Button
						variant="ghost"
						size="icon-sm"
						class="text-muted-foreground hover:text-foreground"
						onclick={() => player.clearQueue()}
					>
						<Trash2 class="size-3.5" />
					</Button>
				{/if}
				<Button
					variant="ghost"
					size="icon-sm"
					class="text-muted-foreground hover:text-foreground"
					onclick={() => player.toggleQueuePanel()}
				>
					<X class="size-4" />
				</Button>
			</div>
		</div>

		<div class="flex-1 overflow-y-auto">
			{#if player.queueTracks.length === 0}
				<div class="flex items-center justify-center h-32">
					<p class="text-sm text-muted-foreground">Queue is empty</p>
				</div>
			{:else}
				<div class="p-2">
					{#if player.currentTrack}
						<div class="px-2 py-1.5 mb-1">
							<p class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Now Playing</p>
						</div>
					{/if}

					{#each player.queueTracks as track, i}
						{@const isCurrent = player.queuePosition === i}
						{@const isUpcoming = player.queuePosition !== null && i > player.queuePosition}

						{#if isUpcoming && i === (player.queuePosition ?? 0) + 1}
							<div class="px-2 py-1.5 mt-2 mb-1">
								<p class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Up Next</p>
							</div>
						{/if}

						<div
							class="flex items-center gap-2 px-2 py-1.5 rounded-md group
								{isCurrent ? 'bg-primary/10' : 'hover:bg-muted/50'}"
						>
							<div class="size-8 shrink-0 rounded bg-muted flex items-center justify-center overflow-hidden">
								{#if track.cover_art_path}
									<img
										src="https://asset.localhost/{track.cover_art_path}"
										alt=""
										class="size-full object-cover"
									/>
								{:else}
									<Music class="size-3.5 text-muted-foreground" />
								{/if}
							</div>
							<div class="min-w-0 flex-1">
								<p class="text-xs font-medium truncate {isCurrent ? 'text-primary' : ''}">
									{track.title}
								</p>
								<p class="text-[10px] text-muted-foreground truncate">
									{track.artist_name ?? 'Unknown'}
								</p>
							</div>
							<span class="text-[10px] text-muted-foreground tabular-nums shrink-0">
								{formatDuration(track.duration_ms)}
							</span>
							{#if !isCurrent}
								<Button
									variant="ghost"
									size="icon-sm"
									class="opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-foreground size-6"
									onclick={() => player.removeFromQueue(i)}
								>
									<X class="size-3" />
								</Button>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
		</div>
	</aside>
{/if}
