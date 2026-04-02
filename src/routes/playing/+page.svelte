<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Slider } from '$lib/components/ui/slider';
	import {
		Shuffle, SkipBack, Play, Pause, SkipForward,
		Repeat, Repeat1, Music, Trash2, AudioLines, History,
		Image, Film, Type, Info
	} from 'lucide-svelte';
	import { player } from '$lib/stores/player.svelte';
	import { getTrack } from '$lib/api/library';
	import { formatDuration, assetUrl } from '$lib/utils/format';
	import DndQueueList from '$lib/components/player/DndQueueList.svelte';
	import SyncedLyrics from '$lib/components/player/SyncedLyrics.svelte';
	import type { Track } from '$lib/types';

	type DisplayMode = 'artwork' | 'video' | 'lyrics';
	let preferredMode = $state<DisplayMode>('artwork');
	let fullTrack = $state<Track | null>(null);

	const hasVideo = $derived(!!fullTrack?.music_video_path);
	const hasLyrics = $derived(!!fullTrack?.lyrics);

	// The preferred mode persists across tracks. When a track has the content
	// (lyrics/video), it shows that mode; otherwise falls back to artwork.
	const displayMode = $derived.by(() => {
		if (preferredMode === 'video' && hasVideo) return 'video';
		if (preferredMode === 'lyrics' && hasLyrics) return 'lyrics';
		return 'artwork';
	});

	$effect(() => {
		const id = player.currentTrack?.id;
		if (id) {
			getTrack(id).then(t => { fullTrack = t; });
		} else {
			fullTrack = null;
		}
	});

	function handleProgressChange(value: number) {
		const seconds = (value / 100) * (player.durationMs / 1000);
		player.seek(seconds);
	}

	const progressPercent = $derived(
		player.durationMs > 0 ? (player.positionMs / player.durationMs) * 100 : 0
	);

	const RepeatIcon = $derived(
		player.repeat === 'one' ? Repeat1 : Repeat
	);

	const hasTrack = $derived(player.currentTrack !== null);
	const hasUpNext = $derived(
		player.queuePosition !== null && player.queuePosition < player.queueTracks.length - 1
	);
</script>

