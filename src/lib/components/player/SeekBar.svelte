<script lang="ts">
	import { Slider } from '$lib/components/ui/slider';
	import { player } from '$lib/stores/player.svelte';

	let { disabled = false, class: className = '' }: { disabled?: boolean; class?: string } = $props();

	// bits-ui's slider snaps out-of-step values programmatically and that snap
	// fires onValueChange too. Only treat onValueChange as a user seek while a
	// pointer interaction is actually in progress — otherwise the snap events
	// would freeze the bar at a stale value (e.g. showing the previous track's
	// position after a track change).
	let isSeeking = $state(false);
	// While seeking (and briefly after commit), show the drag value instead of
	// live progress so the backend's ~450ms progress events don't snap the
	// thumb back mid-drag.
	let heldValue = $state<number | null>(null);
	let clearTimer: ReturnType<typeof setTimeout>;

	const progressPercent = $derived(
		player.durationMs > 0 ? Math.min(100, (player.positionMs / player.durationMs) * 100) : 0
	);

	// Drop any held value when the track changes so the bar starts at 0.
	let lastTrackId: number | null = null;
	$effect(() => {
		const id = player.currentTrack?.id ?? null;
		if (id !== lastTrackId) {
			lastTrackId = id;
			isSeeking = false;
			heldValue = null;
		}
	});

	function handleValueChange(v: number) {
		if (isSeeking) heldValue = v;
	}

	function handleValueCommit(v: number) {
		isSeeking = false;
		if (disabled || player.durationMs <= 0) {
			heldValue = null;
			return;
		}
		heldValue = v;
		const seconds = (v / 100) * (player.durationMs / 1000);
		player.seek(seconds);
		// Keep showing the committed value briefly to avoid a snap-back flash
		// before the next progress event reflects the seek.
		clearTimeout(clearTimer);
		clearTimer = setTimeout(() => {
			heldValue = null;
		}, 600);
	}
</script>

<!-- Pointer-down anywhere on the bar starts a seek: bits-ui moves the thumb to
     the pointer immediately and commits on pointer-up, so a plain click jumps
     playback there while dragging keeps working. The py padding enlarges the
     click target beyond the 4px track. -->
<div
	role="presentation"
	class="flex min-w-0 flex-1 items-center {className}"
	onpointerdown={() => {
		if (!disabled) isSeeking = true;
	}}
>
	<Slider
		type="single"
		value={heldValue ?? progressPercent}
		max={100}
		step={0.1}
		{disabled}
		class="flex-1 cursor-pointer py-2"
		onValueChange={handleValueChange}
		onValueCommit={handleValueCommit}
	/>
</div>
