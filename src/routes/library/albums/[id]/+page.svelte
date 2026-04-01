<script lang="ts">
	import { page } from '$app/state';
	import { getAlbum, getAlbumTracks, enrichAlbum } from '$lib/api/library';
	import TrackTable from '$lib/components/library/TrackTable.svelte';
	import { Button } from '$lib/components/ui/button';
	import { player } from '$lib/stores/player.svelte';
	import { formatDurationLong, assetUrl } from '$lib/utils/format';
	import { ArrowLeft, Disc, Play, Shuffle, Loader2, Sparkles } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import type { Album, Track, AlbumTrackInfo } from '$lib/types';

	let album: Album | null = $state(null);
	let tracks: Track[] = $state([]);
	let loading = $state(true);
	let enriching = $state(false);
	let enrichedPlaceholders: { track_number: number; disc_number: number; title?: string }[] = $state([]);

	const albumId = $derived(Number(page.params.id));

	async function load(id: number) {
		loading = true;
		enrichedPlaceholders = [];
		try {
			const [a, t] = await Promise.all([getAlbum(id), getAlbumTracks(id)]);
			album = a;
			tracks = t;
			// Auto-enrich if album hasn't been enriched yet
			if (a && !a.musicbrainz_id) {
				autoEnrich(a);
			}
		} catch (e) {
			toast.error('Failed to load album');
		} finally {
			loading = false;
		}
	}

	async function autoEnrich(a: Album) {
		enriching = true;
		try {
			const result = await enrichAlbum(a.id);
			if (result.tracklist.length > 0) {
				const existingKeys = new Set(
					tracks.map((t) => `${t.disc_number ?? 1}:${t.track_number ?? 0}`)
				);
				enrichedPlaceholders = result.tracklist
					.filter((t) => !existingKeys.has(`${t.disc_number}:${t.track_number}`))
					.map((t) => ({ track_number: t.track_number, disc_number: t.disc_number, title: t.title }));
			}
			album = await getAlbum(a.id);
		} catch {
			// Silent fail for auto-enrich
		} finally {
			enriching = false;
		}
	}

	$effect(() => {
		load(albumId);
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

	async function handleEnrich() {
		if (!album || enriching) return;
		await autoEnrich(album);
		toast.success('Album metadata refreshed');
	}

	const totalDuration = $derived(
		tracks.reduce((sum, t) => sum + (t.duration_ms ?? 0), 0)
	);

	const missingTracks = $derived.by(() => {
		const existing = new Set(
			tracks.map((t) => `${t.disc_number ?? 1}:${t.track_number ?? 0}`)
		);
		// First: use enriched placeholders from MusicBrainz if available
		if (enrichedPlaceholders.length > 0) {
			return enrichedPlaceholders.filter((p) => !existing.has(`${p.disc_number}:${p.track_number}`));
		}
		// Fallback: generate from total_tracks count
		if (!album?.total_tracks || album.total_tracks <= tracks.length) return [];
		const totalDiscs = album.total_discs ?? 1;
		const missing: { track_number: number; disc_number: number }[] = [];
		for (let d = 1; d <= totalDiscs; d++) {
			for (let n = 1; n <= album.total_tracks; n++) {
				if (!existing.has(`${d}:${n}`)) {
					missing.push({ track_number: n, disc_number: d });
				}
			}
		}
		return missing;
	});
</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
	<a
		href="/library/albums"
		class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors"
	>
		<ArrowLeft class="size-4" />
		Albums
	</a>

	{#if loading}
		<div class="flex items-center justify-center h-48">
			<Loader2 class="size-6 animate-spin text-muted-foreground" />
		</div>
	{:else if album}
		<div class="flex gap-6 items-end">
			<div class="size-48 rounded-lg bg-muted flex items-center justify-center overflow-hidden shrink-0 shadow-lg">
				{#if album.cover_art_path}
					<img
						src={assetUrl(album.cover_art_path)}
						alt={album.title}
						class="size-full object-cover"
					/>
				{:else}
					<Disc class="size-16 text-muted-foreground" />
				{/if}
			</div>
			<div class="space-y-2">
				<p class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Album</p>
				<h1 class="text-3xl font-bold tracking-tight">{album.title}</h1>
				<p class="text-sm text-muted-foreground">
					{album.artist_name ?? 'Unknown Artist'}
					{#if album.year}&middot; {album.year}{/if}
					&middot; {tracks.length} track{tracks.length !== 1 ? 's' : ''}
					{#if totalDuration > 0}&middot; {formatDurationLong(totalDuration)}{/if}
				</p>
				{#if album.label || album.album_type || album.release_date}
					<p class="text-xs text-muted-foreground">
						{#if album.album_type}{album.album_type}{/if}
						{#if album.label}{album.album_type ? ' · ' : ''}{album.label}{/if}
						{#if album.release_date}{(album.album_type || album.label) ? ' · ' : ''}{album.release_date}{/if}
					</p>
				{/if}
				<div class="flex gap-2 pt-2">
					<Button onclick={playAll} disabled={tracks.length === 0}>
						<Play class="size-4" fill="currentColor" />
						Play
					</Button>
					<Button variant="outline" onclick={shuffleAll} disabled={tracks.length === 0}>
						<Shuffle class="size-4" />
						Shuffle
					</Button>
					<Button variant="outline" onclick={handleEnrich} disabled={enriching}>
						{#if enriching}
							<Loader2 class="size-4 animate-spin" />
						{:else}
							<Sparkles class="size-4" />
						{/if}
						Enrich
					</Button>
				</div>
			</div>
		</div>

		<TrackTable {tracks} placeholders={missingTracks} />
	{:else}
		<p class="text-muted-foreground">Album not found.</p>
	{/if}
</div>
