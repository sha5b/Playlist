<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { addPlaylist } from '$lib/api/manager';
	import { importFolder } from '$lib/api/library';
	import { open } from '@tauri-apps/plugin-dialog';
	import { Music, ListMusic, FolderOpen, Loader2, Plus } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';

	let { onChanged }: { onChanged: () => void } = $props();

	let playlistUrlInput = $state('');
	let addingPlaylist = $state(false);
	let importing = $state(false);

	async function handleAddPlaylist() {
		const url = playlistUrlInput.trim();
		if (!url) return;

		addingPlaylist = true;
		try {
			await addPlaylist(url);
			toast.success('Playlist added successfully');
			playlistUrlInput = '';
			onChanged();
		} catch (e) {
			toast.error(`Failed to add playlist: ${e}`);
		} finally {
			addingPlaylist = false;
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') handleAddPlaylist();
	}

	async function handleImportFolder() {
		const selected = await open({ directory: true, multiple: false });
		if (!selected) return;

		importing = true;
		try {
			const count = await importFolder(selected);
			toast.success(`Imported ${count} track${count !== 1 ? 's' : ''}`);
			onChanged();
		} catch (e) {
			toast.error(`Import failed: ${e}`);
		} finally {
			importing = false;
		}
	}
</script>

<div class="flex flex-col items-center justify-center min-h-[60vh] text-center space-y-8 max-w-lg mx-auto">
	<div class="rounded-full bg-muted p-6">
		<Music class="size-12 text-muted-foreground" />
	</div>

	<div class="space-y-3">
		<h1 class="text-3xl font-bold tracking-tight">Your music. Your data. Your rules.</h1>
		<p class="text-muted-foreground text-lg">
			Break free from streaming platforms that lock your library behind a subscription and pay artists fractions of a cent.
			Build your own collection, keep it forever, play it anywhere.
		</p>
		<p class="text-sm text-muted-foreground">
			Support artists directly — buy their albums, back them on Patreon, go to their shows. Use your freedom responsibly.
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
					onkeydown={handleKeydown}
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
		<div class="flex items-center gap-3 text-sm text-muted-foreground">
			<div class="h-px flex-1 bg-border"></div>
			<span>or</span>
			<div class="h-px flex-1 bg-border"></div>
		</div>
		<Button variant="outline" class="w-full" onclick={handleImportFolder} disabled={importing}>
			{#if importing}
				<Loader2 class="size-4 animate-spin" />
				Importing...
			{:else}
				<FolderOpen class="size-4" />
				Import a music folder
			{/if}
		</Button>
	</div>
</div>
