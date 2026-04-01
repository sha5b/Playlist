<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { search as searchApi } from '$lib/api/library';
	import TrackTable from '$lib/components/library/TrackTable.svelte';
	import { Search, Disc, Users, Loader2 } from 'lucide-svelte';
	import type { SearchResults } from '$lib/types';

	let query = $state('');
	let results: SearchResults | null = $state(null);
	let searching = $state(false);
	let debounceTimer: ReturnType<typeof setTimeout>;

	function handleInput(e: Event) {
		const value = (e.target as HTMLInputElement).value;
		query = value;
		clearTimeout(debounceTimer);
		if (!value.trim()) {
			results = null;
			return;
		}
		debounceTimer = setTimeout(async () => {
			searching = true;
			try {
				results = await searchApi(value, 30);
			} catch (err) {
				console.error('Search failed:', err);
			} finally {
				searching = false;
			}
		}, 300);
	}

	const hasResults = $derived(
		results && (results.tracks.length > 0 || results.albums.length > 0 || results.artists.length > 0)
	);
</script>

<div class="space-y-6">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Search</h1>
		<p class="text-muted-foreground mt-1">Find tracks, albums, and artists</p>
	</div>

	<div class="relative max-w-md">
		<Search class="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
		<Input
			placeholder="Search your library..."
			class="pl-10"
			value={query}
			oninput={handleInput}
		/>
		{#if searching}
			<Loader2 class="absolute right-3 top-1/2 -translate-y-1/2 size-4 animate-spin text-muted-foreground" />
		{/if}
	</div>

	{#if !query.trim()}
		<div class="flex items-center justify-center h-48 rounded-lg border border-dashed border-border">
			<p class="text-muted-foreground text-sm">Start typing to search your library</p>
		</div>
	{:else if results && !hasResults}
		<div class="flex items-center justify-center h-48 rounded-lg border border-dashed border-border">
			<p class="text-muted-foreground text-sm">No results for "{query}"</p>
		</div>
	{:else if results}
		{#if results.artists.length > 0}
			<div>
				<h2 class="text-lg font-semibold mb-3">Artists</h2>
				<div class="flex gap-4 overflow-x-auto pb-2">
					{#each results.artists as artist}
						<a href="/library/artists/{artist.id}" class="text-center shrink-0 w-28 group">
							<div class="size-28 rounded-full bg-muted flex items-center justify-center mx-auto mb-2">
								<Users class="size-8 text-muted-foreground" />
							</div>
							<p class="text-sm font-medium truncate">{artist.name}</p>
							<p class="text-xs text-muted-foreground">{artist.track_count} tracks</p>
						</a>
					{/each}
				</div>
			</div>
		{/if}

		{#if results.albums.length > 0}
			<div>
				<h2 class="text-lg font-semibold mb-3">Albums</h2>
				<div class="flex gap-4 overflow-x-auto pb-2">
					{#each results.albums as album}
						<a href="/library/albums/{album.id}" class="shrink-0 w-36 group">
							<div class="aspect-square rounded-lg bg-muted flex items-center justify-center mb-2">
								<Disc class="size-10 text-muted-foreground" />
							</div>
							<p class="text-sm font-medium truncate">{album.title}</p>
							<p class="text-xs text-muted-foreground truncate">{album.artist_name ?? 'Unknown'}</p>
						</a>
					{/each}
				</div>
			</div>
		{/if}

		{#if results.tracks.length > 0}
			<div>
				<h2 class="text-lg font-semibold mb-3">Tracks</h2>
				<TrackTable tracks={results.tracks} />
			</div>
		{/if}
	{/if}
</div>
