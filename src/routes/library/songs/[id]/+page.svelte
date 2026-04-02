<script lang="ts">
	import { page } from '$app/state';
	import { getTrack, enrichTrack, getAlbum, getArtist } from '$lib/api/library';
	import { Button } from '$lib/components/ui/button';
	import { player } from '$lib/stores/player.svelte';
	import { mvDownloadStore } from '$lib/stores/mvDownloads.svelte';
	import { formatDuration, formatFileSize, formatDate, assetUrl, platformLabel } from '$lib/utils/format';
	import { ArrowLeft, Music, Play, ListStart, ListPlus, Loader2, Sparkles, ExternalLink, User, Disc3, Download } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import type { Track, Album, Artist } from '$lib/types';

	let track = $state<Track | null>(null);
	let album = $state<Album | null>(null);
	let artist = $state<Artist | null>(null);
	let loading = $state(true);
	let enriching = $state(false);
	let lyricsOpen = $state(true);

	const downloadingMV = $derived(track ? mvDownloadStore.isDownloading(track.id) : false);
	const mvProgress = $derived(track ? mvDownloadStore.getProgress(track.id) : 0);

	mvDownloadStore.init();

	const trackId = $derived(Number(page.params.id));

	const parsedTags = $derived.by(() => {
		if (!track?.tags) return [];
		try { return JSON.parse(track.tags) as string[]; } catch { return []; }
	});

	const plainLyrics = $derived.by(() => {
		if (!track?.lyrics) return '';
		// Strip LRC timestamps [mm:ss.xx] for plain display
		return track.lyrics.replace(/\[\d{2}:\d{2}(?:\.\d{2,3})?\]/g, '').trim();
	});

	async function load(id: number) {
		loading = true;
		album = null;
		artist = null;
		try {
			const t = await getTrack(id);
			track = t;
			if (t) {
				// Load album and artist in parallel
				const [albumResult, artistResult] = await Promise.all([
					t.album_id ? getAlbum(t.album_id).catch(() => null) : null,
					t.artist_id ? getArtist(t.artist_id).catch(() => null) : null,
				]);
				album = albumResult;
				artist = artistResult;
			}
		} catch (e) {
			console.error('Failed to load track:', e);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load(trackId);
	});

	// Reload track when MV download finishes so the video appears
	let wasMvDownloading = false;
	$effect(() => {
		const downloading = downloadingMV;
		if (wasMvDownloading && !downloading && track) {
			getTrack(track.id).then((t) => { track = t; });
		}
		wasMvDownloading = downloading;
	});

	function playTrack() {
		if (track) player.playTrack(track.id);
	}

	async function playNext() {
		if (!track) return;
		await player.addNext(track.id);
		toast.success(`"${track.title}" will play next`);
	}

	async function addToQueue() {
		if (!track) return;
		await player.addToQueue(track.id);
		toast.success(`Added "${track.title}" to queue`);
	}

	async function handleEnrich() {
		if (!track || enriching) return;
		enriching = true;
		try {
			const result = await enrichTrack(track.id);
			toast.success(`Enriched metadata (${result.fields_updated} fields updated, ${result.completeness}% complete)`);
			track = await getTrack(track.id);
		} catch (e) {
			toast.error('Failed to enrich metadata', { description: String(e) });
		} finally {
			enriching = false;
		}
	}

	function youtubeVideoId(url: string): string | null {
		const m = url.match(/(?:youtube\.com\/watch\?v=|youtu\.be\/|youtube\.com\/embed\/)([\w-]+)/);
		return m ? m[1] : null;
	}

	function youtubeThumbnail(url: string): string | null {
		const id = youtubeVideoId(url);
		return id ? `https://img.youtube.com/vi/${id}/maxresdefault.jpg` : null;
	}

	function handleDownloadMV() {
		if (!track || downloadingMV) return;
		mvDownloadStore.start(track.id);
	}

	function formatBitrate(br: number | null): string {
		if (!br) return '--';
		return `${br} kbps`;
	}

	function formatSampleRate(sr: number | null): string {
		if (!sr) return '--';
		return `${(sr / 1000).toFixed(1)} kHz`;
	}


