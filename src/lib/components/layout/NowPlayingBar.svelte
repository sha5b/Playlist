<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Slider } from '$lib/components/ui/slider';
	import {
		Shuffle, SkipBack, Play, Pause, SkipForward,
		Repeat, Repeat1, Volume2, VolumeX, Volume1,
		ListMusic, Music, Infinity
	} from 'lucide-svelte';
	import { goto } from '$app/navigation';
	import { player } from '$lib/stores/player.svelte';
	import { formatDuration } from '$lib/utils/format';
	import CoverArt from '$lib/components/shared/CoverArt.svelte';
	import { processQueueDrop, handleQueueDragOver } from '$lib/utils/queueDrop';

	// While dragging, show the drag value instead of the live progress so the
	// backend's ~450ms progress events don't snap the thumb back mid-drag.
	let seekDragValue = $state<number | null>(null);
	let seekClearTimeout: ReturnType<typeof setTimeout>;

	function handleProgressChange(value: number) {
		const seconds = (value / 100) * (player.durationMs / 1000);
		player.seek(seconds);
		// Keep showing the committed value briefly to avoid a snap-back flash
		// before the next progress event reflects the seek.
		clearTimeout(seekClearTimeout);
		seekClearTimeout = setTimeout(() => { seekDragValue = null; }, 500);
	}

	let volumeTimeout: ReturnType<typeof setTimeout>;
	function handleVolumeChange(value: number) {
		clearTimeout(volumeTimeout);
		volumeTimeout = setTimeout(() => player.setVolume(value / 100), 16);
	}

	// Remember the volume before muting so unmute restores it (not a hardcoded value)
	let volumeBeforeMute = 0.75;
	function toggleMute() {
		if (player.volume === 0) {
			player.setVolume(volumeBeforeMute > 0 ? volumeBeforeMute : 0.75);
		} else {
			volumeBeforeMute = player.volume;
			player.setVolume(0);
		}
	}

	const progressPercent = $derived(
		player.durationMs > 0 ? (player.positionMs / player.durationMs) * 100 : 0
	);

	const VolumeIcon = $derived(
		player.volume === 0 ? VolumeX : player.volume < 0.5 ? Volume1 : Volume2
	);

	const RepeatIcon = $derived(
		player.repeat === 'one' ? Repeat1 : Repeat
	);

	const hasTrack = $derived(player.currentTrack !== null);

	function handlePlayPause() {
		if (hasTrack) {
			player.togglePlayPause();
		} else {
			player.playRandom();
		}
	}

	// Drop zone on the queue button area
	let queueDragOver = $state(false);

	function onQueueDragOver(e: DragEvent) {
		if (handleQueueDragOver(e)) {
			queueDragOver = true;
			if (!player.queueOpen) player.toggleQueuePanel();
		}
	}

	async function onQueueDrop(e: DragEvent) {
		queueDragOver = false;
		e.preventDefault();
		await processQueueDrop(e.dataTransfer);
	}
</script>

