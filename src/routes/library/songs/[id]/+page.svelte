<script lang="ts">
	import { page } from '$app/state';
	import { getTrack, enrichTrack } from '$lib/api/library';
	import { Button } from '$lib/components/ui/button';
	import { player } from '$lib/stores/player.svelte';
	import { formatDuration, formatFileSize, formatDate, assetUrl } from '$lib/utils/format';
	import { ArrowLeft, Music, Play, ListStart, ListPlus, Loader2, Sparkles } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import type { Track } from '$lib/types';

	let track = $state<Track | null>(null);
	let loading = $state(true);
	let enriching = $state(false);

	const trackId = $derived(Number(page.params.id));

	async function load(id: number) {
		loading = true;
		try {
			track = await getTrack(id);
		} catch (e) {
			console.error('Failed to load track:', e);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load(trackId);
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
				{#if track.source_url}
					<div class="col-span-2">
						<span class="text-muted-foreground">Source</span>
						<p class="font-medium truncate">{track.source_url}</p>
					</div>
				{/if}
			</div>
		</div>
	{:else}
		<p class="text-muted-foreground">Track not found.</p>
	{/if}
</div>
