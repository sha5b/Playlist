<script lang="ts">
	import { dndzone, SHADOW_ITEM_MARKER_PROPERTY_NAME, TRIGGERS } from 'svelte-dnd-action';
	import { GripVertical, X, Music } from 'lucide-svelte';
	import { Button } from '$lib/components/ui/button';
	import { player } from '$lib/stores/player.svelte';
	import { getAlbumTracks, getArtistTracks } from '$lib/api/library';
	import { formatDuration, assetUrl } from '$lib/utils/format';
	import type { QueueTrack } from '$lib/types';
	import { flip } from 'svelte/animate';

	interface Props {
		compact?: boolean;
	}

	let { compact = false }: Props = $props();

	// Only show tracks after current position (up next)
	let startOffset = $derived((player.queuePosition ?? -1) + 1);
	let upNextTracks = $derived(player.queueTracks.slice(startOffset));

	// svelte-dnd-action requires items with a unique `id` field
	interface DndItem {
		id: string;
		track: QueueTrack;
		queueIndex: number;
		[key: string]: unknown;
	}

	let dndItems: DndItem[] = $state([]);
	let isDragging = $state(false);

	// Sync dndItems when upNextTracks changes (but not during active drag)
	$effect(() => {
		if (!isDragging) {
			dndItems = upNextTracks.map((t, i) => ({
				id: `${t.id}-${startOffset + i}`,
				track: t,
				queueIndex: startOffset + i,
			}));
		}
	});

	function handleConsider(e: CustomEvent<{ items: DndItem[]; info: { trigger: string } }>) {
		dndItems = e.detail.items;
		if (e.detail.info.trigger === TRIGGERS.DRAG_STARTED) {
			isDragging = true;
		}
	}

	function handleFinalize(e: CustomEvent<{ items: DndItem[]; info: { trigger: string } }>) {
		const newItems = e.detail.items;

		// Check if an item was removed (dragged out)
		if (newItems.length < upNextTracks.length) {
			const newIds = new Set(newItems.map(item => item.id));
			const removed = dndItems.find(item => !newIds.has(item.id));
			if (removed) {
				player.removeFromQueue(removed.queueIndex);
			}
		} else {
			// Find which item moved and where
			dndItems = newItems;
			for (let i = 0; i < newItems.length; i++) {
				const item = newItems[i];
				const expectedQueueIndex = startOffset + i;
				if (item.queueIndex !== expectedQueueIndex) {
					player.moveInQueue(item.queueIndex, expectedQueueIndex);
					// Update all queueIndex values to match new positions
					for (let j = 0; j < newItems.length; j++) {
						newItems[j].queueIndex = startOffset + j;
					}
					break;
				}
			}
		}

		isDragging = false;
	}

	// Handle external drops (tracks, albums, artists)
	let dragOver = $state(false);
	const DRAG_TYPES = ['application/x-playlist-track', 'application/x-playlist-album', 'application/x-playlist-artist'];

	function handleDragOver(e: DragEvent) {
		if (e.dataTransfer && DRAG_TYPES.some((t) => Array.from(e.dataTransfer!.types).includes(t))) {
			e.preventDefault();
			e.dataTransfer.dropEffect = 'copy';
			dragOver = true;
		}
	}

	function handleDragLeave() {
		dragOver = false;
	}

	async function handleDrop(e: DragEvent) {
		dragOver = false;
		if (!e.dataTransfer) return;
		e.preventDefault();

		const trackData = e.dataTransfer.getData('application/x-playlist-track');
		if (trackData) {
			try {
				const { trackId } = JSON.parse(trackData);
				if (player.queueTracks.length === 0 || player.isStopped) {
					player.playTrack(trackId);
				} else {
					player.addToQueue(trackId);
				}
			} catch { /* ignore */ }
			return;
		}

		const albumData = e.dataTransfer.getData('application/x-playlist-album');
		if (albumData) {
			try {
				const { albumId } = JSON.parse(albumData);
				const tracks = await getAlbumTracks(albumId);
				const ids = tracks.map((t) => t.id);
				if (ids.length > 0) {
					if (player.queueTracks.length === 0 || player.isStopped) {
						player.playTracks(ids, 0);
					} else {
						for (const id of ids) await player.addToQueue(id);
					}
				}
			} catch { /* ignore */ }
			return;
		}

		const artistData = e.dataTransfer.getData('application/x-playlist-artist');
		if (artistData) {
			try {
				const { artistId } = JSON.parse(artistData);
				const tracks = await getArtistTracks(artistId);
				const ids = tracks.map((t) => t.id);
				if (ids.length > 0) {
					if (player.queueTracks.length === 0 || player.isStopped) {
						player.playTracks(ids, 0);
					} else {
						for (const id of ids) await player.addToQueue(id);
					}
				}
			} catch { /* ignore */ }
		}
	}

	const itemSize = $derived(compact ? 'py-1.5 gap-2' : 'py-2 gap-3');
	const thumbSize = $derived(compact ? 'size-8' : 'size-10');
	const titleSize = $derived(compact ? 'text-xs' : 'text-sm');
	const subSize = $derived(compact ? 'text-[10px]' : 'text-xs');
	const durationSize = $derived(compact ? 'text-[10px]' : 'text-xs');

	// Wrapper to attach svelte-dnd-action events using Svelte 5 syntax
	// svelte-dnd-action dispatches custom DOM events, handled via action wrapper
	function dndzoneWrapper(node: HTMLElement, options: Parameters<typeof dndzone>[1]) {
		const instance = dndzone(node, options);
		node.addEventListener('consider', ((e: Event) => handleConsider(e as CustomEvent)) as EventListener);
		node.addEventListener('finalize', ((e: Event) => handleFinalize(e as CustomEvent)) as EventListener);
		return {
			update: (newOptions: Parameters<typeof dndzone>[1]) => instance.update?.(newOptions),
			destroy: () => {
				node.removeEventListener('consider', handleConsider as EventListener);
				node.removeEventListener('finalize', handleFinalize as EventListener);
				instance.destroy?.();
			},
		};
	}