</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
	<a
		href="/library/songs"
		class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors"
	>
		<ArrowLeft class="size-4" />
		Songs
	</a>

	{#if loading}
		<div class="flex items-center justify-center h-48">
			<Loader2 class="size-6 animate-spin text-muted-foreground" />
		</div>
	{:else if track}
		<div class="flex gap-6 items-end">
			<div class="size-48 rounded-lg bg-muted flex items-center justify-center overflow-hidden shrink-0 shadow-lg">
				{#if track.cover_art_path}
					<img
						src={assetUrl(track.cover_art_path)}
						alt={track.title}
						class="size-full object-cover"
					/>
				{:else}
					<Music class="size-16 text-muted-foreground" />
				{/if}
			</div>
			<div class="space-y-2">
				<div class="flex items-center gap-2">
					<p class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Song</p>
					<span class="text-xs font-medium {track.metadata_completeness >= 80 ? 'text-green-500' : track.metadata_completeness >= 50 ? 'text-yellow-500' : 'text-red-500'}">
						{track.metadata_completeness}% metadata
					</span>
				</div>
				<h1 class="text-3xl font-bold tracking-tight">{track.title}</h1>
				<p class="text-sm text-muted-foreground">
					{#if track.artist_id}
						<a href="/library/artists/{track.artist_id}" class="hover:text-foreground hover:underline transition-colors">
							{track.artist_name ?? 'Unknown Artist'}
						</a>
					{:else}
						{track.artist_name ?? 'Unknown Artist'}
					{/if}
					{#if track.album_id}
						&middot;
						<a href="/library/albums/{track.album_id}" class="hover:text-foreground hover:underline transition-colors">
							{track.album_title ?? 'Unknown Album'}
						</a>
					{/if}
					{#if track.year}&middot; {track.year}{/if}
					&middot; {formatDuration(track.duration_ms)}
				</p>
				<div class="flex gap-2 pt-2">
					<Button onclick={playTrack}>
						<Play class="size-4" fill="currentColor" />
						Play
					</Button>
					<Button variant="outline" onclick={playNext}>
						<ListStart class="size-4" />
						Play Next
					</Button>
					<Button variant="outline" onclick={addToQueue}>
						<ListPlus class="size-4" />
						Add to Queue
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

		{#if track.description}
			<div class="rounded-lg border border-border p-6">
				<h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground mb-3">Description</h2>
				<p class="text-sm text-foreground/80 whitespace-pre-line">{track.description}</p>
			</div>
		{/if}

		{#if parsedTags.length > 0}
			<div class="rounded-lg border border-border p-5">
				<h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground mb-3">Tags</h2>
				<div class="flex flex-wrap gap-2">
					{#each parsedTags as tag}
						<span class="inline-flex items-center rounded-full bg-secondary px-3 py-1 text-xs font-medium text-secondary-foreground">
							{tag}
						</span>
					{/each}
				</div>
			</div>
		{/if}

		{@const hasCards = !!(album || artist || track.source_url)}
		{#if track.music_video_url || hasCards}
			<div class="flex gap-4">
				{#if track.music_video_url}
					<div class="flex-1 min-w-0 rounded-lg overflow-hidden self-start">
						{#if track.music_video_path}
							<!-- svelte-ignore a11y_media_has_caption -->
							<video
								src={assetUrl(track.music_video_path)}
								controls
								class="w-full block bg-black"
							></video>
						{:else}
							{@const thumb = youtubeThumbnail(track.music_video_url)}
							<button
								class="relative w-full aspect-video bg-black group cursor-pointer block p-0 m-0 border-0"
								onclick={handleDownloadMV}
								disabled={downloadingMV}
							>
								{#if thumb}
									<img src={thumb} alt="" class="absolute inset-0 w-full h-full object-cover opacity-40 group-hover:opacity-60 transition-opacity" />
								{/if}
								<div class="absolute inset-0 flex flex-col items-center justify-center gap-2">
									{#if downloadingMV}
										<Loader2 class="size-8 text-white animate-spin" />
										<span class="text-xs text-white/80 font-medium">Downloading{mvProgress > 0 ? ` ${mvProgress}%` : '...'}</span>
									{:else}
										<div class="size-12 rounded-full bg-white/10 backdrop-blur-sm flex items-center justify-center group-hover:bg-white/20 transition-colors">
											<Download class="size-6 text-white" />
										</div>
										<span class="text-xs text-white/70 font-medium">Download to watch</span>
									{/if}
								</div>
							</button>
						{/if}
					</div>
				{/if}
				{#if hasCards}
					<div class="flex-1 min-w-0 flex flex-col gap-3">
						{#if album}
							<div class="rounded-lg border border-border p-4 flex gap-4 items-center hover:bg-muted/50 transition-colors">
								<a href="/library/albums/{album.id}" class="size-12 rounded-md bg-muted overflow-hidden shrink-0">
									{#if album.cover_art_path}
										<img src={assetUrl(album.cover_art_path)} alt={album.title} class="size-full object-cover" />
									{:else}
										<div class="size-full flex items-center justify-center"><Disc3 class="size-5 text-muted-foreground" /></div>
									{/if}
								</a>
								<div class="min-w-0 flex-1">
									<p class="text-xs text-muted-foreground uppercase">{album.album_type ?? 'Album'}</p>
									<a href="/library/albums/{album.id}" class="text-sm font-medium truncate block hover:underline">{album.title}</a>
									{#if album.year}<p class="text-xs text-muted-foreground">{album.year}</p>{/if}
									{#if album.purchase_url}
										<a href={album.purchase_url} target="_blank" rel="noopener noreferrer" class="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground mt-1">
											<ExternalLink class="size-3" />
											Buy Album
										</a>
									{/if}
								</div>
							</div>
						{/if}
						{#if artist}
							<div class="rounded-lg border border-border p-4 flex gap-4 items-center hover:bg-muted/50 transition-colors">
								<a href="/library/artists/{artist.id}" class="size-12 rounded-md bg-muted overflow-hidden shrink-0">
									{#if artist.image_path}
										<img src={assetUrl(artist.image_path)} alt={artist.name} class="size-full object-cover" />
									{:else}
										<div class="size-full flex items-center justify-center"><User class="size-5 text-muted-foreground" /></div>
									{/if}
								</a>
								<div class="min-w-0 flex-1">
									<p class="text-xs text-muted-foreground uppercase">Artist</p>
									<a href="/library/artists/{artist.id}" class="text-sm font-medium truncate block hover:underline">{artist.name}</a>
									{#if artist.website_url}
										<a href={artist.website_url} target="_blank" rel="noopener noreferrer" class="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground mt-1">
											<ExternalLink class="size-3" />
											Website
										</a>
									{/if}
								</div>
							</div>
						{/if}
						{#if track.source_url}
							<a href={track.source_url} target="_blank" rel="noopener noreferrer" class="rounded-lg border border-border p-4 flex gap-4 items-center hover:bg-muted/50 transition-colors">
								<div class="size-12 rounded-md bg-muted overflow-hidden shrink-0 flex items-center justify-center">
									<ExternalLink class="size-5 text-muted-foreground" />
								</div>
								<div class="min-w-0">
									<p class="text-xs text-muted-foreground uppercase">{track.source_platform ? platformLabel(track.source_platform) : 'Source'}</p>
									<p class="text-sm font-medium truncate">{track.source_url}</p>
								</div>
							</a>
						{/if}
					</div>
				{/if}
			</div>
		{/if}

		<div class="rounded-lg border border-border p-6">
			<h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground mb-4">Details</h2>
			<div class="grid grid-cols-2 gap-x-8 gap-y-3 text-sm">
				{#if track.track_number}
					<div>
						<span class="text-muted-foreground">Track Number</span>
						<p class="font-medium">{track.track_number}{#if track.disc_number && track.disc_number > 1} (Disc {track.disc_number}){/if}</p>
					</div>
				{/if}
				{#if track.genre}
					<div>
						<span class="text-muted-foreground">Genre</span>
						<p class="font-medium">{track.genre}</p>
					</div>
				{/if}
				{#if track.year}
					<div>
						<span class="text-muted-foreground">Year</span>
						<p class="font-medium">{track.year}</p>
					</div>
				{/if}
				{#if track.release_date}
					<div>
						<span class="text-muted-foreground">Release Date</span>
						<p class="font-medium">{track.release_date}</p>
					</div>
				{/if}
				{#if track.label}
					<div>
						<span class="text-muted-foreground">Label</span>
						<p class="font-medium">{track.label}</p>
					</div>
				{/if}
				{#if track.composer}
					<div>
						<span class="text-muted-foreground">Composer</span>
						<p class="font-medium">{track.composer}</p>
					</div>
				{/if}
				{#if track.language}
					<div>
						<span class="text-muted-foreground">Language</span>
						<p class="font-medium">{track.language}</p>
					</div>
				{/if}
				{#if track.format}
					<div>
						<span class="text-muted-foreground">Format</span>
						<p class="font-medium">{track.format.toUpperCase()}</p>
					</div>
				{/if}
				{#if track.bitrate}
					<div>
						<span class="text-muted-foreground">Bitrate</span>
						<p class="font-medium">{formatBitrate(track.bitrate)}</p>
					</div>
				{/if}
				{#if track.sample_rate}
					<div>
						<span class="text-muted-foreground">Sample Rate</span>
						<p class="font-medium">{formatSampleRate(track.sample_rate)}</p>
					</div>
				{/if}
				{#if track.channels}
					<div>
						<span class="text-muted-foreground">Channels</span>
						<p class="font-medium">{track.channels === 1 ? 'Mono' : track.channels === 2 ? 'Stereo' : `${track.channels}ch`}</p>
					</div>
				{/if}
				{#if track.file_size}
					<div>
						<span class="text-muted-foreground">File Size</span>
						<p class="font-medium">{formatFileSize(track.file_size)}</p>
					</div>
				{/if}
				<div>
					<span class="text-muted-foreground">Play Count</span>
					<p class="font-medium">{track.play_count}</p>
				</div>
				<div>
					<span class="text-muted-foreground">Date Added</span>
					<p class="font-medium">{formatDate(track.date_added)}</p>
				</div>
			</div>
		</div>
		{#if plainLyrics}
			{@const lyricLines = plainLyrics.split('\n')}
			{@const verses = lyricLines.reduce<string[][]>((acc, line) => {
				if (line.trim() === '') {
					if (acc.length > 0 && acc[acc.length - 1].length > 0) acc.push([]);
				} else {
					if (acc.length === 0) acc.push([]);
					acc[acc.length - 1].push(line);
				}
				return acc;
			}, [])}
			<div class="rounded-xl overflow-hidden relative">
				<!-- Background glow from cover art -->
				<div class="absolute inset-0 opacity-[0.07] blur-3xl pointer-events-none">
					{#if track.cover_art_path}
						<img src={assetUrl(track.cover_art_path)} alt="" class="size-full object-cover" />
					{/if}
				</div>
				<div class="relative border border-border/60 rounded-xl bg-card/60 backdrop-blur-sm">
					<button
						class="w-full px-6 py-4 flex items-center justify-between hover:bg-muted/20 transition-colors"
						onclick={() => lyricsOpen = !lyricsOpen}
					>
						<h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">Lyrics</h2>
						<span class="text-muted-foreground transition-transform duration-200 {lyricsOpen ? 'rotate-180' : ''}"
							>&#9662;</span
						>
					</button>
					{#if lyricsOpen}
						<div class="px-6 pb-8 pt-2 max-h-[36rem] overflow-y-auto">
							<div class="columns-2 gap-12">
								{#each verses as verse}
									{#if verse.length > 0}
										<div class="mb-5 break-inside-avoid">
											{#each verse as line}
												<p class="text-sm leading-7 text-foreground/80">{line}</p>
											{/each}
										</div>
									{/if}
								{/each}
							</div>
						</div>
					{/if}
				</div>
			</div>
		{/if}

	{:else}
		<p class="text-muted-foreground">Track not found.</p>
	{/if}
</div>
