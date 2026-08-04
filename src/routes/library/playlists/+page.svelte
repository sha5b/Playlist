<script lang="ts">
	import { getPlaylists, createPlaylist } from '$lib/api/library';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as Dialog from '$lib/components/ui/dialog';
	import CardGridSkeleton from '$lib/components/shared/CardGridSkeleton.svelte';
	import { ListMusic, Plus, Play, Music2 } from 'lucide-svelte';
	import { formatDurationLong, assetUrl } from '$lib/utils/format';
	import type { Playlist } from '$lib/types';
	import { toast } from 'svelte-sonner';

	let playlists: Playlist[] = $state([]);
	let loading = $state(true);
	let createOpen = $state(false);
	let newName = $state('');
	let newDescription = $state('');
	let creating = $state(false);

	async function load() {
		loading = true;
		try {
			playlists = await getPlaylists();
		} catch (e) {
			console.error('Failed to load playlists:', e);
		} finally {
			loading = false;
		}
	}

	async function handleCreate() {
		if (!newName.trim()) return;
		creating = true;
		try {
			await createPlaylist(newName.trim(), newDescription.trim() || undefined);
			createOpen = false;
			newName = '';
			newDescription = '';
			toast.success('Playlist created');
			await load();
		} catch (e) {
			toast.error('Failed to create playlist');
			console.error('Failed to create playlist:', e);
		} finally {
			creating = false;
		}
	}

	function openCreateDialog() {
		newName = '';
		newDescription = '';
		createOpen = true;
	}

	$effect(() => {
		load();
	});
</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
	<div class="flex items-center justify-between">
		<div>
			<h1 class="text-3xl font-bold tracking-tight">Playlists</h1>
			<p class="text-muted-foreground mt-1">
				{loading ? 'Loading...' : `${playlists.length} playlist${playlists.length !== 1 ? 's' : ''}`}
			</p>
		</div>
		<Button onclick={openCreateDialog} class="gap-2">
			<Plus class="size-4" />
			New Playlist
		</Button>
	</div>

	{#if loading}
		<CardGridSkeleton />
	{:else if playlists.length === 0}
		<div class="flex flex-col items-center justify-center py-20 rounded-xl border border-dashed border-border/60 gap-4">
			<div class="size-16 rounded-2xl bg-muted/30 flex items-center justify-center">
				<Music2 class="size-8 text-muted-foreground/30" />
			</div>
			<div class="text-center space-y-1">
				<p class="text-muted-foreground font-medium">No playlists yet</p>
				<p class="text-muted-foreground/70 text-sm">Create your first playlist to organize your music</p>
			</div>
			<Button variant="outline" onclick={openCreateDialog} class="gap-2 mt-1">
				<Plus class="size-4" />
				Create Playlist
			</Button>
		</div>
	{:else}
		<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
			{#each playlists as playlist}
				<a href="/library/playlists/{playlist.id}" class="group rounded-xl transition-all duration-200 hover:bg-muted/30 p-2.5 -m-0.5">
					<div class="aspect-square rounded-lg bg-muted/80 flex items-center justify-center overflow-hidden mb-3 relative shadow-sm group-hover:shadow-md transition-shadow">
						{#if playlist.cover_art_path}
							<img
								src={assetUrl(playlist.cover_art_path)}
								alt={playlist.name}
								class="size-full object-cover group-hover:scale-105 transition-transform duration-300"
								loading="lazy"
							/>
						{:else}
							<div class="size-full bg-gradient-to-br from-muted to-muted/50 flex items-center justify-center">
								<ListMusic class="size-12 text-muted-foreground/50" />
							</div>
						{/if}
						<!-- Play overlay on hover -->
						<div class="absolute inset-0 bg-black/0 group-hover:bg-black/30 transition-all duration-200 flex items-center justify-center">
							<div class="size-11 rounded-full bg-primary flex items-center justify-center opacity-0 group-hover:opacity-100 translate-y-2 group-hover:translate-y-0 transition-all duration-200 shadow-lg">
								<Play class="size-5 text-primary-foreground ml-0.5" fill="currentColor" />
							</div>
						</div>
					</div>
					<div class="space-y-0.5 px-0.5">
						<p class="text-sm font-medium truncate group-hover:text-foreground transition-colors">{playlist.name}</p>
						<div class="flex items-center gap-1.5 text-xs text-muted-foreground">
							<span>{playlist.track_count} track{playlist.track_count !== 1 ? 's' : ''}</span>
							{#if playlist.total_duration_ms > 0}
								<span class="opacity-50">&middot;</span>
								<span>{formatDurationLong(playlist.total_duration_ms)}</span>
							{/if}
						</div>
					</div>
				</a>
			{/each}
		</div>
	{/if}
</div>

<Dialog.Root bind:open={createOpen}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Create Playlist</Dialog.Title>
			<Dialog.Description>Give your playlist a name to get started.</Dialog.Description>
		</Dialog.Header>
		<form onsubmit={(e) => { e.preventDefault(); handleCreate(); }} class="space-y-4">
			<div class="space-y-2">
				<Input
					placeholder="Playlist name"
					bind:value={newName}
					autofocus
				/>
				<Input
					placeholder="Description (optional)"
					bind:value={newDescription}
				/>
			</div>
			<Dialog.Footer>
				<Dialog.Close>
					{#snippet child({ props })}
						<Button variant="outline" type="button" {...props}>Cancel</Button>
					{/snippet}
				</Dialog.Close>
				<Button type="submit" disabled={!newName.trim() || creating}>
					{creating ? 'Creating...' : 'Create'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
