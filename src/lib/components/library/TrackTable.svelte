<script lang="ts">
	import { goto } from '$app/navigation';
	import { Music, Clock, Hash, Play, ListPlus, ListStart, Trash2, MoreHorizontal, Download, ArrowUp, ArrowDown } from 'lucide-svelte';
	import { formatDuration } from '$lib/utils/format';
	import CoverArt from '$lib/components/shared/CoverArt.svelte';
	import { player } from '$lib/stores/player.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Button } from '$lib/components/ui/button';
	import { toast } from 'svelte-sonner';
	import { scrollRestore } from '$lib/utils/scrollRestore';
	import type { Track } from '$lib/types';

	interface Placeholder {
		_placeholder: true;
		track_number: number;
		disc_number: number;
		title?: string;
	}

	type DisplayRow = (Track & { _placeholder?: never }) | Placeholder;

	const ROW_HEIGHT = 44; // px — matches py-2.5 + content
	const BUFFER_ROWS = 10; // extra rows rendered above/below viewport

	let {
		tracks,
		placeholders = [],
		onplay,
		ondelete,
		ondownload,
		navigable = true,
		sortBy = '',
		sortDir = 'asc',
		onsort,
		deleteLabel = 'Remove',
		referrer,
	}: {
		tracks: Track[];
		placeholders?: { track_number: number; disc_number: number; title?: string }[];
		onplay?: (track: Track) => void;
		ondelete?: (track: Track) => void;
		ondownload?: (placeholder: { track_number: number; disc_number: number; title?: string }) => void;
		navigable?: boolean;
		sortBy?: string;
		sortDir?: 'asc' | 'desc';
		onsort?: (column: string) => void;
		deleteLabel?: string;
		referrer?: { type: string; id?: number; label?: string };
	} = $props();

	// Merge tracks and placeholders, sorted by disc/track number (only when placeholders exist)
	const displayRows: DisplayRow[] = $derived.by(() => {
		if (placeholders.length === 0) return tracks;
		const rows: DisplayRow[] = [
			...tracks,
			...placeholders.map((p) => ({ _placeholder: true as const, ...p })),
		];
		rows.sort((a, b) => {
			const da = (a._placeholder ? a.disc_number : a.disc_number ?? 1) ?? 1;
			const db = (b._placeholder ? b.disc_number : b.disc_number ?? 1) ?? 1;
			if (da !== db) return da - db;
			const ta = (a._placeholder ? a.track_number : a.track_number ?? 0) ?? 0;
			const tb = (b._placeholder ? b.track_number : b.track_number ?? 0) ?? 0;
			return ta - tb;
		});
		return rows;
	});

	let scrollContainer: HTMLDivElement | undefined = $state();
	let scrollTop = $state(0);
	let containerHeight = $state(600);

	// Virtualization: compute visible range
	let startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - BUFFER_ROWS));
	let endIndex = $derived(Math.min(displayRows.length, Math.ceil((scrollTop + containerHeight) / ROW_HEIGHT) + BUFFER_ROWS));
	let visibleRows = $derived(displayRows.slice(startIndex, endIndex));
	let totalHeight = $derived(displayRows.length * ROW_HEIGHT);
	let offsetY = $derived(startIndex * ROW_HEIGHT);

	// Reset scroll to top when sort/filter changes (not on every tracks reference change)
	// Include the first track's id so page changes (same length) also reset scroll
	let scrollKey = $derived(`${sortBy}-${sortDir}-${tracks.length}-${tracks[0]?.id}`);
	// null until the first run: skip resetting on mount so the scrollRestore
	// action (which restores the saved position for back-navigation) isn't
	// immediately clobbered back to 0.
	let prevScrollKey: string | null = null;
	$effect(() => {
		if (prevScrollKey !== null && scrollKey !== prevScrollKey) {
			if (scrollContainer) {
				scrollContainer.scrollTop = 0;
				scrollTop = 0;
			}
		}
		prevScrollKey = scrollKey;
	});

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
			// Find the track's position in the tracks-only array
			const trackIndex = tracks.indexOf(track);
			player.playTracks(ids, trackIndex >= 0 ? trackIndex : index);
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

	// Track drag state to prevent onclick navigation after a drag
	let wasDragging = false;

	function songHref(trackId: number): string {
		if (referrer?.type && referrer?.id != null) {
			const params = new URLSearchParams({ from: referrer.type, fromId: String(referrer.id) });
			if (referrer.label) params.set('fromLabel', referrer.label);
			return `/library/songs/${trackId}?${params.toString()}`;
		}
		return `/library/songs/${trackId}`;
	}

	function rowKey(row: DisplayRow, index: number): string | number {
		if (row._placeholder) return `p-${row.disc_number}-${row.track_number}`;
		return row.id;
	}
</script>

