<script lang="ts">
	import Titlebar from './Titlebar.svelte';
	import Sidebar from './Sidebar.svelte';
	import NowPlayingBar from './NowPlayingBar.svelte';
	import QueuePanel from './QueuePanel.svelte';
	import DebugConsole from './DebugConsole.svelte';
	import { player } from '$lib/stores/player.svelte';
	import { depsStore } from '$lib/stores/deps.svelte';
	import { CircleX, RefreshCw } from 'lucide-svelte';
	import { Button } from '$lib/components/ui/button';
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
		if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === 'D') {
			e.preventDefault();
			debugOpen = !debugOpen;
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex h-dvh w-dvw flex-col">
	<Titlebar />
	<div class="flex flex-1 min-h-0">
		<Sidebar />
		<main class="flex-1 flex flex-col min-h-0 overflow-hidden p-6">
			{#if depsStore.setupError}
				<div class="bg-destructive/10 border border-destructive/30 text-destructive text-sm rounded-md p-3 mb-4 flex items-center gap-2">
					<CircleX class="size-4 shrink-0" />
					<span class="flex-1">Download tools failed to initialize: {depsStore.setupError}</span>
					<Button variant="outline" size="sm" onclick={() => depsStore.init()}>
						<RefreshCw class="size-3" />
						Retry
					</Button>
				</div>
			{/if}
			{@render children()}
		</main>
		<QueuePanel />
	</div>
	<NowPlayingBar />
	<DebugConsole bind:open={debugOpen} />
</div>
