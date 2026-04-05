<script lang="ts">
	import { page } from '$app/state';
	import { getAlbum, getAlbumTracks, enrichAlbum, deleteAlbumTracks, deleteTrack } from '$lib/api/library';
	import { searchAndDownload, searchAndDownloadBatch } from '$lib/api/downloads';
	import type { SearchDownloadRequest } from '$lib/api/downloads';
	import TrackTable from '$lib/components/library/TrackTable.svelte';
	import { Button } from '$lib/components/ui/button';
	import { player } from '$lib/stores/player.svelte';
	import { formatDurationLong, assetUrl, shuffleArray } from '$lib/utils/format';
	import { ArrowLeft, Disc, Play, Shuffle, Loader2, Sparkles, Download, Trash2 } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import { listen } from '@tauri-apps/api/event';
	import { onMount } from 'svelte';
	import type { Album, Track, AlbumTrackInfo } from '$lib/types';

	let album = $state<Album | null>(null);
	let tracks: Track[] = $state([]);
	let loading = $state(true);
	let enriching = $state(false);
	let downloadingMissing = $state(false);
	let deletingTracks = $state(false);

	const albumId = $derived(Number(page.params.id));

	// Parse saved enriched tracklist from the album's DB column
	let savedTracklist = $derived.by(() => {
		if (!album?.enriched_tracklist) return [];
		try {
			return JSON.parse(album.enriched_tracklist) as AlbumTrackInfo[];
		} catch { return []; }
	});

	async function load(id: number) {
		loading = true;
		try {
			const [a, t] = await Promise.all([getAlbum(id), getAlbumTracks(id)]);
			album = a;
			tracks = t;
			// Auto-enrich if album hasn't been enriched yet
			if (a && (!a.musicbrainz_id || !a.enriched_tracklist)) {
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
			await enrichAlbum(a.id);
			album = await getAlbum(a.id);
			tracks = await getAlbumTracks(a.id);
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
		const ids = shuffleArray(tracks.map((t) => t.id));
		player.playTracks(ids, 0);
	}

	async function handleEnrich() {
		if (!album || enriching) return;
		await autoEnrich(album);
		toast.success('Album metadata refreshed');
	}

	async function downloadAllMissing() {
		if (!album || missingTracks.length === 0 || downloadingMissing) return;
		downloadingMissing = true;
		try {
			const artistName = album.artist_name ?? album.album_artist ?? 'Unknown Artist';
			const queries: SearchDownloadRequest[] = missingTracks
				.filter((t) => t.title)
				.map((t) => ({
					query: `${artistName} - ${t.title}`,
					title: t.title,
					artist: artistName,
					album_id: album!.id,
					artist_id: album!.artist_id ?? undefined,
				}));
			if (queries.length === 0) {
				toast.error('No track titles available — enrich the album first');
				return;
			}
			await searchAndDownloadBatch(queries);
			toast.success(`Queued ${queries.length} download${queries.length !== 1 ? 's' : ''}`);
		} catch (e) {
			toast.error(`Failed to queue downloads: ${e}`);
		} finally {
			downloadingMissing = false;
		}
	}

	function handleDownloadTrack(placeholder: { track_number: number; disc_number: number; title?: string }) {
		if (!album || !placeholder.title) {
			toast.error('No track title available — enrich the album first');
			return;
		}
		const artistName = album.artist_name ?? album.album_artist ?? 'Unknown Artist';
		const query = `${artistName} - ${placeholder.title}`;
		searchAndDownload(query, placeholder.title, artistName, undefined, undefined, album.id, album.artist_id ?? undefined, placeholder.disc_number, placeholder.track_number)
			.then(() => toast.success(`Queued: ${placeholder.title}`))
			.catch((e) => toast.error(`Failed: ${e}`));
	}

	async function handleDeleteAlbumTracks() {
		if (!album || tracks.length === 0 || deletingTracks) return;
		deletingTracks = true;
		try {
			const count = await deleteAlbumTracks(album.id);
			toast.success(`Deleted ${count} track${count !== 1 ? 's' : ''} — placeholders kept for re-download`);
			await load(album.id);
		} catch (e) {
			toast.error(`Failed to delete tracks: ${e}`);
		} finally {
			deletingTracks = false;
		}
	}

	async function handleDeleteTrack(track: Track) {
		try {
			await deleteTrack(track.id, true);
			toast.success(`Deleted "${track.title}"`);
			if (album) await load(album.id);
		} catch (e) {
			toast.error(`Failed to delete track: ${e}`);
		}
	}

	// Auto-refresh tracks when downloads complete (so placeholders disappear)
	onMount(() => {
		let cleanup: (() => void) | undefined;
		listen('library-updated', () => {
			if (album) load(album.id);
		}).then((unlisten) => { cleanup = unlisten; });
		return () => cleanup?.();
	});

	const totalDuration = $derived(
		tracks.reduce((sum, t) => sum + (t.duration_ms ?? 0), 0)
	);

	const missingTracks = $derived.by(() => {
		const existingPositions = new Set(
			tracks.map((t) => `${t.disc_number ?? 1}:${t.track_number ?? 0}`)
		);
		// Also match by title (normalized) so tracks downloaded before track_number was set are recognized
		const existingTitles = new Set(
			tracks.map((t) => t.title.toLowerCase().trim())
		);
		// Use saved enriched tracklist from DB (persisted across page reloads)
		if (savedTracklist.length > 0) {
			return savedTracklist
				.filter((p) => !existingPositions.has(`${p.disc_number}:${p.track_number}`) &&
					(!p.title || !existingTitles.has(p.title.toLowerCase().trim())))
				.map((p) => ({ track_number: p.track_number, disc_number: p.disc_number, title: p.title }));
		}
		// Fallback: generate from total_tracks count
		if (!album?.total_tracks || album.total_tracks <= tracks.length) return [];
		const totalDiscs = album.total_discs ?? 1;
		const missing: { track_number: number; disc_number: number; title?: string }[] = [];
		for (let d = 1; d <= totalDiscs; d++) {
			for (let n = 1; n <= album.total_tracks; n++) {
				if (!existingPositions.has(`${d}:${n}`)) {
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
					{#if missingTracks.length > 0}
						<Button variant="outline" onclick={downloadAllMissing} disabled={downloadingMissing}>
							{#if downloadingMissing}
								<Loader2 class="size-4 animate-spin" />
							{:else}
								<Download class="size-4" />
							{/if}
							Download {missingTracks.length} Missing
						</Button>
					{/if}
					{#if tracks.length > 0}
						<Button variant="destructive" onclick={handleDeleteAlbumTracks} disabled={deletingTracks}>
							{#if deletingTracks}
								<Loader2 class="size-4 animate-spin" />
							{:else}
								<Trash2 class="size-4" />
							{/if}
							Delete Tracks
						</Button>
					{/if}
				</div>
			</div>
		</div>

		<TrackTable
			{tracks}
			placeholders={missingTracks}
			ondownload={handleDownloadTrack}
			ondelete={handleDeleteTrack}
			deleteLabel="Delete from Library"
			referrer={album ? { type: 'album', id: album.id, label: album.title } : undefined}
		/>
	{:else}
		<p class="text-muted-foreground">Album not found.</p>
	{/if}
</div>
