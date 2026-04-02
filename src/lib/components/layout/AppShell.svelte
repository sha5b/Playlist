<script lang="ts">
	import Sidebar from './Sidebar.svelte';
	import NowPlayingBar from './NowPlayingBar.svelte';
	import QueuePanel from './QueuePanel.svelte';
	import DebugConsole from './DebugConsole.svelte';
	import { player } from '$lib/stores/player.svelte';
	import { depsStore } from '$lib/stores/deps.svelte';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { downloadStore } from '$lib/stores/downloads.svelte';
	import { metadataScanStore } from '$lib/stores/metadataScan.svelte';

	let { children } = $props();

	let debugOpen = $state(false);

	$effect(() => {
		player.init();
		depsStore.init();
		libraryStore.init();
		downloadStore.init();
		metadataScanStore.init();
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
		<main class="flex-1 flex flex-col min-h-0 overflow-hidden p-6">
			{@render children()}
		</main>
		<QueuePanel />
	</div>
	<NowPlayingBar />
	<DebugConsole bind:open={debugOpen} />
</div>
