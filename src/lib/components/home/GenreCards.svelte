<script lang="ts">
	import { getGenres, getTracksByGenre } from '$lib/api/library';
	import { Tags } from 'lucide-svelte';
	import DiscoveryCard from './DiscoveryCard.svelte';
	import type { Track } from '$lib/types';

	const MIN_TRACKS = 3;
	const MAX_GENRES = 4;
	const TRACKS_PER_GENRE = 10;

	let genres = $state<string[]>([]);
	let genreTracks = $state<Record<string, Track[]>>({});
	let loading = $state(true);

	async function load() {
		loading = true;
		try {
			const allGenres = await getGenres();
			// Load tracks for all genres, then filter to those with enough content
			const entries = await Promise.all(
				allGenres.map(async (genre) => {
					const tracks = await getTracksByGenre(genre, TRACKS_PER_GENRE);
					return [genre, tracks] as const;
				})
			);
			// Only keep genres with MIN_TRACKS+ tracks, cap at MAX_GENRES
			const filtered = entries.filter(([, tracks]) => tracks.length >= MIN_TRACKS);
			genres = filtered.slice(0, MAX_GENRES).map(([g]) => g);
			genreTracks = Object.fromEntries(filtered.slice(0, MAX_GENRES));
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
		{#if genreTracks[genre]?.length >= MIN_TRACKS}
			<DiscoveryCard
				title={genre}
				icon={Tags}
				tracks={genreTracks[genre]}
				onRefresh={() => refreshGenre(genre)}
			/>
		{/if}
	{/each}
{/if}
