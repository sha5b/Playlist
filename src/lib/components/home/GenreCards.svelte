<script lang="ts">
	import { getGenres, getTracksByGenre } from '$lib/api/library';
	import { Tags } from 'lucide-svelte';
	import DiscoveryCard from './DiscoveryCard.svelte';
	import type { Track } from '$lib/types';

	const MAX_GENRES = 6;
	const TRACKS_PER_GENRE = 10;

	let genres = $state<string[]>([]);
	let genreTracks = $state<Record<string, Track[]>>({});
	let loading = $state(true);

	async function load() {
		loading = true;
		try {
			const allGenres = await getGenres();
			genres = allGenres.slice(0, MAX_GENRES);
			const entries = await Promise.all(
				genres.map(async (genre) => {
					const tracks = await getTracksByGenre(genre, TRACKS_PER_GENRE);
					return [genre, tracks] as const;
				})
			);
			genreTracks = Object.fromEntries(entries);
		} catch (e) {
			console.error('Failed to load genres:', e);
		} finally {
			loading = false;
		}
	}

	async function refreshGenre(genre: string) {
		try {
			genreTracks[genre] = await getTracksByGenre(genre, TRACKS_PER_GENRE);
		} catch (e) {
			console.error(`Failed to refresh genre ${genre}:`, e);
		}
	}

	$effect(() => {
		load();
	});
</script>

{#if !loading && genres.length > 0}
	{#each genres as genre}
		{#if genreTracks[genre]?.length > 0}
			<DiscoveryCard
				title={genre}
				icon={Tags}
				tracks={genreTracks[genre]}
				onRefresh={() => refreshGenre(genre)}
			/>
		{/if}
	{/each}
{/if}
