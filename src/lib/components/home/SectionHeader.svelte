<script lang="ts">
	import type { Snippet } from 'svelte';
	import { ChevronRight, Disc } from 'lucide-svelte';

	let {
		title,
		icon,
		href,
		hrefLabel = 'Show all',
		actions,
	}: {
		title: string;
		icon: typeof Disc;
		/** Optional "Show all" destination (library page). */
		href?: string;
		hrefLabel?: string;
		/** Extra action buttons rendered on the right, before the link. */
		actions?: Snippet;
	} = $props();

	const Icon = $derived(icon);
</script>

<div class="flex items-center justify-between gap-2">
	<div class="flex items-center gap-2 min-w-0">
		<Icon class="size-4 shrink-0 text-muted-foreground/60" />
		<h2 class="text-sm font-semibold tracking-wide truncate">{title}</h2>
	</div>
	<div class="flex items-center gap-0.5 shrink-0">
		{#if actions}
			{@render actions()}
		{/if}
		{#if href}
			<a
				{href}
				class="group/all flex items-center gap-0.5 rounded-md px-1.5 py-1 text-xs text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors"
			>
				{hrefLabel}
				<ChevronRight class="size-3 transition-transform group-hover/all:translate-x-0.5" />
			</a>
		{/if}
	</div>
</div>
