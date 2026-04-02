<script lang="ts">
	import { getLibraryStats, getTracks } from '$lib/api/library';
	import { getMonitoredPlaylists } from '$lib/api/manager';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { Clock, TrendingUp, Sparkles, Shuffle } from 'lucide-svelte';
	import type { LibraryStats, Track } from '$lib/types';

	import WelcomeScreen from '$lib/components/home/WelcomeScreen.svelte';
	import StatsBar from '$lib/components/home/StatsBar.svelte';
	import DiscoveryCard from '$lib/components/home/DiscoveryCard.svelte';
	import GenreCards from '$lib/components/home/GenreCards.svelte';

	let stats = $state<LibraryStats | null>(null);
	let hasPlaylists = $state(false);

	let recentlyAdded = $state<Track[]>([]);
	let mostListened = $state<Track[]>([]);
	let forgotten = $state<Track[]>([]);
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
			const [recent, top, least, random] = await Promise.all([
				getTracks(0, 10, 'date_added', 'desc'),
				getTracks(0, 10, 'play_count', 'desc'),
				getTracks(0, 10, 'play_count', 'asc'),
				getTracks(0, 10, 'random', 'desc'),
			]);
			recentlyAdded = recent.tracks;
			mostListened = top.tracks;
			forgotten = least.tracks;
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

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
	{#if isEmpty}
		<WelcomeScreen onChanged={() => { loadStats(); loadPlaylists(); }} />
	{:else}
		<div>
			<h1 class="text-3xl font-bold tracking-tight">Home</h1>
			<p class="text-muted-foreground mt-1">Your library, your way</p>
		</div>

		<StatsBar {stats} />

		<DiscoveryCard
			title="Recently Added"
			icon={Clock}
			tracks={recentlyAdded}
			loading={discoveryLoading}
		/>

		<DiscoveryCard
			title="Most Listened"
			icon={TrendingUp}
			tracks={mostListened}
			loading={discoveryLoading}
		/>

		<DiscoveryCard
			title="Forgotten Gems"
			icon={Sparkles}
			tracks={forgotten}
			loading={discoveryLoading}
		/>

		<DiscoveryCard
			title="Random Mix"
			icon={Shuffle}
			tracks={randomMix}
			loading={discoveryLoading}
			onRefresh={refreshRandom}
		/>

		<GenreCards />
	{/if}
</div>
