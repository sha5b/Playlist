<script lang="ts">
	import { isLrcFormat, parseLrc, findActiveLine, type LrcLine } from '$lib/utils/lrc';

	let { lyrics, positionMs }: { lyrics: string; positionMs: number } = $props();

	const isSynced = $derived(isLrcFormat(lyrics));
	const lines = $derived(isSynced ? parseLrc(lyrics) : []);
	const activeIndex = $derived(isSynced ? findActiveLine(lines, positionMs) : -1);

	let container: HTMLDivElement | undefined = $state();

	$effect(() => {
		if (activeIndex >= 0 && container) {
			const el = container.children[activeIndex] as HTMLElement | undefined;
			el?.scrollIntoView({ behavior: 'smooth', block: 'center' });
		}
	});
</script>

<div
	bind:this={container}
	class="size-full overflow-y-auto rounded-xl bg-black/40 p-6 flex flex-col gap-1"
>
	{#if isSynced}
		{#each lines as line, i}
			<p
				class="transition-all duration-300 leading-relaxed {i === activeIndex
					? 'text-2xl font-bold text-primary'
					: i < activeIndex
						? 'text-base text-muted-foreground/40'
						: 'text-base text-muted-foreground/70'}"
			>
				{line.text || '\u00A0'}
			</p>
		{/each}
	{:else}
		<p class="text-sm text-foreground/80 whitespace-pre-line">{lyrics}</p>
	{/if}
</div>
