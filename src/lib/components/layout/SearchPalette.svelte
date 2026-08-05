<script lang="ts">
	import { goto } from '$app/navigation';
	import * as Dialog from '$lib/components/ui/dialog';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import CoverArt from '$lib/components/shared/CoverArt.svelte';
	import { search } from '$lib/api/library';
	import { player } from '$lib/stores/player.svelte';
	import type { SearchResults } from '$lib/types';
	import { Search, Music, Disc, Users, Play, LoaderCircle } from 'lucide-svelte';

	let { open = $bindable(false) }: { open?: boolean } = $props();

	interface Item {
		kind: 'track' | 'album' | 'artist';
		id: number;
		title: string;
		subtitle: string | null;
		cover: string | null;
	}

	let query = $state('');
	let results: SearchResults | null = $state(null);
	let loading = $state(false);
	let selectedIndex = $state(0);
	let inputEl: HTMLInputElement | null = $state(null);

	let debounceTimer: ReturnType<typeof setTimeout> | undefined;
	let requestSeq = 0;

	const items = $derived.by<Item[]>(() => {
		if (!results) return [];
		const out: Item[] = [];
		for (const t of results.tracks) {
			out.push({ kind: 'track', id: t.id, title: t.title, subtitle: t.artist_name, cover: t.cover_art_path });
		}
		for (const a of results.albums) {
			out.push({ kind: 'album', id: a.id, title: a.title, subtitle: a.album_artist, cover: a.cover_art_path });
		}
		for (const ar of results.artists) {
			out.push({ kind: 'artist', id: ar.id, title: ar.name, subtitle: null, cover: ar.image_path });
		}
		return out;
	});

	// Group offsets into the flat `items` list (for keyboard selection highlighting)
	const trackOffset = 0;
	const albumOffset = $derived.by(() => (results ? results.tracks.length : 0));
	const artistOffset = $derived.by(() =>
		results ? results.tracks.length + results.albums.length : 0
	);

	const hasResults = $derived(items.length > 0);
	const showEmpty = $derived(!loading && query.trim().length > 0 && results !== null && items.length === 0);

	// Reset state each time the palette opens
	$effect(() => {
		if (open) {
			query = '';
			results = null;
			loading = false;
			selectedIndex = 0;
			requestSeq++;
			clearTimeout(debounceTimer);
		}
	});

	// Debounced search-as-you-type
	$effect(() => {
		const q = query.trim();
		clearTimeout(debounceTimer);
		const seq = ++requestSeq;
		if (!q) {
			results = null;
			loading = false;
			return;
		}
		loading = true;
		debounceTimer = setTimeout(async () => {
			try {
				const r = await search(q, 8);
				if (seq === requestSeq) {
					results = r;
					selectedIndex = 0;
				}
			} catch (err) {
				if (seq === requestSeq) {
					results = { tracks: [], albums: [], artists: [] };
					console.warn('Search failed:', err);
				}
			} finally {
				if (seq === requestSeq) loading = false;
			}
		}, 200);
	});

	function scrollSelectedIntoView(index: number) {
		requestAnimationFrame(() => {
			document
				.getElementById(`search-palette-item-${index}`)
				?.scrollIntoView({ block: 'nearest' });
		});
	}

	function activate(item: Item) {
		open = false;
		switch (item.kind) {
			case 'track':
				goto(`/library/songs/${item.id}`);
				break;
			case 'album':
				goto(`/library/albums/${item.id}`);
				break;
			case 'artist':
				goto(`/library/artists/${item.id}`);
				break;
		}
	}

	function playTrack(e: MouseEvent, trackId: number) {
		e.stopPropagation();
		player.playTrack(trackId);
	}

	function handleInputKeydown(e: KeyboardEvent) {
		if (!hasResults) return;
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			selectedIndex = (selectedIndex + 1) % items.length;
			scrollSelectedIntoView(selectedIndex);
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			selectedIndex = (selectedIndex - 1 + items.length) % items.length;
			scrollSelectedIntoView(selectedIndex);
		} else if (e.key === 'Enter') {
			e.preventDefault();
			const item = items[selectedIndex];
			if (item) activate(item);
		}
	}
</script>

