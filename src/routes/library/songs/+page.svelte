<script lang="ts">
	import { getTracks } from '$lib/api/library';
	import TrackTable from '$lib/components/library/TrackTable.svelte';
	import TrackTableSkeleton from '$lib/components/shared/TrackTableSkeleton.svelte';
	import { Button } from '$lib/components/ui/button';
	import type { TrackPage } from '$lib/types';

	let page: TrackPage | null = $state(null);
	let loading = $state(true);
	let currentPage = $state(0);
	const pageSize = 50;

	async function load() {
		loading = true;
		try {
			page = await getTracks(currentPage * pageSize, pageSize, 'date_added', 'desc');
		} catch (e) {
			console.error('Failed to load tracks:', e);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
	});

	const totalPages = $derived(page ? Math.ceil(page.total / pageSize) : 0);
</script>

<div class="space-y-6">
	<div class="flex items-center justify-between">
		<div>
			<h1 class="text-3xl font-bold tracking-tight">Songs</h1>
			<p class="text-muted-foreground mt-1">
				{#if page}
					{page.total} track{page.total !== 1 ? 's' : ''} in your library
				{:else}
					Loading...
				{/if}
			</p>
		</div>
	</div>

	{#if loading && !page}
		<TrackTableSkeleton />
	{:else if page}
		<TrackTable tracks={page.tracks} />

		{#if totalPages > 1}
			<div class="flex items-center justify-center gap-2">
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
