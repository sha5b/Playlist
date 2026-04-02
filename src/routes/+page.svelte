<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { getLibraryStats, importFolder } from '$lib/api/library';
	import { addPlaylist, getMonitoredPlaylists } from '$lib/api/manager';
	import { open } from '@tauri-apps/plugin-dialog';
	import { FolderOpen, Music, Disc, Users, ListMusic, Loader2, Plus } from 'lucide-svelte';
	import { formatDurationLong, formatFileSize } from '$lib/utils/format';
	import { toast } from 'svelte-sonner';
	import { libraryStore } from '$lib/stores/library.svelte';
	import type { LibraryStats } from '$lib/types';

	let stats = $state<LibraryStats | null>(null);
	let importing = $state(false);
	let playlistUrlInput = $state('');
	let addingPlaylist = $state(false);
	let hasPlaylists = $state(false);

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

	async function handleAddPlaylist() {
		const url = playlistUrlInput.trim();
		if (!url) return;

		addingPlaylist = true;
		try {
			await addPlaylist(url);
			toast.success('Playlist added successfully');
			playlistUrlInput = '';
			hasPlaylists = true;
			await loadStats();
		} catch (e) {
			toast.error(`Failed to add playlist: ${e}`);
		} finally {
			addingPlaylist = false;
		}
	}

	function handlePlaylistKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') handleAddPlaylist();
	}

	async function handleImportFolder() {
		const selected = await open({ directory: true, multiple: false });
		if (!selected) return;

		importing = true;
		try {
			const count = await importFolder(selected);
			toast.success(`Imported ${count} track${count !== 1 ? 's' : ''}`);
			await loadStats();
		} catch (e) {
			toast.error(`Import failed: ${e}`);
		} finally {
			importing = false;
		}
	}

	$effect(() => {
		libraryStore.version;
		loadStats();
		loadPlaylists();
	});

	const statCards = $derived([
		{ label: 'Tracks', value: stats?.total_tracks, icon: Music },
		{ label: 'Albums', value: stats?.total_albums, icon: Disc },
		{ label: 'Artists', value: stats?.total_artists, icon: Users },
		{ label: 'Playlists', value: stats?.total_playlists, icon: ListMusic },
	]);

	const isEmpty = $derived(!hasPlaylists && stats !== null && stats.total_tracks === 0);
</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
	{#if isEmpty}
		<div class="flex flex-col items-center justify-center min-h-[60vh] text-center space-y-6 max-w-lg mx-auto">
			<div class="rounded-full bg-muted p-6">
				<Music class="size-12 text-muted-foreground" />
			</div>
			<div class="space-y-2">
				<h1 class="text-3xl font-bold tracking-tight">Welcome to Playlist</h1>
				<p class="text-muted-foreground text-lg">
					Monitor your favorite playlists from YouTube, Spotify, and more.
					Tracks are automatically synced and downloaded to your local library.
				</p>
			</div>

			<div class="w-full space-y-3">
				<p class="text-sm font-medium text-muted-foreground">Add your first playlist to get started</p>
				<div class="flex gap-2">
					<div class="relative flex-1">
						<ListMusic class="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
						<Input
							placeholder="Paste a playlist URL..."
							class="pl-10"
							bind:value={playlistUrlInput}
							onkeydown={handlePlaylistKeydown}
						/>
					</div>
					<Button
						onclick={handleAddPlaylist}
						disabled={!playlistUrlInput.trim() || addingPlaylist}
					>
						{#if addingPlaylist}
							<Loader2 class="size-4 animate-spin" />
						{:else}
							<Plus class="size-4" />
						{/if}
						Add
					</Button>
				</div>
			</div>
		</div>
	{:else}
		<div>
			<h1 class="text-3xl font-bold tracking-tight">Home</h1>
			<p class="text-muted-foreground mt-1">Welcome to Playlist</p>
		</div>

		<div class="grid grid-cols-2 gap-4 md:grid-cols-4">
			{#each statCards as card}
				<Card>
					<CardHeader class="flex flex-row items-center justify-between pb-2">
						<CardDescription>{card.label}</CardDescription>
						<card.icon class="size-4 text-muted-foreground" />
					</CardHeader>
					<CardContent>
						<p class="text-2xl font-bold tabular-nums">{card.value ?? '...'}</p>
					</CardContent>
				</Card>
			{/each}
		</div>

		{#if stats && stats.total_tracks > 0}
			<Card>
				<CardHeader>
					<CardTitle>Library</CardTitle>
					<CardDescription>
						{formatDurationLong(stats.total_duration_ms)} of music
						 &middot; {formatFileSize(stats.total_size_bytes)}
					</CardDescription>
				</CardHeader>
				<CardContent>
					<Button variant="outline" onclick={handleImportFolder} disabled={importing}>
						{#if importing}
							<Loader2 class="size-4 animate-spin" />
							Importing...
						{:else}
							<FolderOpen class="size-4" />
							Import More Music
						{/if}
					</Button>
				</CardContent>
			</Card>
		{/if}
	{/if}
</div>
