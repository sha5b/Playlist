<script lang="ts">
	import { getLibraryStats, getTracks, getRecentlyAddedAlbums } from '$lib/api/library';
	import { getMonitoredPlaylists } from '$lib/api/manager';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { TrendingUp, Shuffle, Clock } from 'lucide-svelte';
	import type { LibraryStats, Track, Album } from '$lib/types';

	import WelcomeScreen from '$lib/components/home/WelcomeScreen.svelte';
	import HomeGreeting from '$lib/components/home/HomeGreeting.svelte';
	import QuickAccessGrid from '$lib/components/home/QuickAccessGrid.svelte';
	import AlbumCarousel from '$lib/components/home/AlbumCarousel.svelte';
	import DiscoveryCard from '$lib/components/home/DiscoveryCard.svelte';
	import GenreGrid from '$lib/components/home/GenreGrid.svelte';
	import { scrollRestore } from '$lib/utils/scrollRestore';

	let stats = $state<LibraryStats | null>(null);
	let hasPlaylists = $state(false);

	let recentlyAddedAlbums = $state<Album[]>([]);
	let mostListened = $state<Track[]>([]);
	let randomMix = $state<Track[]>([]);

	let discoveryLoading = $state(true);

	async function loadStats() {
		try {
			stats = await getLibraryStats();
		} catch (e) {
			console.error('Failed to load stats:', e);
		}
	}

	async function loadPlaylists() {
		try {
			const playlists = await getMonitoredPlaylists();
			hasPlaylists = playlists.length > 0;
		} catch (e) {
			console.error('Failed to load playlists:', e);
		}
	}

	async function loadDiscovery() {
		if (!stats || stats.total_tracks === 0) return;
		discoveryLoading = true;
		try {
			const [albums, top, random] = await Promise.all([
				getRecentlyAddedAlbums(10),
				getTracks(0, 10, 'play_count', 'desc'),
				getTracks(0, 10, 'random', 'desc'),
			]);
			recentlyAddedAlbums = albums;
			mostListened = top.tracks;
			randomMix = random.tracks;
		} catch (e) {
			console.error('Failed to load discovery:', e);
		} finally {
			discoveryLoading = false;
		}
	}

	async function refreshRandom() {
		try {
			const result = await getTracks(0, 10, 'random', 'desc');
			randomMix = result.tracks;
		} catch (e) {
			console.error('Failed to refresh random:', e);
		}
	}

	// Hide "On Repeat" when no tracks have been played
	const hasPlayHistory = $derived(mostListened.some((t) => t.play_count > 0));

	$effect(() => {
		libraryStore.version;
		loadStats().then(() => {
			if (stats && stats.total_tracks > 0) {
				loadDiscovery();
			}
		});
		loadPlaylists();
	});

	const isEmpty = $derived(!hasPlaylists && stats !== null && stats.total_tracks === 0);
</script>

<div use:scrollRestore class="flex-1 min-h-0 overflow-y-auto px-1">
	{#if isEmpty}
		<WelcomeScreen onChanged={() => { loadStats(); loadPlaylists(); }} />
	{:else}
		<div class="mx-auto max-w-6xl space-y-7 pb-6">
			<HomeGreeting {stats} />

			<QuickAccessGrid />

			<AlbumCarousel
				title="Recently Added"
				icon={Clock}
				albums={recentlyAddedAlbums}
				loading={discoveryLoading}
				href="/library/albums"
			/>

			{#if hasPlayHistory}
				<DiscoveryCard
					title="On Repeat"
					icon={TrendingUp}
					tracks={mostListened}
					loading={discoveryLoading}
					href="/library/songs"
				/>
			{/if}

			<DiscoveryCard
				title="Rediscover"
				icon={Shuffle}
				tracks={randomMix}
				loading={discoveryLoading}
				onRefresh={refreshRandom}
				href="/library/songs"
			/>

			<GenreGrid />
		</div>
	{/if}
</div>