<div class="flex flex-col flex-1 min-h-0 overflow-y-auto">
	{#if !hasTrack}
		<!-- Empty state -->
		<div class="flex flex-col items-center justify-center flex-1 gap-4">
			<div class="size-20 rounded-2xl bg-muted flex items-center justify-center">
				<AudioLines class="size-10 text-muted-foreground" strokeWidth={1.5} />
			</div>
			<div class="text-center">
				<h1 class="text-2xl font-bold tracking-tight">Nothing playing</h1>
				<p class="text-muted-foreground mt-1">Pick something from your library to get started</p>
			</div>
		</div>
	{:else}
		<div class="p-6 space-y-8">
			<!-- Hero: Current Track -->
			<div class="flex gap-8 items-start">
				<!-- Artwork always visible -->
				<div class="size-64 lg:size-72 shrink-0 rounded-xl bg-muted overflow-hidden shadow-2xl shadow-black/40">
					{#if player.currentTrack?.cover_art_path}
						<img
							src={assetUrl(player.currentTrack.cover_art_path)}
							alt=""
							class="size-full object-cover"
						/>
					{:else}
						<div class="size-full flex items-center justify-center">
							<Music class="size-20 text-muted-foreground" strokeWidth={1} />
						</div>
					{/if}
				</div>

				<!-- Track info + controls -->
				<div class="relative flex flex-col flex-1 min-w-0 pt-2">
					<!-- Mode toggle (top-right) -->
					<div class="absolute top-2 right-0 flex gap-1">
						<Button
							variant={preferredMode === 'artwork' ? 'default' : 'ghost'}
							size="icon"
							class="size-7"
							onclick={() => preferredMode = 'artwork'}
						>
							<Image class="size-3.5" />
						</Button>
						<Button
							variant={preferredMode === 'video' ? 'default' : 'ghost'}
							size="icon"
							class="size-7"
							onclick={() => preferredMode = 'video'}
						>
							<Film class="size-3.5" />
						</Button>
						<Button
							variant={preferredMode === 'lyrics' ? 'default' : 'ghost'}
							size="icon"
							class="size-7"
							onclick={() => preferredMode = 'lyrics'}
						>
							<Type class="size-3.5" />
						</Button>
					</div>

					<p class="text-xs font-semibold uppercase tracking-wider text-primary mb-2">Now Playing</p>
					<h1 class="text-3xl lg:text-4xl font-bold tracking-tight truncate">
						{player.currentTrack?.title}
					</h1>
					<p class="text-lg text-muted-foreground mt-1 truncate">
						{player.currentTrack?.artist_name ?? 'Unknown Artist'}
					</p>
					{#if player.currentTrack?.album_title}
						<p class="text-sm text-muted-foreground/70 mt-0.5 truncate">
							{player.currentTrack.album_title}
						</p>
					{/if}

					<a
						href="/library/songs/{player.currentTrack?.id}"
						class="inline-flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors mt-2 w-fit"
					>
						<Info class="size-3.5" />
						Song Details
					</a>

					<!-- Playback controls -->
					<div class="flex flex-col gap-3 mt-8 max-w-md">
						<!-- Progress bar -->
						<div class="flex items-center gap-3">
							<span class="text-xs text-muted-foreground w-10 text-right tabular-nums">
								{formatDuration(player.positionMs)}
							</span>
							<Slider
								type="single"
								value={progressPercent}
								max={100}
								step={0.1}
								class="flex-1"
								onValueCommit={handleProgressChange}
							/>
							<span class="text-xs text-muted-foreground w-10 tabular-nums">
								{formatDuration(player.durationMs)}
							</span>
						</div>

						<!-- Control buttons -->
						<div class="flex items-center gap-3 justify-center">
							<Button
								variant="ghost"
								size="icon"
								class={player.shuffle ? 'text-primary hover:text-primary' : 'text-muted-foreground hover:text-foreground'}
								onclick={() => player.toggleShuffle()}
							>
								<Shuffle class="size-5" />
							</Button>
							<Button
								variant="ghost"
								size="icon"
								class="text-foreground hover:text-foreground"
								onclick={() => player.prev()}
							>
								<SkipBack class="size-5" fill="currentColor" />
							</Button>
							<Button
								variant="default"
								size="icon"
								class="rounded-full size-12"
								onclick={() => player.togglePlayPause()}
							>
								{#if player.isPlaying}
									<Pause class="size-6" fill="currentColor" />
								{:else}
									<Play class="size-6" fill="currentColor" />
								{/if}
							</Button>
							<Button
								variant="ghost"
								size="icon"
								class="text-foreground hover:text-foreground"
								onclick={() => player.next()}
							>
								<SkipForward class="size-5" fill="currentColor" />
							</Button>
							<Button
								variant="ghost"
								size="icon"
								class={player.repeat !== 'off' ? 'text-primary hover:text-primary' : 'text-muted-foreground hover:text-foreground'}
								onclick={() => player.cycleRepeat()}
							>
								<RepeatIcon class="size-5" />
							</Button>
						</div>
					</div>
				</div>
			</div>

			<!-- Lyrics / Video panel (below hero) -->
			{#if displayMode === 'video' && fullTrack?.music_video_path}
				<div class="rounded-xl border border-border bg-black overflow-hidden">
					<!-- svelte-ignore a11y_media_has_caption -->
					<video
						src={assetUrl(fullTrack.music_video_path)}
						controls
						autoplay
						class="w-full max-h-[28rem] object-contain"
					></video>
				</div>
			{:else if displayMode === 'lyrics' && fullTrack?.lyrics}
				<div class="rounded-xl border border-border/60 bg-card/60 backdrop-blur-sm overflow-hidden">
					<div class="h-[24rem] overflow-hidden">
						<SyncedLyrics lyrics={fullTrack.lyrics} positionMs={player.positionMs} />
					</div>
				</div>
			{/if}

			<!-- Previously Played -->
			{#if player.previousTrack}
				<div>
					<div class="flex items-center gap-2 mb-3">
						<History class="size-4 text-muted-foreground" />
						<h2 class="text-lg font-semibold">Previously Played</h2>
					</div>
					<button
						class="w-full flex items-center gap-4 p-3 rounded-lg border border-border bg-card/50 hover:bg-muted/50 transition-colors text-left group"
						onclick={() => player.prev()}
					>
						<div class="size-12 shrink-0 rounded-md bg-muted overflow-hidden">
							{#if player.previousTrack.cover_art_path}
								<img
									src={assetUrl(player.previousTrack.cover_art_path)}
									alt=""
									class="size-full object-cover"
									loading="lazy"
								/>
							{:else}
								<div class="size-full flex items-center justify-center">
									<Music class="size-5 text-muted-foreground" />
								</div>
							{/if}
						</div>
						<div class="min-w-0 flex-1">
							<p class="text-sm font-medium truncate text-muted-foreground group-hover:text-foreground transition-colors">
								{player.previousTrack.title}
							</p>
							<p class="text-xs text-muted-foreground/60 truncate">
								{player.previousTrack.artist_name ?? 'Unknown Artist'}
								{#if player.previousTrack.album_title}
									&middot; {player.previousTrack.album_title}
								{/if}
							</p>
						</div>
						<div class="shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
							<SkipBack class="size-4 text-muted-foreground" />
						</div>
					</button>
				</div>
			{/if}

			<!-- Up Next Section -->
			<div>
				<div class="flex items-center justify-between mb-3">
					<h2 class="text-lg font-semibold">Up Next</h2>
					{#if hasUpNext}
						<Button
							variant="ghost"
							size="sm"
							class="text-muted-foreground hover:text-foreground gap-1.5"
							onclick={() => player.clearQueue()}
						>
							<Trash2 class="size-3.5" />
							Clear
						</Button>
					{/if}
				</div>

				<div class="rounded-lg border border-border bg-card/50 p-3">
					<DndQueueList />
				</div>
			</div>
		</div>
	{/if}
</div>
