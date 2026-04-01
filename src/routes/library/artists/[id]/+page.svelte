<script lang="ts">
	import { page } from '$app/state';
	import { getArtist, getArtistTracks, getArtistAlbums } from '$lib/api/library';
	import TrackTable from '$lib/components/library/TrackTable.svelte';
	import { Button } from '$lib/components/ui/button';
	import { player } from '$lib/stores/player.svelte';
	import { ArrowLeft, Users, Disc, Play, Shuffle, Loader2 } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import { assetUrl } from '$lib/utils/format';
	import type { Artist, Album, Track } from '$lib/types';

	let artist: Artist | null = $state(null);
	let tracks: Track[] = $state([]);
	let albums: Album[] = $state([]);
	let loading = $state(true);

	const artistId = $derived(Number(page.params.id));

	async function load(id: number) {
		loading = true;
		try {
			const [a, t, al] = await Promise.all([
				getArtist(id),
				getArtistTracks(id),
				getArtistAlbums(id),
			]);
			artist = a;
			tracks = t;
			albums = al;
		} catch (e) {
			toast.error('Failed to load artist');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load(artistId);
	});

	function playAll() {
		if (tracks.length === 0) return;
		player.playTracks(tracks.map((t) => t.id), 0);
	}

	function shuffleAll() {
		if (tracks.length === 0) return;
		const ids = tracks.map((t) => t.id);
		const randomStart = Math.floor(Math.random() * ids.length);
		player.playTracks(ids, randomStart);
	}
</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
	<a
		href="/library/artists"
		class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors"
	>
		<ArrowLeft class="size-4" />
		Artists
	</a>

	{#if loading}
		<div class="flex items-center justify-center h-48">
			<Loader2 class="size-6 animate-spin text-muted-foreground" />
		</div>
	{:else if artist}
		<div class="flex gap-6 items-end">
			<div class="size-48 rounded-full bg-muted flex items-center justify-center overflow-hidden shrink-0 shadow-lg">
				{#if artist.image_path}
					<img
						src={assetUrl(artist.image_path)}
						alt={artist.name}
						class="size-full object-cover"
					/>
				{:else}
					<Users class="size-16 text-muted-foreground" />
				{/if}
			</div>
			<div class="space-y-2">
				<p class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Artist</p>
				<h1 class="text-3xl font-bold tracking-tight">{artist.name}</h1>
				<p class="text-sm text-muted-foreground">
					{artist.track_count} track{artist.track_count !== 1 ? 's' : ''}
					{#if albums.length > 0}&middot; {albums.length} album{albums.length !== 1 ? 's' : ''}{/if}
				</p>
				<div class="flex gap-2 pt-2">
					<Button onclick={playAll} disabled={tracks.length === 0}>
						<Play class="size-4" fill="currentColor" />
						Play All
					</Button>
					<Button variant="outline" onclick={shuffleAll} disabled={tracks.length === 0}>
						<Shuffle class="size-4" />
						Shuffle
					</Button>
				</div>
			</div>
		</div>

		{#if albums.length > 0}
			<div class="space-y-3">
				<h2 class="text-lg font-semibold">Albums</h2>
				<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
					{#each albums as album}
						<a href="/library/albums/{album.id}" class="group">
							<div class="aspect-square rounded-lg bg-muted flex items-center justify-center overflow-hidden mb-2">
								{#if album.cover_art_path}
									<img
										src={assetUrl(album.cover_art_path)}
										alt={album.title}
										class="size-full object-cover group-hover:scale-105 transition-transform"
										loading="lazy"
									/>
								{:else}
									<Disc class="size-12 text-muted-foreground" />
								{/if}
							</div>
							<p class="text-sm font-medium truncate">{album.title}</p>
							<p class="text-xs text-muted-foreground truncate">
								{album.year ?? ''}
								{#if album.track_count}&middot; {album.track_count} tracks{/if}
							</p>
						</a>
					{/each}
				</div>
			</div>
		{/if}

		<div class="space-y-3">
			<h2 class="text-lg font-semibold">All Tracks</h2>
			<TrackTable {tracks} />
		</div>
	{:else}
		<p class="text-muted-foreground">Artist not found.</p>
	{/if}
</div>
