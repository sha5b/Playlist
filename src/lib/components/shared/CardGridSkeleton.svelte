<script lang="ts">
	import Skeleton from './Skeleton.svelte';

	let {
		count = 12,
		rounded = 'lg',
	}: {
		count?: number;
		rounded?: 'lg' | 'full';
	} = $props();
</script>

<div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
	{#each Array(count) as _}
		<div>
			<Skeleton class="aspect-square mb-2 {rounded === 'full' ? 'rounded-full' : 'rounded-lg'}" />
			<Skeleton class="h-4 w-3/4 mb-1" />
			<Skeleton class="h-3 w-1/2" />
		</div>
	{/each}
</div>
