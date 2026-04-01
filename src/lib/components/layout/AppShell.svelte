<script lang="ts">
	import Sidebar from './Sidebar.svelte';
	import NowPlayingBar from './NowPlayingBar.svelte';
	import QueuePanel from './QueuePanel.svelte';
	import DebugConsole from './DebugConsole.svelte';
	import { player } from '$lib/stores/player.svelte';
	import { depsStore } from '$lib/stores/deps.svelte';

	let { children } = $props();

	let debugOpen = $state(false);

	$effect(() => {
		player.init();
		depsStore.init();
	});

	function handleKeydown(e: KeyboardEvent) {
		if (e.ctrlKey && e.shiftKey && e.key === 'D') {
			e.preventDefault();
			debugOpen = !debugOpen;
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex h-screen w-screen flex-col">
	<div class="flex flex-1 min-h-0">
		<Sidebar />
		<main class="flex-1 overflow-y-auto">
			<div class="p-6">
				{@render children()}
			</div>
		</main>
		<QueuePanel />
	</div>
	<NowPlayingBar />
	<DebugConsole bind:open={debugOpen} />
</div>