</script>

<div
	class="relative {dragOver ? 'ring-2 ring-primary/50 rounded-lg' : ''}"
	role="list"
	ondragover={handleDragOver}
	ondragleave={handleDragLeave}
	ondrop={handleDrop}
>
	{#if dndItems.length === 0}
		<div class="flex items-center justify-center {compact ? 'h-16' : 'h-24'}">
			<p class="text-xs text-muted-foreground">
				{dragOver ? 'Drop to add to queue' : 'Drag songs here to add up next'}
			</p>
		</div>
	{:else}
		<div
			use:dndzoneWrapper={{
				items: dndItems,
				flipDurationMs: 200,
				type: 'queue-track',
				dropTargetStyle: {},
				dropTargetClasses: ['ring-2', 'ring-primary/30', 'rounded-lg'],
				dragDisabled: false,
			}}
			class="space-y-0.5"
		>
			{#each dndItems as item (item.id)}
				{@const t = item.track}
				<div
					animate:flip={{ duration: 200 }}
					class="flex items-center {itemSize} px-2 rounded-md group hover:bg-muted/50 transition-colors
						{item[SHADOW_ITEM_MARKER_PROPERTY_NAME] ? 'opacity-50 border border-dashed border-primary/30 bg-primary/5' : ''}"
				>
					<div class="shrink-0 cursor-grab active:cursor-grabbing text-muted-foreground hover:text-foreground">
						<GripVertical class={compact ? 'size-3.5' : 'size-4'} />
					</div>
					<div class="{thumbSize} shrink-0 rounded bg-muted flex items-center justify-center overflow-hidden">
						{#if t?.cover_art_path}
							<img
								src={assetUrl(t.cover_art_path)}
								alt=""
								class="size-full object-cover"
								loading="lazy"
							/>
						{:else}
							<Music class={`${compact ? 'size-3.5' : 'size-4'} text-muted-foreground`} />
						{/if}
					</div>
					<div class="min-w-0 flex-1">
						<p class="{titleSize} font-medium truncate">{t?.title ?? ''}</p>
						<p class="{subSize} text-muted-foreground truncate">{t?.artist_name ?? 'Unknown'}</p>
					</div>
					<span class="{durationSize} text-muted-foreground tabular-nums shrink-0">
						{formatDuration(t?.duration_ms ?? null)}
					</span>
					<Button
						variant="ghost"
						size="icon-sm"
						class={`opacity-0 group-hover:opacity-100 text-muted-foreground hover:text-foreground ${compact ? 'size-6' : 'size-7'}`}
						onclick={() => player.removeFromQueue(item.queueIndex)}
					>
						<X class={compact ? 'size-3' : 'size-3.5'} />
					</Button>
				</div>
			{/each}
		</div>
	{/if}
</div>
