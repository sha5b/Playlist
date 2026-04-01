<script lang="ts">
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { getPlaylist, deletePlaylist, removeFromPlaylist } from '$lib/api/library';
	import TrackTable from '$lib/components/library/TrackTable.svelte';
	import { Button } from '$lib/components/ui/button';
	import { player } from '$lib/stores/player.svelte';
	import { formatDurationLong } from '$lib/utils/format';
	import { ArrowLeft, ListMusic, Play, Shuffle, Trash2, Loader2 } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import type { PlaylistDetail, Track } from '$lib/types';

	let detail: PlaylistDetail | null = $state(null);
	let loading = $state(true);

	const playlistId = $derived(Number(page.params.id));

	async function load(id: number) {
		loading = true;
		try {
			detail = await getPlaylist(id);
		} catch (e) {
			toast.error('Failed to load playlist');
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load(playlistId);
	});

	function playAll() {
		if (!detail || detail.tracks.length === 0) return;
		player.playTracks(detail.tracks.map((t) => t.id), 0);
	}

	function shuffleAll() {
		if (!detail || detail.tracks.length === 0) return;
		const ids = detail.tracks.map((t) => t.id);
		const randomStart = Math.floor(Math.random() * ids.length);
		player.playTracks(ids, randomStart);
	}

	async function handleDelete() {
		if (!detail) return;
		if (!confirm(`Delete playlist "${detail.playlist.name}"?`)) return;
		try {
			await deletePlaylist(detail.playlist.id);
			toast.success('Playlist deleted');
			goto('/library/playlists');
		} catch (e) {
			toast.error('Failed to delete playlist');
		}
	}

	async function handleRemoveTrack(track: Track) {
		if (!detail) return;
		try {
			await removeFromPlaylist(detail.playlist.id, track.id);
			toast.success(`Removed "${track.title}"`);
			await load(playlistId);
		} catch (e) {
			toast.error('Failed to remove track');
		}
	}
</script>

<div class="space-y-6">
	<a
		href="/library/playlists"
		class="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors"
	>
		<ArrowLeft class="size-4" />
		Playlists
	</a>

	{#if loading}
		<div class="flex items-center justify-center h-48">
			<Loader2 class="size-6 animate-spin text-muted-foreground" />
		</div>
	{:else if detail}
		<div class="flex gap-6 items-end">
			<div class="size-48 rounded-lg bg-muted flex items-center justify-center overflow-hidden shrink-0 shadow-lg">
				{#if detail.playlist.cover_art_path}
					<img
						src="https://asset.localhost/{detail.playlist.cover_art_path}"
						alt={detail.playlist.name}
						class="size-full object-cover"
					/>
				{:else}
					<ListMusic class="size-16 text-muted-foreground" />
				{/if}
			</div>
			<div class="space-y-2">
				<p class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Playlist</p>
				<h1 class="text-3xl font-bold tracking-tight">{detail.playlist.name}</h1>
				{#if detail.playlist.description}
					<p class="text-sm text-muted-foreground">{detail.playlist.description}</p>
				{/if}
				<p class="text-sm text-muted-foreground">
					{detail.tracks.length} track{detail.tracks.length !== 1 ? 's' : ''}
					{#if detail.playlist.total_duration_ms > 0}
						&middot; {formatDurationLong(detail.playlist.total_duration_ms)}
					{/if}
				</p>
				<div class="flex gap-2 pt-2">
					<Button onclick={playAll} disabled={detail.tracks.length === 0}>
						<Play class="size-4" fill="currentColor" />
						Play
					</Button>
					<Button variant="outline" onclick={shuffleAll} disabled={detail.tracks.length === 0}>
						<Shuffle class="size-4" />
						Shuffle
					</Button>
					<Button variant="outline" class="text-destructive hover:text-destructive" onclick={handleDelete}>
						<Trash2 class="size-4" />
					</Button>
				</div>
			</div>
		</div>

		<TrackTable tracks={detail.tracks} ondelete={handleRemoveTrack} />
	{:else}
		<p class="text-muted-foreground">Playlist not found.</p>
	{/if}
</div>
