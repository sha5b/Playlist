<script lang="ts">
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Loader2 } from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import { updateTrackTags, updateTracksTags } from '$lib/api/library';
	import type { Track, TrackTagUpdate } from '$lib/types';

	let {
		open = $bindable(false),
		tracks,
		onsaved,
	}: {
		open?: boolean;
		/** One track for single edit; several for batch edit. */
		tracks: Track[];
		onsaved?: (updated: Track[]) => void;
	} = $props();

	const batch = $derived(tracks.length > 1);

	let title = $state('');
	let artist = $state('');
	let album = $state('');
	let albumArtist = $state('');
	let genre = $state('');
	let year = $state('');
	let trackNumber = $state('');
	let saving = $state(false);
	let error = $state('');

	// Prefill fields each time the dialog opens (single edit only; batch starts blank).
	$effect(() => {
		if (!open) return;
		error = '';
		const t = tracks.length === 1 ? tracks[0] : null;
		title = t?.title ?? '';
		artist = t?.artist_name ?? '';
		album = t?.album_title ?? '';
		albumArtist = t?.album_artist ?? '';
		genre = t?.genre ?? '';
		year = t?.year != null ? String(t.year) : '';
		trackNumber = t?.track_number != null ? String(t.track_number) : '';
	});

	function buildUpdate(): TrackTagUpdate {
		const u: TrackTagUpdate = {};
		if (!batch && title.trim()) u.title = title.trim();
		if (artist.trim()) u.artist = artist.trim();
		if (album.trim()) u.album = album.trim();
		if (albumArtist.trim()) u.album_artist = albumArtist.trim();
		if (genre.trim()) u.genre = genre.trim();
		const y = parseInt(year, 10);
		if (year.trim() && !Number.isNaN(y)) u.year = y;
		const n = parseInt(trackNumber, 10);
		if (!batch && trackNumber.trim() && !Number.isNaN(n)) u.track_number = n;
		return u;
	}

	async function save() {
		if (tracks.length === 0 || saving) return;
		const update = buildUpdate();
		if (Object.keys(update).length === 0) {
			open = false;
			return;
		}
		saving = true;
		error = '';
		try {
			let updated: Track[];
			if (batch) {
				updated = await updateTracksTags(
					tracks.map((t) => t.id),
					update
				);
			} else {
				updated = [await updateTrackTags(tracks[0].id, update)];
			}
			toast.success(batch ? `Updated tags on ${updated.length} tracks` : 'Tags updated');
			onsaved?.(updated);
			open = false;
		} catch (e) {
			error = String(e);
			toast.error('Failed to update tags', { description: String(e) });
		} finally {
			saving = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Edit Tags</Dialog.Title>
			<Dialog.Description>
				{#if batch}
					Editing {tracks.length} tracks. Leave a field blank to keep each track's existing value.
				{:else}
					Changes are written to the audio file and the library. Leave a field blank to keep its current value.
				{/if}
			</Dialog.Description>
		</Dialog.Header>
		<form
			class="space-y-3"
			onsubmit={(e) => {
				e.preventDefault();
				save();
			}}
		>
			{#if !batch}
				<div class="space-y-1.5">
					<label for="tag-title" class="text-sm font-medium">Title</label>
					<Input id="tag-title" bind:value={title} placeholder="Title" disabled={saving} />
				</div>
			{/if}
			<div class="space-y-1.5">
				<label for="tag-artist" class="text-sm font-medium">Artist</label>
				<Input id="tag-artist" bind:value={artist} placeholder={batch ? 'Keep existing' : 'Artist'} disabled={saving} />
			</div>
			<div class="space-y-1.5">
				<label for="tag-album" class="text-sm font-medium">Album</label>
				<Input id="tag-album" bind:value={album} placeholder={batch ? 'Keep existing' : 'Album'} disabled={saving} />
			</div>
			<div class="space-y-1.5">
				<label for="tag-album-artist" class="text-sm font-medium">Album Artist</label>
				<Input id="tag-album-artist" bind:value={albumArtist} placeholder={batch ? 'Keep existing' : 'Album artist'} disabled={saving} />
			</div>
			<div class="grid grid-cols-2 gap-3">
				<div class="space-y-1.5">
					<label for="tag-genre" class="text-sm font-medium">Genre</label>
					<Input id="tag-genre" bind:value={genre} placeholder={batch ? 'Keep existing' : 'Genre'} disabled={saving} />
				</div>
				<div class="space-y-1.5">
					<label for="tag-year" class="text-sm font-medium">Year</label>
					<Input id="tag-year" bind:value={year} type="number" placeholder={batch ? 'Keep existing' : 'Year'} disabled={saving} />
				</div>
			</div>
			{#if !batch}
				<div class="space-y-1.5">
					<label for="tag-track-number" class="text-sm font-medium">Track Number</label>
					<Input id="tag-track-number" bind:value={trackNumber} type="number" placeholder="Track number" disabled={saving} />
				</div>
			{/if}
			{#if error}
				<p class="text-sm text-destructive break-words">{error}</p>
			{/if}
			<Dialog.Footer>
				<Dialog.Close>
					<Button type="button" variant="outline" disabled={saving}>Cancel</Button>
				</Dialog.Close>
				<Button type="submit" disabled={saving}>
					{#if saving}
						<Loader2 class="size-4 animate-spin" />
					{/if}
					Save
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
