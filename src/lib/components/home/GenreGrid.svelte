<script lang="ts">
	import { getGenres, getTracksByGenre } from '$lib/api/library';
	import { player } from '$lib/stores/player.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { toast } from 'svelte-sonner';
	import { Tags, Play, ListPlus, LoaderCircle } from 'lucide-svelte';
	import Skeleton from '$lib/components/shared/Skeleton.svelte';
	import SectionHeader from './SectionHeader.svelte';

	const MIN_TRACKS = 3;
	const MAX_GENRES = 12;

	// Muted background colors that work on dark themes
	const GENRE_COLORS = [
		'bg-rose-500/15 text-rose-300 hover:bg-rose-500/25',
		'bg-amber-500/15 text-amber-300 hover:bg-amber-500/25',
		'bg-emerald-500/15 text-emerald-300 hover:bg-emerald-500/25',
		'bg-sky-500/15 text-sky-300 hover:bg-sky-500/25',
		'bg-violet-500/15 text-violet-300 hover:bg-violet-500/25',
		'bg-pink-500/15 text-pink-300 hover:bg-pink-500/25',
		'bg-teal-500/15 text-teal-300 hover:bg-teal-500/25',
		'bg-orange-500/15 text-orange-300 hover:bg-orange-500/25',
		'bg-indigo-500/15 text-indigo-300 hover:bg-indigo-500/25',
		'bg-cyan-500/15 text-cyan-300 hover:bg-cyan-500/25',
		'bg-fuchsia-500/15 text-fuchsia-300 hover:bg-fuchsia-500/25',
		'bg-lime-500/15 text-lime-300 hover:bg-lime-500/25',
	];

	type GenreInfo = { name: string; trackCount: number };

	let genres = $state<GenreInfo[]>([]);
	let loading = $state(true);
	let loadingGenre = $state<string | null>(null);

	async function load() {
		loading = true;
		try {
			const allGenres = await getGenres();
			const entries = await Promise.all(
				allGenres.map(async (genre) => {
					const tracks = await getTracksByGenre(genre, MIN_TRACKS);
					return { name: genre, trackCount: tracks.length };
				})
			);
			genres = entries
				.filter((g) => g.trackCount >= MIN_TRACKS)
				.slice(0, MAX_GENRES);
		} catch (e) {
			console.error('Failed to load genres:', e);
		} finally {
			loading = false;
		}
	}

	async function playGenre(genre: string) {
		loadingGenre = genre;
		try {
			const tracks = await getTracksByGenre(genre, 50);
			if (tracks.length > 0) {
				await player.playTracks(tracks.map((t) => t.id), 0);
			}
		} catch {
			toast.error(`Failed to play ${genre}`);
		} finally {
			loadingGenre = null;
		}
	}

	async function queueGenre(genre: string, e: MouseEvent) {
		e.stopPropagation();
		loadingGenre = genre;
		try {
			const tracks = await getTracksByGenre(genre, 30);
			for (const track of tracks) {
				await player.addToQueue(track.id);
			}
			toast.success(`Added ${tracks.length} ${genre} tracks to queue`);
		} catch {
			toast.error(`Failed to queue ${genre}`);
		} finally {
			loadingGenre = null;
		}
	}

	function colorFor(index: number) {
		return GENRE_COLORS[index % GENRE_COLORS.length];
	}

	$effect(() => {
		libraryStore.version;
		load();
	});
</script>

{#if loading && genres.length === 0}
	<section class="space-y-2.5">
		<SectionHeader title="Genres" icon={Tags} />
		<div class="flex flex-wrap gap-2">
			{#each ['w-28', 'w-20', 'w-32', 'w-24', 'w-20', 'w-28'] as width}
				<Skeleton class="h-9 rounded-full {width}" />
			{/each}
		</div>
	</section>
{:else if genres.length > 0}
	<section class="space-y-2.5">
		<SectionHeader title="Genres" icon={Tags} />

		<div class="flex flex-wrap gap-2">
			{#each genres as genre, i (genre.name)}
				<!-- div[role=button] instead of <button>: the queue action inside would
					otherwise be invalid nested-interactive HTML and unreachable by keyboard -->
				<div
					role="button"
					tabindex="0"
					class="group relative flex items-center gap-2 rounded-full px-4 py-2 text-sm font-medium transition-all cursor-pointer {colorFor(i)}"
					aria-disabled={loadingGenre === genre.name}
					onclick={() => { if (loadingGenre !== genre.name) playGenre(genre.name); }}
					onkeydown={(e) => {
						if ((e.key === 'Enter' || e.key === ' ') && loadingGenre !== genre.name) {
							e.preventDefault();
							playGenre(genre.name);
						}
					}}
				>
					{#if loadingGenre === genre.name}
						<LoaderCircle class="size-3.5 animate-spin" />
					{:else}
						<Play class="size-3.5 opacity-60 group-hover:opacity-100 transition-opacity" />
					{/if}
					<span>{genre.name}</span>
					<button
						type="button"
						class="opacity-0 group-hover:opacity-70 hover:!opacity-100 focus-visible:opacity-100 transition-opacity cursor-pointer"
						onclick={(e) => queueGenre(genre.name, e)}
						aria-label={`Add ${genre.name} to queue`}
					>
						<ListPlus class="size-3.5" />
					</button>
				</div>
			{/each}
		</div>
	</section>
{/if}
