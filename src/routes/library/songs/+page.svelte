<script lang="ts">
	import { untrack } from 'svelte';
	import { getTracks, cleanupDuplicateTracks } from '$lib/api/library';
	import TrackTable from '$lib/components/library/TrackTable.svelte';
	import TrackTableSkeleton from '$lib/components/shared/TrackTableSkeleton.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { useSearch } from '$lib/hooks/useSearch.svelte';
	import { Search, X, Sparkles, Loader2 } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import type { TrackPage } from '$lib/types';

	let page = $state<TrackPage | null>(null);
	let loading = $state(true);
	let currentPage = $state(0);
	let cleaningDuplicates = $state(false);
	let sortBy = $state('random');
	let sortDir = $state<'asc' | 'desc'>('asc');
	const pageSize = 50;

	const search = useSearch(async () => {
		currentPage = 0;
		await load();
	});

	async function load() {
		loading = true;
		try {
			page = await getTracks(currentPage * pageSize, pageSize, sortBy, sortDir, search.query || undefined);
		} catch (e) {
			console.error('Failed to load tracks:', e);
		} finally {
			loading = false;
		}
	}

	async function handleCleanupDuplicates() {
		cleaningDuplicates = true;
		try {
			const result = await cleanupDuplicateTracks();
			toast.success('Duplicates cleaned up', {
				description: `Merged ${result.merged_track_groups} track groups, removed ${result.deleted_duplicate_tracks} duplicate tracks.`
			});
			load();
		} catch (e) {
			toast.error('Failed to cleanup duplicates', { description: String(e) });
		} finally {
			cleaningDuplicates = false;
		}
	}

	function handleSort(column: string) {
		if (sortBy === column) {
			sortDir = sortDir === 'asc' ? 'desc' : 'asc';
		} else {
			sortBy = column;
			sortDir = 'asc';
		}
		currentPage = 0;
		load();
	}

	$effect(() => {
		libraryStore.version;
		// untrack: load() reads currentPage/sortBy/search.query — tracking them
		// here made every pagination/sort/keystroke fetch TWICE (the explicit
		// load() plus this effect re-running), visibly reshuffling the default
		// random sort and defeating the search debounce.
		untrack(() => load());
	});

	const totalPages = $derived(page ? Math.ceil(page.total / pageSize) : 0);
</script>

<div class="flex flex-col flex-1 min-h-0 gap-4">
	<div class="flex items-center justify-between shrink-0">
		<div>
			<h1 class="text-3xl font-bold tracking-tight">Songs</h1>
			<p class="text-muted-foreground mt-1">
				{#if page}
					{page.total} track{page.total !== 1 ? 's' : ''}{search.query ? ` matching "${search.query}"` : ' in your library'}
				{:else}
					Loading...
				{/if}
			</p>
		</div>
		<div class="flex items-center gap-2">
			<Button variant="outline" size="sm" onclick={handleCleanupDuplicates} disabled={cleaningDuplicates}>
				{#if cleaningDuplicates}
					<Loader2 class="size-4 animate-spin" />
					Cleaning...
				{:else}
					<Sparkles class="size-4" />
					Cleanup Duplicates
				{/if}
			</Button>
			<div class="relative w-64">
			<Search class="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
			<Input
				placeholder="Search songs..."
				class="pl-9 pr-8"
				value={search.query}
				oninput={search.handleSearch}
			/>
			{#if search.query}
				<button class="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground" onclick={search.clearSearch}>
					<X class="size-4" />
				</button>
			{/if}
			</div>
		</div>
	</div>

	{#if loading && !page}
		<TrackTableSkeleton />
	{:else if page}
		<TrackTable tracks={page.tracks} {sortBy} {sortDir} onsort={handleSort} />

		{#if totalPages > 1}
			<div class="flex items-center justify-center gap-2 shrink-0 pb-2">
				<Button
					variant="outline"
					size="sm"
					disabled={currentPage === 0}
					onclick={() => { currentPage--; load(); }}
				>
					Previous
				</Button>
				<span class="text-sm text-muted-foreground">
					Page {currentPage + 1} of {totalPages}
				</span>
				<Button
					variant="outline"
					size="sm"
					disabled={currentPage >= totalPages - 1}
					onclick={() => { currentPage++; load(); }}
				>
					Next
				</Button>
			</div>
		{/if}
	{/if}
</div>
