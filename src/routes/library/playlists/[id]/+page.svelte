<script lang="ts">
	import { untrack } from 'svelte';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { getPlaylist, getPlaylistTracks, deletePlaylist, removeFromPlaylist, exportPlaylistM3u } from '$lib/api/library';
	import { save } from '@tauri-apps/plugin-dialog';
	import TrackTable from '$lib/components/library/TrackTable.svelte';
	import SmartPlaylistDialog from '$lib/components/library/SmartPlaylistDialog.svelte';
	import { Button } from '$lib/components/ui/button';
	import { player } from '$lib/stores/player.svelte';
	import { formatDurationLong, assetUrl, shuffleArray, platformLabel, platformColor, timeAgo } from '$lib/utils/format';
	import { Badge } from '$lib/components/ui/badge';
	import { ArrowLeft, ListMusic, Play, Shuffle, Trash2, Loader2, ExternalLink, RefreshCw, Clock, ChevronLeft, ChevronRight, FileDown, Sparkles, Pencil } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import type { Playlist, TrackPage, Track } from '$lib/types';

	let deleteOpen = $state(false);
	let editRulesOpen = $state(false);

	let playlist = $state<Playlist | null>(null);
	let trackPage = $state<TrackPage | null>(null);
	let loading = $state(true);
	let currentPage = $state(0);
	const pageSize = 50;

	const playlistId = $derived(Number(page.params.id));
	const totalPages = $derived(trackPage ? Math.ceil(trackPage.total / pageSize) : 0);

	// Monotonic request id: a slow response for playlist A must not clobber
	// the state after navigation to playlist B has already loaded.
	let loadSeq = 0;

	async function load(id: number) {
		const seq = ++loadSeq;
		loading = true;
		// Reset so navigating between playlists shows the loading spinner
		// instead of the previous playlist's data until the fetch resolves.
		playlist = null;
		trackPage = null;
		try {
			const detail = await getPlaylist(id);
			const tp = await getPlaylistTracks(id, 0, pageSize);
			if (seq !== loadSeq) return;
			if (detail) {
				playlist = detail.playlist;
			}
			trackPage = tp;
		} catch (e) {
			toast.error('Failed to load playlist');
		} finally {
			if (seq === loadSeq) loading = false;
		}
	}

	async function loadTracks() {
		if (!playlist) return;
		const seq = loadSeq;
		try {
			const tp = await getPlaylistTracks(playlist.id, currentPage * pageSize, pageSize);
			if (seq !== loadSeq) return;
			trackPage = tp;
		} catch (e) {
			console.error('Failed to load tracks:', e);
		}
	}

	$effect(() => {
		playlistId;
		// untrack: load() reads currentPage — tracking it here made the effect
		// re-run on every pagination click and reset back to page 0.
		untrack(() => {
			currentPage = 0;
			load(playlistId);
		});
	});

	// Fetch ALL track ids, not just the visible 50-track page — otherwise
	// "Play"/"Shuffle" on a 500-track playlist queued only the current page.
	async function allTrackIds(): Promise<number[]> {
		if (!playlist || !trackPage) return [];
		if (trackPage.total <= trackPage.tracks.length && currentPage === 0) {
			return trackPage.tracks.map((t) => t.id);
		}
		const all = await getPlaylistTracks(playlist.id, 0, trackPage.total);
		return all.tracks.map((t) => t.id);
	}

	async function playAll() {
		if (!trackPage || trackPage.tracks.length === 0) return;
		try {
			player.playTracks(await allTrackIds(), 0);
		} catch (e) {
			toast.error('Failed to start playback');
		}
	}

	async function shuffleAll() {
		if (!trackPage || trackPage.tracks.length === 0) return;
		try {
			player.playTracks(shuffleArray(await allTrackIds()), 0);
		} catch (e) {
			toast.error('Failed to start playback');
		}
	}

	async function handleDelete() {
		if (!playlist) return;
		// bits-ui v2 AlertDialog.Action does not auto-close — close explicitly
		deleteOpen = false;
		try {
			await deletePlaylist(playlist.id);
			toast.success('Playlist deleted');
			goto('/library/playlists');
		} catch (e) {
			toast.error('Failed to delete playlist');
		}
	}

	async function handleExportM3u() {
		if (!playlist) return;
		try {
			const dest = await save({
				defaultPath: `${playlist.name}.m3u`,
				filters: [{ name: 'M3U Playlist', extensions: ['m3u', 'm3u8'] }]
			});
			if (!dest) return;
			const count = await exportPlaylistM3u(playlist.id, dest);
			toast.success(`Exported ${count} track${count !== 1 ? 's' : ''} to .m3u`);
		} catch (e) {
			toast.error('Failed to export playlist', { description: String(e) });
		}
	}

	async function handleRemoveTrack(track: Track) {
		if (!playlist) return;
		try {
			await removeFromPlaylist(playlist.id, track.id);
			toast.success(`Removed "${track.title}"`);
			await loadTracks();
			// If the total shrank, clamp to the new last page and reload
			const lastPage = trackPage ? Math.max(0, Math.ceil(trackPage.total / pageSize) - 1) : 0;
			if (currentPage > lastPage) {
				currentPage = lastPage;
				await loadTracks();
			}
		} catch (e) {
			toast.error('Failed to remove track');
		}
	}
