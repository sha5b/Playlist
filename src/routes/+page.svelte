<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { getLibraryStats, importFolder } from '$lib/api/library';
	import { open } from '@tauri-apps/plugin-dialog';
	import { FolderOpen, Music, Disc, Users, ListMusic, Loader2 } from 'lucide-svelte';
	import { formatDurationLong, formatFileSize } from '$lib/utils/format';
	import { toast } from 'svelte-sonner';
	import { libraryStore } from '$lib/stores/library.svelte';
	import type { LibraryStats } from '$lib/types';

	let stats: LibraryStats | null = $state(null);
	let importing = $state(false);

	async function loadStats() {
		try {
			stats = await getLibraryStats();
		} catch (e) {
			console.error('Failed to load stats:', e);
		}
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
	});

	const statCards = $derived([
		{ label: 'Tracks', value: stats?.total_tracks, icon: Music },
		{ label: 'Albums', value: stats?.total_albums, icon: Disc },
		{ label: 'Artists', value: stats?.total_artists, icon: Users },
		{ label: 'Playlists', value: stats?.total_playlists, icon: ListMusic },
	]);
</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
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
	{:else}
		<Card>
			<CardHeader>
				<CardTitle>Getting Started</CardTitle>
				<CardDescription>Your library is empty. Import some music to get started.</CardDescription>
			</CardHeader>
			<CardContent class="flex gap-3 items-center">
				<Button onclick={handleImportFolder} disabled={importing}>
					{#if importing}
						<Loader2 class="size-4 animate-spin" />
						Importing...
					{:else}
						<FolderOpen class="size-4" />
						Import Folder
					{/if}
				</Button>
			</CardContent>
		</Card>
	{/if}

</div>