{#if displayRows.length === 0}
	<div class="flex items-center justify-center h-48 rounded-xl border border-dashed border-border/60">
		<p class="text-muted-foreground text-sm">No tracks found</p>
	</div>
{:else}
	<div class="rounded-xl border border-border/60 overflow-hidden flex flex-col flex-1 min-h-0">
		<!-- Fixed header -->
		<div class="border-b border-border bg-muted/30 text-xs text-muted-foreground uppercase tracking-wider flex items-center shrink-0">
			<div class="w-12 px-4 py-3 text-center">
				<Hash class="size-3 inline" />
			</div>
			<button class="flex-1 px-4 py-3 text-left flex items-center gap-1 hover:text-foreground transition-colors" onclick={() => onsort?.('title')}>
				Title
				{#if sortBy === 'title'}
					{#if sortDir === 'asc'}<ArrowUp class="size-3" />{:else}<ArrowDown class="size-3" />{/if}
				{/if}
			</button>
			<button class="w-40 px-4 py-3 text-left flex items-center gap-1 hover:text-foreground transition-colors" onclick={() => onsort?.('artist_name')}>
				Artist
				{#if sortBy === 'artist_name'}
					{#if sortDir === 'asc'}<ArrowUp class="size-3" />{:else}<ArrowDown class="size-3" />{/if}
				{/if}
			</button>
			<div class="w-40 px-4 py-3 text-left hidden lg:block">Album</div>
			<button class="w-20 px-4 py-3 text-right flex items-center justify-end gap-1 hover:text-foreground transition-colors" onclick={() => onsort?.('duration_ms')}>
				<Clock class="size-3" />
				{#if sortBy === 'duration_ms'}
					{#if sortDir === 'asc'}<ArrowUp class="size-3" />{:else}<ArrowDown class="size-3" />{/if}
				{/if}
			</button>
			<div class="w-10"></div>
		</div>
		<!-- Virtualized scrollable body -->
		<div
			bind:this={scrollContainer}
			bind:clientHeight={containerHeight}
			onscroll={onScroll}
			use:scrollRestore
			class="overflow-y-auto flex-1"
		>
			<div style="height: {totalHeight}px; position: relative;">
				<div style="transform: translateY({offsetY}px);">
					{#each visibleRows as row, vi (rowKey(row, startIndex + vi))}
						{@const i = startIndex + vi}
						{#if row._placeholder}
							<!-- Placeholder row for missing track -->
							<div
								class="group border-b border-border/40 flex items-center opacity-50 hover:opacity-80 cursor-default transition-opacity"
								style="height: {ROW_HEIGHT}px;"
							>
								<div class="w-12 px-4 text-center text-sm tabular-nums text-muted-foreground">
									{row.track_number}
								</div>
								<div class="flex-1 px-4">
									<div class="flex items-center gap-3 min-w-0">
										<div class="size-9 rounded bg-muted flex items-center justify-center shrink-0">
											<Music class="size-4 text-muted-foreground" />
										</div>
										<div class="min-w-0">
											<p class="text-sm text-muted-foreground truncate">{row.title ?? `Track ${row.track_number}`}</p>
										</div>
									</div>
								</div>
								<div class="w-40 px-4 text-sm text-muted-foreground truncate">--</div>
								<div class="w-40 px-4 text-sm text-muted-foreground truncate hidden lg:block">--</div>
								<div class="w-20 px-4 text-sm text-muted-foreground text-right tabular-nums">--:--</div>
								<div class="w-10 flex items-center justify-center">
									{#if ondownload}
										<button
											class="opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground"
											title="Download this track"
											onclick={() => ondownload?.(row)}
										>
											<Download class="size-4" />
										</button>
									{/if}
								</div>
							</div>
						{:else}
							{@const track = row}
							{@const isCurrentTrack = player.currentTrack?.id === track.id}
							<div
								role="row"
								tabindex="0"
								draggable="true"
								ondragstart={(e) => {
									if (!e.dataTransfer) return;
									wasDragging = true;
									e.dataTransfer.setData('application/x-playlist-track', JSON.stringify({ trackId: track.id, title: track.title }));
									e.dataTransfer.setData('text/plain', track.title);
									e.dataTransfer.effectAllowed = 'copyMove';
								}}
								ondragend={() => { setTimeout(() => { wasDragging = false; }, 0); }}
								class="border-b border-border/40 hover:bg-muted/30 transition-colors cursor-pointer group flex items-center
									{isCurrentTrack ? 'bg-primary/5' : ''}"
								style="height: {ROW_HEIGHT}px;"
								onclick={() => { if (wasDragging) return; if (navigable) goto(songHref(track.id)); }}
								onkeydown={(e) => { if (e.key === 'Enter' && navigable) goto(songHref(track.id)); }}
							>
								<div
									class="w-12 px-4 text-center text-sm tabular-nums
									{isCurrentTrack ? 'text-primary' : 'text-muted-foreground'}"
								>
									<span class="group-hover:hidden">{track.track_number ?? i + 1}</span>
									<button
										class="hidden group-hover:inline-flex items-center justify-center"
										onclick={(e) => { e.stopPropagation(); handlePlay(track, i); }}
									>
										<Play class="size-3.5" fill="currentColor" />
									</button>
								</div>
								<div class="flex-1 px-4">
									<div class="flex items-center gap-3 min-w-0">
										<CoverArt
											src={track.cover_art_path}
											alt=""
											class="size-9 rounded object-cover shrink-0"
											iconClass="size-4 text-muted-foreground"
											icon={Music}
										/>
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
									<div role="presentation" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
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
														{deleteLabel}
													</DropdownMenu.Item>
												{/if}
											</DropdownMenu.Content>
										</DropdownMenu.Root>
									</div>
								</div>
							</div>
						{/if}
					{/each}
				</div>
			</div>
		</div>
	</div>
{/if}