<footer class="flex h-20 items-center border-t border-border bg-card px-4 gap-4">
	<!-- Left: Track info (click to open Playing page) -->
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<div
		role={hasTrack ? 'button' : undefined}
		tabindex={hasTrack ? 0 : undefined}
		class="flex items-center gap-3 w-[200px] lg:w-[280px] min-w-0 rounded-lg -m-1 p-1 transition-colors {hasTrack ? 'cursor-pointer hover:bg-muted/30' : ''}"
		onclick={() => { if (hasTrack) goto('/playing'); }}
		onkeydown={(e) => { if (hasTrack && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); goto('/playing'); } }}
	>
		<div class="size-12 shrink-0 rounded-md bg-muted flex items-center justify-center overflow-hidden">
			<CoverArt
				src={player.currentTrack?.cover_art_path}
				alt=""
				class="size-full object-cover"
				iconClass="size-6 text-muted-foreground"
				icon={Music}
			/>
		</div>
		<div class="min-w-0">
			<p class="text-sm font-medium truncate text-foreground">
				{player.currentTrack?.title ?? 'No track playing'}
			</p>
			<p class="text-xs text-muted-foreground truncate">
				{player.currentTrack?.artist_name ?? '--'}
			</p>
		</div>
	</div>

	<!-- Center: Controls + Progress -->
	<div class="flex flex-1 flex-col items-center gap-1 max-w-[600px] mx-auto">
		<div class="flex items-center gap-2">
			<Button
				variant="ghost"
				size="icon-sm"
				class={player.shuffle ? 'text-primary hover:text-primary' : 'text-muted-foreground hover:text-foreground'}
				onclick={() => player.toggleShuffle()}
				aria-label="Toggle shuffle"
			>
				<Shuffle class="size-4" />
			</Button>
			<Button
				variant="ghost"
				size="icon-sm"
				class="text-muted-foreground hover:text-foreground"
				onclick={() => player.prev()}
				disabled={!hasTrack}
				aria-label="Previous track"
			>
				<SkipBack class="size-4" />
			</Button>
			<Button
				variant="default"
				size="icon"
				class="rounded-full"
				onclick={handlePlayPause}
				aria-label={player.isPlaying ? 'Pause' : 'Play'}
			>
				{#if player.isPlaying}
					<Pause class="size-5" fill="currentColor" />
				{:else}
					<Play class="size-5" fill="currentColor" />
				{/if}
			</Button>
			<Button
				variant="ghost"
				size="icon-sm"
				class="text-muted-foreground hover:text-foreground"
				onclick={() => player.next()}
				disabled={!hasTrack}
				aria-label="Next track"
			>
				<SkipForward class="size-4" />
			</Button>
			<Button
				variant="ghost"
				size="icon-sm"
				class={player.repeat !== 'off' ? 'text-primary hover:text-primary' : 'text-muted-foreground hover:text-foreground'}
				onclick={() => player.cycleRepeat()}
				disabled={!hasTrack}
				aria-label="Cycle repeat mode"
			>
				<RepeatIcon class="size-4" />
			</Button>
			<Button
				variant="ghost"
				size="icon-sm"
				class={player.autoplay ? 'text-primary hover:text-primary' : 'text-muted-foreground hover:text-foreground'}
				onclick={() => player.toggleAutoplay()}
				aria-label="Toggle autoplay"
				title="Autoplay: add random tracks when queue ends"
			>
				<Infinity class="size-4" />
			</Button>
		</div>
		<div class="flex w-full items-center gap-2 px-2">
			<span class="text-[11px] text-muted-foreground w-10 text-right tabular-nums">
				{formatDuration(player.positionMs)}
			</span>
			<Slider
				type="single"
				value={seekDragValue ?? progressPercent}
				max={100}
				step={0.1}
				class="flex-1"
				disabled={!hasTrack}
				onValueChange={(v: number) => { seekDragValue = v; }}
				onValueCommit={handleProgressChange}
			/>
			<span class="text-[11px] text-muted-foreground w-10 tabular-nums">
				{formatDuration(player.durationMs)}
			</span>
		</div>
	</div>

	<!-- Right: Volume + Queue -->
	<div class="flex items-center gap-2 w-[160px] lg:w-[200px] justify-end">
		<Button
			variant="ghost"
			size="icon-sm"
			class="text-muted-foreground hover:text-foreground"
			onclick={toggleMute}
			aria-label={player.volume === 0 ? 'Unmute' : 'Mute'}
		>
			<VolumeIcon class="size-4" />
		</Button>
		<Slider
			type="single"
			value={player.volume * 100}
			max={100}
			step={1}
			class="w-20 lg:w-24"
			onValueChange={handleVolumeChange}
		/>
		<div
			role="presentation"
			class="rounded-md transition-colors {queueDragOver ? 'ring-2 ring-primary bg-primary/10' : ''}"
			ondragover={onQueueDragOver}
			ondragleave={() => { queueDragOver = false; }}
			ondrop={onQueueDrop}
		>
			<Button
				variant="ghost"
				size="icon-sm"
				class={player.queueOpen ? 'text-primary hover:text-primary' : 'text-muted-foreground hover:text-foreground'}
				onclick={() => player.toggleQueuePanel()}
				aria-label="Toggle queue"
			>
				<ListMusic class="size-4" />
			</Button>
		</div>
	</div>
</footer>
