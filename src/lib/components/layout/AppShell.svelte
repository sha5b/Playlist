<script lang="ts">
	import Sidebar from './Sidebar.svelte';
	import NowPlayingBar from './NowPlayingBar.svelte';
	import QueuePanel from './QueuePanel.svelte';
	import { player } from '$lib/stores/player.svelte';

	let { children } = $props();

	$effect(() => {
		player.init();
	});
</script>

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
</div>