</script>

<div class="flex flex-col flex-1 min-h-0 gap-6">
	<!-- Back navigation -->
	<a
		href="/library/playlists"
		class="inline-flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors shrink-0 w-fit"
	>
		<ArrowLeft class="size-4" />
		Playlists
	</a>

	{#if loading && !playlist}
		<div class="flex items-center justify-center h-48">
			<Loader2 class="size-6 animate-spin text-muted-foreground" />
		</div>
	{:else if playlist}
		<!-- Header -->
		<div class="flex gap-6 items-start shrink-0">
			<!-- Cover art -->
			<div class="size-52 rounded-xl bg-muted flex items-center justify-center overflow-hidden shrink-0 shadow-lg ring-1 ring-white/5">
				{#if playlist.cover_art_path}
					<img
						src={assetUrl(playlist.cover_art_path)}
						alt={playlist.name}
						class="size-full object-cover"
					/>
				{:else}
					<div class="size-full bg-gradient-to-br from-muted to-muted/40 flex items-center justify-center">
						<ListMusic class="size-16 text-muted-foreground/40" />
					</div>
				{/if}
			</div>

			<!-- Info -->
			<div class="space-y-3 py-2 min-w-0 flex-1">
				<!-- Type label + badges -->
				<div class="flex items-center gap-2 flex-wrap">
					<p class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
						{playlist.is_smart ? 'Smart Playlist' : 'Playlist'}
					</p>
					{#if playlist.is_smart}
						<Badge variant="secondary" class="text-xs gap-1">
							<Sparkles class="size-2.5" />
							Smart
						</Badge>
					{/if}
					{#if playlist.source_platform}
						<Badge variant={platformColor(playlist.source_platform)} class="text-xs">
							{platformLabel(playlist.source_platform)}
						</Badge>
					{/if}
					{#if playlist.is_synced}
						<Badge variant="outline" class="text-xs gap-1">
							<RefreshCw class="size-2.5" />
							Synced
						</Badge>
					{/if}
				</div>

				<!-- Title -->
				<h1 class="text-4xl font-bold tracking-tight leading-tight">{playlist.name}</h1>

				<!-- Description -->
				{#if playlist.description}
					<p class="text-sm text-muted-foreground/80 leading-relaxed max-w-xl">{playlist.description}</p>
				{/if}

				<!-- Metadata row -->
				<div class="flex items-center gap-3 text-sm text-muted-foreground flex-wrap">
					<span class="font-medium text-foreground/80">
						{trackPage ? trackPage.total : 0} track{(trackPage?.total ?? 0) !== 1 ? 's' : ''}
					</span>
					{#if playlist.total_duration_ms > 0}
						<span class="opacity-40">&middot;</span>
						<span class="flex items-center gap-1">
							<Clock class="size-3.5" />
							{formatDurationLong(playlist.total_duration_ms)}
						</span>
					{/if}
					{#if playlist.last_synced_at}
						<span class="opacity-40">&middot;</span>
						<span>Synced {timeAgo(playlist.last_synced_at)}</span>
					{/if}
				</div>

				<!-- Source link -->
				{#if playlist.source_url}
					<a
						href={playlist.source_url}
						target="_blank"
						rel="noopener noreferrer"
						class="inline-flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
					>
						<ExternalLink class="size-3" />
						View source
					</a>
				{/if}

				<!-- Actions -->
				<div class="flex items-center gap-2 pt-1">
					<Button onclick={playAll} disabled={!trackPage || trackPage.tracks.length === 0} class="gap-2">
						<Play class="size-4" fill="currentColor" />
						Play
					</Button>
					<Button variant="outline" onclick={shuffleAll} disabled={!trackPage || trackPage.tracks.length === 0} class="gap-2">
						<Shuffle class="size-4" />
						Shuffle
					</Button>
					<Button variant="outline" onclick={handleExportM3u} disabled={!trackPage || trackPage.tracks.length === 0} class="gap-2">
						<FileDown class="size-4" />
						Export .m3u
					</Button>
					{#if playlist.is_smart}
						<Button variant="outline" onclick={() => (editRulesOpen = true)} class="gap-2">
							<Pencil class="size-4" />
							Edit rules
						</Button>
					{/if}
					<AlertDialog.Root bind:open={deleteOpen}>
						<AlertDialog.Trigger>
							{#snippet child({ props })}
								<Button variant="ghost" size="icon" class="text-muted-foreground hover:text-destructive rounded-full" {...props}>
									<Trash2 class="size-4" />
								</Button>
							{/snippet}
						</AlertDialog.Trigger>
						<AlertDialog.Content>
							<AlertDialog.Header>
								<AlertDialog.Title>Delete Playlist</AlertDialog.Title>
								<AlertDialog.Description>
									Are you sure you want to delete "{playlist?.name}"? This action cannot be undone.
								</AlertDialog.Description>
							</AlertDialog.Header>
							<AlertDialog.Footer>
								<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
								<AlertDialog.Action onclick={handleDelete}>Delete</AlertDialog.Action>
							</AlertDialog.Footer>
						</AlertDialog.Content>
					</AlertDialog.Root>
				</div>
			</div>
		</div>

		<!-- Track list -->
		{#if trackPage}
			<!-- Smart playlists are rule-based: no manual remove affordance -->
			<TrackTable
				tracks={trackPage.tracks}
				ondelete={playlist.is_smart ? undefined : handleRemoveTrack}
				onplay={async (t) => {
					// Row play must queue the WHOLE playlist, not just the
					// visible 50-track page.
					try {
						const ids = await allTrackIds();
						const idx = ids.indexOf(t.id);
						player.playTracks(ids, idx >= 0 ? idx : 0);
					} catch {
						player.playTracks([t.id], 0);
					}
				}}
				referrer={playlist ? { type: 'playlist', id: playlist.id, label: playlist.name } : undefined}
			/>

			{#if totalPages > 1}
				<div class="flex items-center justify-center gap-3 shrink-0 pb-2">
					<Button
						variant="outline"
						size="sm"
						disabled={currentPage === 0}
						onclick={() => { currentPage--; loadTracks(); }}
						class="gap-1"
					>
						<ChevronLeft class="size-4" />
						Previous
					</Button>
					<span class="text-sm text-muted-foreground tabular-nums px-2">
						{currentPage + 1} / {totalPages}
					</span>
					<Button
						variant="outline"
						size="sm"
						disabled={currentPage >= totalPages - 1}
						onclick={() => { currentPage++; loadTracks(); }}
						class="gap-1"
					>
						Next
						<ChevronRight class="size-4" />
					</Button>
				</div>
			{/if}
		{/if}
	{:else}
		<div class="flex flex-col items-center justify-center py-24 gap-3">
			<ListMusic class="size-10 text-muted-foreground/40" />
			<p class="text-muted-foreground">Playlist not found</p>
			<Button variant="outline" size="sm" onclick={() => goto('/library/playlists')}>
				Back to playlists
			</Button>
		</div>
	{/if}
</div>