{#snippet row(item: Item, index: number)}
	<div
		id="search-palette-item-{index}"
		role="option"
		aria-selected={index === selectedIndex}
		tabindex="-1"
		class="group flex w-full cursor-pointer items-center gap-3 rounded-lg px-2 py-1.5 text-left text-sm transition-colors
			{index === selectedIndex ? 'bg-accent text-accent-foreground' : 'text-foreground hover:bg-accent/50'}"
		onclick={() => activate(item)}
		onkeydown={(e) => { if (e.key === 'Enter') activate(item); }}
		onmousemove={() => (selectedIndex = index)}
	>
		<CoverArt
			src={item.cover}
			alt=""
			class="size-8 shrink-0 rounded object-cover {item.kind === 'artist' ? 'rounded-full' : ''}"
			iconClass="size-4 text-muted-foreground/40"
			icon={item.kind === 'artist' ? Users : item.kind === 'album' ? Disc : Music}
		/>
		<div class="min-w-0 flex-1">
			<div class="truncate font-medium">{item.title}</div>
			{#if item.subtitle}
				<div class="truncate text-xs text-muted-foreground">{item.subtitle}</div>
			{/if}
		</div>
		{#if item.kind === 'track'}
			<button
				class="inline-flex size-7 shrink-0 items-center justify-center rounded-full text-muted-foreground opacity-0 transition-opacity hover:bg-primary hover:text-primary-foreground focus-visible:opacity-100 group-hover:opacity-100 {index === selectedIndex ? 'opacity-100' : ''}"
				aria-label="Play {item.title}"
				onclick={(e) => playTrack(e, item.id)}
			>
				<Play class="size-3.5" />
			</button>
		{/if}
	</div>
{/snippet}

<Dialog.Root bind:open>
	<Dialog.Content
		showCloseButton={false}
		class="top-[15%] w-full translate-y-0 gap-0 overflow-hidden p-0 sm:max-w-xl"
	>
		<Dialog.Title class="sr-only">Search library</Dialog.Title>
		<Dialog.Description class="sr-only">
			Search tracks, albums and artists. Use arrow keys to navigate, Enter to open.
		</Dialog.Description>

		<div class="flex items-center gap-2 border-b border-border px-3">
			{#if loading}
				<LoaderCircle class="size-4 shrink-0 animate-spin text-muted-foreground" />
			{:else}
				<Search class="size-4 shrink-0 text-muted-foreground" />
			{/if}
			<!-- svelte-ignore a11y_autofocus -->
			<input
				bind:this={inputEl}
				bind:value={query}
				onkeydown={handleInputKeydown}
				autofocus
				type="text"
				placeholder="Search tracks, albums, artists…"
				class="h-11 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
				role="combobox"
				aria-expanded={hasResults}
				aria-controls="search-palette-list"
				aria-autocomplete="list"
			/>
			<kbd class="rounded border border-border bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">Esc</kbd>
		</div>

		{#if hasResults}
			<ScrollArea class="max-h-[55vh]">
				<div id="search-palette-list" role="listbox" aria-label="Search results" class="flex flex-col gap-1 p-2">
					{#if results && results.tracks.length > 0}
						<div class="px-2 pt-1 pb-0.5 text-[10px] font-semibold tracking-wider text-muted-foreground uppercase">Tracks</div>
						{#each results.tracks as _, i (results.tracks[i].id)}
							{@render row(items[trackOffset + i], trackOffset + i)}
						{/each}
					{/if}
					{#if results && results.albums.length > 0}
						<div class="px-2 pt-2 pb-0.5 text-[10px] font-semibold tracking-wider text-muted-foreground uppercase">Albums</div>
						{#each results.albums as _, i (results.albums[i].id)}
							{@render row(items[albumOffset + i], albumOffset + i)}
						{/each}
					{/if}
					{#if results && results.artists.length > 0}
						<div class="px-2 pt-2 pb-0.5 text-[10px] font-semibold tracking-wider text-muted-foreground uppercase">Artists</div>
						{#each results.artists as _, i (results.artists[i].id)}
							{@render row(items[artistOffset + i], artistOffset + i)}
						{/each}
					{/if}
				</div>
			</ScrollArea>
			<div class="flex items-center gap-3 border-t border-border px-3 py-2 text-[11px] text-muted-foreground">
				<span class="flex items-center gap-1">
					<kbd class="rounded border border-border bg-muted px-1 py-0.5 font-medium">↑</kbd>
					<kbd class="rounded border border-border bg-muted px-1 py-0.5 font-medium">↓</kbd>
					navigate
				</span>
				<span class="flex items-center gap-1">
					<kbd class="rounded border border-border bg-muted px-1 py-0.5 font-medium">↵</kbd>
					open
				</span>
			</div>
		{:else if showEmpty}
			<div class="flex flex-col items-center gap-2 px-4 py-10 text-center">
				<Search class="size-6 text-muted-foreground/40" />
				<p class="text-sm text-muted-foreground">No results for “{query.trim()}”</p>
			</div>
		{:else}
			<div class="px-4 py-10 text-center text-sm text-muted-foreground">
				Start typing to search your library
			</div>
		{/if}
	</Dialog.Content>
</Dialog.Root>
