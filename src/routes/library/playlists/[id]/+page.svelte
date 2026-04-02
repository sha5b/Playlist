<script lang="ts">
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { getPlaylist, getPlaylistTracks, deletePlaylist, removeFromPlaylist } from '$lib/api/library';
	import TrackTable from '$lib/components/library/TrackTable.svelte';
	import { Button } from '$lib/components/ui/button';
	import { player } from '$lib/stores/player.svelte';
	import { formatDurationLong, assetUrl, shuffleArray, platformLabel, platformColor, timeAgo } from '$lib/utils/format';
	import { Badge } from '$lib/components/ui/badge';
	import { ArrowLeft, ListMusic, Play, Shuffle, Trash2, Loader2, ExternalLink } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import type { Playlist, TrackPage, Track } from '$lib/types';

	let playlist = $state<Playlist | null>(null);
	let trackPage = $state<TrackPage | null>(null);
	let loading = $state(true);
	let currentPage = $state(0);
	const pageSize = 50;

	const playlistId = $derived(Number(page.params.id));
	const totalPages = $derived(trackPage ? Math.ceil(trackPage.total / pageSize) : 0);

	async function load(id: number) {
		loading = true;
		try {
			const detail = await getPlaylist(id);
			if (detail) {
				playlist = detail.playlist;
			}
			trackPage = await getPlaylistTracks(id, currentPage * pageSize, pageSize);
		} catch (e) {
			toast.error('Failed to load playlist');
		} finally {
			loading = false;
		}
	}

	async function loadTracks() {
		if (!playlist) return;
		try {
			trackPage = await getPlaylistTracks(playlist.id, currentPage * pageSize, pageSize);
		} catch (e) {
			console.error('Failed to load tracks:', e);
		}
	}

	$effect(() => {
		currentPage = 0;
		load(playlistId);
	});

	function playAll() {
		if (!trackPage || trackPage.tracks.length === 0) return;
		player.playTracks(trackPage.tracks.map((t) => t.id), 0);
	}

	function shuffleAll() {
		if (!trackPage || trackPage.tracks.length === 0) return;
		const ids = shuffleArray(trackPage.tracks.map((t) => t.id));
		player.playTracks(ids, 0);
	}

	async function handleDelete() {
		if (!playlist) return;
		if (!confirm(`Delete playlist "${playlist.name}"?`)) return;
		try {
			await deletePlaylist(playlist.id);
			toast.success('Playlist deleted');
			goto('/library/playlists');
		} catch (e) {
			toast.error('Failed to delete playlist');
		}
	}

	async function handleRemoveTrack(track: Track) {
		if (!playlist) return;
		try {
			await removeFromPlaylist(playlist.id, track.id);
			toast.success(`Removed "${track.title}"`);
			await loadTracks();
		} catch (e) {
			toast.error('Failed to remove track');
		}
	}
</script>

<div class="flex flex-col flex-1 min-h-0 gap-4">
	<a
		href="/library/playlists"
		class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors shrink-0"
	>
		<ArrowLeft class="size-4" />
		Playlists
	</a>

	{#if loading && !playlist}
		<div class="flex items-center justify-center h-48">
			<Loader2 class="size-6 animate-spin text-muted-foreground" />
		</div>
	{:else if playlist}
		<div class="flex gap-6 items-end shrink-0">
			<div class="size-48 rounded-lg bg-muted flex items-center justify-center overflow-hidden shrink-0 shadow-lg">
				{#if playlist.cover_art_path}
					<img
						src={assetUrl(playlist.cover_art_path)}
						alt={playlist.name}
						class="size-full object-cover"
					/>
				{:else}
					<ListMusic class="size-16 text-muted-foreground" />
				{/if}
			</div>
			<div class="space-y-2">
				<div class="flex items-center gap-2">
					<p class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Playlist</p>
					{#if playlist.source_platform}
						<Badge variant={platformColor(playlist.source_platform)} class="text-xs">
							{platformLabel(playlist.source_platform)}
						</Badge>
					{/if}
					{#if playlist.is_synced}
						<Badge variant="outline" class="text-xs">Synced</Badge>
					{/if}
				</div>
				<h1 class="text-3xl font-bold tracking-tight">{playlist.name}</h1>
				{#if playlist.description}
					<p class="text-sm text-muted-foreground">{playlist.description}</p>
				{/if}
				<p class="text-sm text-muted-foreground">
					{trackPage ? trackPage.total : 0} track{(trackPage?.total ?? 0) !== 1 ? 's' : ''}
					{#if playlist.total_duration_ms > 0}
						&middot; {formatDurationLong(playlist.total_duration_ms)}
					{/if}
					{#if playlist.last_synced_at}
						&middot; Synced {timeAgo(playlist.last_synced_at)}
					{/if}
				</p>
				{#if playlist.source_url}
					<a
						href={playlist.source_url}
						target="_blank"
						rel="noopener noreferrer"
						class="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
					>
						<ExternalLink class="size-3" />
						Source
					</a>
				{/if}
				<div class="flex gap-2 pt-2">
					<Button onclick={playAll} disabled={!trackPage || trackPage.tracks.length === 0}>
						<Play class="size-4" fill="currentColor" />
						Play
					</Button>
					<Button variant="outline" onclick={shuffleAll} disabled={!trackPage || trackPage.tracks.length === 0}>
						<Shuffle class="size-4" />
						Shuffle
					</Button>
					<Button variant="outline" class="text-destructive hover:text-destructive" onclick={handleDelete}>
						<Trash2 class="size-4" />
					</Button>
				</div>
			</div>
		</div>

		{#if trackPage}
			<TrackTable tracks={trackPage.tracks} ondelete={handleRemoveTrack} />

			{#if totalPages > 1}
				<div class="flex items-center justify-center gap-2 shrink-0 pb-2">
					<Button
						variant="outline"
						size="sm"
						disabled={currentPage === 0}
						onclick={() => { currentPage--; loadTracks(); }}
					>
						Previous
					</Button>
					<span class="text-sm text-muted-foreground">
						Page {currentPage + 1} of {totalPages}
					</span>
					<Button
						variant="outline"
						size="sm"
						disabled={currentPage >= totalPages - 1}
						onclick={() => { currentPage++; loadTracks(); }}
					>
						Next
					</Button>
				</div>
			{/if}
		{/if}
	{:else}
		<p class="text-muted-foreground">Playlist not found.</p>
	{/if}
</div>
