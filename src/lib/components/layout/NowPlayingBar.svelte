<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Slider } from '$lib/components/ui/slider';
	import {
		Shuffle, SkipBack, Play, Pause, SkipForward,
		Repeat, Repeat1, Volume2, VolumeX, Volume1,
		ListMusic, Music
	} from 'lucide-svelte';
	import { player } from '$lib/stores/player.svelte';
	import { formatDuration } from '$lib/utils/format';

	function handleProgressChange(values: number[]) {
		const seconds = (values[0] / 100) * (player.durationMs / 1000);
		player.seek(seconds);
	}

	function handleVolumeChange(values: number[]) {
		player.setVolume(values[0] / 100);
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
</script>

<footer class="flex h-20 items-center border-t border-border bg-card px-4 gap-4">
	<!-- Left: Track info -->
	<div class="flex items-center gap-3 w-[280px] min-w-0">
		<div class="size-12 shrink-0 rounded-md bg-muted flex items-center justify-center overflow-hidden">
			{#if player.currentTrack?.cover_art_path}
				<img
					src="https://asset.localhost/{player.currentTrack.cover_art_path}"
					alt=""
					class="size-full object-cover"
				/>
			{:else}
				<Music class="size-6 text-muted-foreground" strokeWidth={1.5} />
			{/if}
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
			>
				<Shuffle class="size-4" />
			</Button>
			<Button
				variant="ghost"
				size="icon-sm"
				class="text-muted-foreground hover:text-foreground"
				onclick={() => player.prev()}
				disabled={!hasTrack}
			>
				<SkipBack class="size-4" />
			</Button>
			<Button
				variant="default"
				size="icon"
				class="rounded-full"
				onclick={() => player.togglePlayPause()}
				disabled={!hasTrack}
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
			>
				<SkipForward class="size-4" />
			</Button>
			<Button
				variant="ghost"
				size="icon-sm"
				class={player.repeat !== 'off' ? 'text-primary hover:text-primary' : 'text-muted-foreground hover:text-foreground'}
				onclick={() => player.cycleRepeat()}
				disabled={!hasTrack}
			>
				<RepeatIcon class="size-4" />
			</Button>
		</div>
		<div class="flex w-full items-center gap-2 px-2">
			<span class="text-[11px] text-muted-foreground w-10 text-right tabular-nums">
				{formatDuration(player.positionMs)}
			</span>
			<Slider
				value={[progressPercent]}
				max={100}
				step={0.1}
				class="flex-1"
				disabled={!hasTrack}
				onValueChange={handleProgressChange}
			/>
			<span class="text-[11px] text-muted-foreground w-10 tabular-nums">
				{formatDuration(player.durationMs)}
			</span>
		</div>
	</div>

	<!-- Right: Volume + Queue -->
	<div class="flex items-center gap-2 w-[200px] justify-end">
		<Button
			variant="ghost"
			size="icon-sm"
			class="text-muted-foreground hover:text-foreground"
			onclick={() => player.setVolume(player.volume === 0 ? 0.75 : 0)}
		>
			<VolumeIcon class="size-4" />
		</Button>
		<Slider
			value={[player.volume * 100]}
			max={100}
			step={1}
			class="w-24"
			onValueChange={handleVolumeChange}
		/>
		<Button
			variant="ghost"
			size="icon-sm"
			class={player.queueOpen ? 'text-primary hover:text-primary' : 'text-muted-foreground hover:text-foreground'}
			onclick={() => player.toggleQueuePanel()}
		>
			<ListMusic class="size-4" />
		</Button>
	</div>
</footer>
