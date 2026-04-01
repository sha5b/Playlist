<script lang="ts">
	import Sidebar from './Sidebar.svelte';
	import NowPlayingBar from './NowPlayingBar.svelte';
	import { ScrollArea } from '$lib/components/ui/scroll-area';

	let { children } = $props();
</script>

<div class="flex h-screen w-screen flex-col">
	<div class="flex flex-1 min-h-0">
		<Sidebar />
		<main class="flex-1 overflow-y-auto">
			<div class="p-6">
				{@render children()}
			</div>
		</main>
	</div>
	<NowPlayingBar />
</div>
