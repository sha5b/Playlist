<script lang="ts">
	import { page } from '$app/state';
	import { getArtist, getArtistTracks, getArtistAlbums, enrichArtist, getArtistMissingAlbums, downloadArtistMissing, getAlbumsDownloadStatus } from '$lib/api/library';
	import type { AlbumDownloadStatus, ArtistDiscographyEntry } from '$lib/api/library';
	import TrackTable from '$lib/components/library/TrackTable.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { player } from '$lib/stores/player.svelte';
	import { ArrowLeft, Users, Disc, Play, Shuffle, Loader2, Sparkles, Download, Check } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import { assetUrl } from '$lib/utils/format';
	import { listen } from '@tauri-apps/api/event';
	import { onMount } from 'svelte';
	import type { Artist, Album, Track } from '$lib/types';

	let artist = $state<Artist | null>(null);
	let tracks: Track[] = $state([]);
	let albums: Album[] = $state([]);
	let loading = $state(true);
	let enriching = $state(false);
	let missingAlbums: ArtistDiscographyEntry[] = $state([]);
	let totalDiscography = $state(0);
	let downloadingMissing = $state(false);
	let downloadingAlbumMbids = $state<Set<string>>(new Set());
	let downloadStatuses: Record<string, AlbumDownloadStatus> = $state({});

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

			// Load download statuses for local albums
			if (al.length > 0) {
				const ids = al.map((a) => a.id);
				downloadStatuses = await getAlbumsDownloadStatus(ids);
			}

			// Auto-enrich if artist has no enriched discography
			if (a && !a.enriched_discography) {
				autoEnrich(a);
			} else if (a) {
				// Load missing albums from already enriched data
				await loadMissing(a.id);
			}
		} catch (e) {
			toast.error('Failed to load artist');
		} finally {
			loading = false;
		}
	}

	async function autoEnrich(a: Artist) {
		enriching = true;
		try {
			await enrichArtist(a.id);
			artist = await getArtist(a.id);
			await loadMissing(a.id);
		} catch {
			// Silent fail for auto-enrich
		} finally {
			enriching = false;
		}
	}

	async function loadMissing(id: number) {
		try {
			const result = await getArtistMissingAlbums(id);
			missingAlbums = result.missing;
			totalDiscography = result.total_discography;
		} catch {
			// ignore
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
		for (let i = ids.length - 1; i > 0; i--) {
			const j = Math.floor(Math.random() * (i + 1));
			[ids[i], ids[j]] = [ids[j], ids[i]];
		}
		player.playTracks(ids, 0);
	}

	async function handleEnrich() {
		if (!artist || enriching) return;
		enriching = true;
		try {
			await enrichArtist(artist.id);
			artist = await getArtist(artist.id);
			if (artist) await loadMissing(artist.id);
			toast.success('Artist discography refreshed');
		} catch (e) {
			toast.error('Enrichment failed', { description: String(e) });
		} finally {
			enriching = false;
		}
	}

	async function handleDownloadAlbum(entry: ArtistDiscographyEntry) {
		if (!artist) return;
		downloadingAlbumMbids = new Set([...downloadingAlbumMbids, entry.mbid]);
		try {
			const downloads = await downloadArtistMissing(artist.id, [entry.mbid]);
			toast.success(`Queued ${downloads.length} tracks from "${entry.title}"`);
			// Remove from missing list
			missingAlbums = missingAlbums.filter((a) => a.mbid !== entry.mbid);
		} catch (e) {
			toast.error(`Failed to download "${entry.title}"`, { description: String(e) });
		} finally {
			const next = new Set(downloadingAlbumMbids);
			next.delete(entry.mbid);
			downloadingAlbumMbids = next;
		}
	}

	async function handleDownloadAllMissing() {
		if (!artist || missingAlbums.length === 0 || downloadingMissing) return;
		downloadingMissing = true;
		try {
			const mbids = missingAlbums.map((a) => a.mbid);
			const downloads = await downloadArtistMissing(artist.id, mbids);
			toast.success(`Queued ${downloads.length} tracks from ${mbids.length} albums`);
			missingAlbums = [];
		} catch (e) {
			toast.error('Failed to queue downloads', { description: String(e) });
		} finally {
			downloadingMissing = false;
		}
	}

	// Auto-refresh when downloads complete
	onMount(() => {
		let cleanup: (() => void) | undefined;
		listen('library-updated', () => {
			if (artist) {
				getArtistTracks(artist.id).then((t) => { tracks = t; });
				getArtistAlbums(artist.id).then((al) => {
					albums = al;
					if (al.length > 0) {
						getAlbumsDownloadStatus(al.map((a) => a.id)).then((s) => { downloadStatuses = s; });
					}
				});
			}
		}).then((unlisten) => { cleanup = unlisten; });
		return () => cleanup?.();
	});
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
					{#if totalDiscography > 0}&middot; {totalDiscography} in discography{/if}
				</p>
				{#if artist.artist_type || artist.country || artist.begin_year}
					<p class="text-xs text-muted-foreground">
						{#if artist.artist_type}{artist.artist_type}{/if}
						{#if artist.country}{artist.artist_type ? ' · ' : ''}{artist.country}{/if}
						{#if artist.begin_year}{(artist.artist_type || artist.country) ? ' · Since ' : 'Since '}{artist.begin_year}{/if}
					</p>
				{/if}
				<div class="flex gap-2 pt-2">
					<Button onclick={playAll} disabled={tracks.length === 0}>
						<Play class="size-4" fill="currentColor" />
						Play All
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
					{#if missingAlbums.length > 0}
						<Button variant="outline" onclick={handleDownloadAllMissing} disabled={downloadingMissing}>
							{#if downloadingMissing}
								<Loader2 class="size-4 animate-spin" />
							{:else}
								<Download class="size-4" />
							{/if}
							Download {missingAlbums.length} Missing Albums
						</Button>
					{/if}
				</div>
			</div>
		</div>

		{#if albums.length > 0}
			<div class="space-y-3">
				<h2 class="text-lg font-semibold">Albums</h2>
				<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
					{#each albums as album}
						<a href="/library/albums/{album.id}" class="group rounded-lg p-2 -m-2 transition-colors hover:bg-muted/30">
							<div class="relative aspect-square rounded-lg bg-muted flex items-center justify-center overflow-hidden mb-2">
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
								{#if downloadStatuses[String(album.id)]?.status === 'complete'}
									<div class="absolute top-1.5 right-1.5 flex items-center justify-center size-5 rounded-full bg-green-500 text-white">
										<Check class="size-3" />
									</div>
								{:else if downloadStatuses[String(album.id)]?.status === 'partial'}
									<div class="absolute top-1.5 right-1.5 rounded-full bg-yellow-500/90 text-white text-[10px] font-bold px-1.5 py-0.5 leading-none">
										{downloadStatuses[String(album.id)].total_local}/{downloadStatuses[String(album.id)].total_expected}
									</div>
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

		{#if missingAlbums.length > 0}
			<div class="space-y-3">
				<div class="flex items-center justify-between">
					<h2 class="text-lg font-semibold">Missing Albums</h2>
					<span class="text-sm text-muted-foreground">{missingAlbums.length} not in library</span>
				</div>
				<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
					{#each missingAlbums as entry (entry.mbid)}
						<div class="group rounded-lg p-2 -m-2 opacity-60 hover:opacity-100 transition-opacity">
							<div class="relative aspect-square rounded-lg bg-muted/50 flex items-center justify-center overflow-hidden mb-2 border border-dashed border-border">
								<Disc class="size-12 text-muted-foreground/50" />
								<div class="absolute inset-0 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity bg-black/30 rounded-lg">
									<Button
										size="sm"
										onclick={() => handleDownloadAlbum(entry)}
										disabled={downloadingAlbumMbids.has(entry.mbid)}
									>
										{#if downloadingAlbumMbids.has(entry.mbid)}
											<Loader2 class="size-4 animate-spin" />
										{:else}
											<Download class="size-4" />
										{/if}
										Download
									</Button>
								</div>
							</div>
							<p class="text-sm font-medium truncate">{entry.title}</p>
							<p class="text-xs text-muted-foreground truncate">
								{#if entry.album_type}
									<Badge variant="outline" class="text-[10px] px-1 py-0 mr-1">{entry.album_type}</Badge>
								{/if}
								{entry.year ?? 'Unknown year'}
							</p>
						</div>
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
