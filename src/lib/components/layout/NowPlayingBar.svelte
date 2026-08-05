<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Slider } from '$lib/components/ui/slider';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import {
		Shuffle, SkipBack, Play, Pause, SkipForward,
		Repeat, Repeat1, Volume2, VolumeX, Volume1,
		ListMusic, Music, Infinity, Moon
	} from 'lucide-svelte';
	import { goto } from '$app/navigation';
	import { player, SLEEP_TIMER_OPTIONS, formatSleepRemaining } from '$lib/stores/player.svelte';
	import { formatDuration } from '$lib/utils/format';
	import CoverArt from '$lib/components/shared/CoverArt.svelte';
	import SeekBar from '$lib/components/player/SeekBar.svelte';
	import { processQueueDrop, handleQueueDragOver } from '$lib/utils/queueDrop';

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
				disabled={!hasTrack}
				aria-label="Toggle shuffle"
				title="Shuffle: reorder the upcoming queue randomly"
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
				class={player.endless ? 'text-primary hover:text-primary' : 'text-muted-foreground hover:text-foreground'}
				onclick={() => player.toggleEndless()}
				aria-label="Toggle endless play"
				title="Endless play: keep adding random tracks so the music never stops"
			>
				<Infinity class="size-4" />
			</Button>
			<DropdownMenu.Root>
				<DropdownMenu.Trigger>
					<Button
						variant="ghost"
						size="icon-sm"
						class={player.sleepTimerMode !== 'off' ? 'text-primary hover:text-primary' : 'text-muted-foreground hover:text-foreground'}
						aria-label="Sleep timer"
						title="Sleep timer: fade out and pause after a while"
					>
						<Moon class="size-4" />
					</Button>
				</DropdownMenu.Trigger>
				<DropdownMenu.Content align="end" class="w-40">
					<DropdownMenu.Label>Sleep timer</DropdownMenu.Label>
					{#each SLEEP_TIMER_OPTIONS as option (option.label)}
						<DropdownMenu.Item
							class={player.sleepTimerMode === option.value ? 'text-primary' : ''}
							onclick={() => player.setSleepTimer(option.value)}
						>
							{option.label}
						</DropdownMenu.Item>
					{/each}
				</DropdownMenu.Content>
			</DropdownMenu.Root>
			{#if player.sleepTimerMode !== 'off'}
				<span class="text-[10px] text-primary tabular-nums -ml-1.5 select-none" title="Sleep timer active">
					{formatSleepRemaining(player.sleepTimerMode, player.sleepRemainingMs)}
				</span>
			{/if}
		</div>
		<div class="flex w-full items-center gap-2 px-2">
			<span class="text-[11px] text-muted-foreground w-10 text-right tabular-nums">
				{formatDuration(player.positionMs)}
			</span>
			<SeekBar class="flex-1" disabled={!hasTrack} />
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
