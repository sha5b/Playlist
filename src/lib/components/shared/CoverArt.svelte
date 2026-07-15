<script lang="ts">
	import { Disc } from 'lucide-svelte';
	import { assetUrl } from '$lib/utils/format';

	interface Props {
		/** Local file path (converted via the asset protocol) or, when `remote`, a URL. */
		src?: string | null;
		alt?: string;
		/** Classes applied to BOTH the <img> and the fallback wrapper so sizing matches. */
		class?: string;
		/** Classes for the fallback icon. */
		iconClass?: string;
		/** Fallback icon component (defaults to a disc). Any lucide-svelte icon. */
		icon?: typeof Disc;
		/** If true, `src` is already a URL and is used as-is (not run through assetUrl). */
		remote?: boolean;
	}

	let {
		src = null,
		alt = '',
		class: klass = '',
		iconClass = 'size-6 text-muted-foreground/40',
		icon: Icon = Disc,
		remote = false
	}: Props = $props();

	let errored = $state(false);
	// Reset the error flag whenever the source changes (e.g. list reused across items).
	$effect(() => {
		void src;
		errored = false;
	});
	const resolved = $derived(src ? (remote ? src : assetUrl(src)) : null);
</script>

{#if resolved && !errored}
	<img src={resolved} {alt} class={klass} loading="lazy" onerror={() => (errored = true)} />
{:else}
	<div class="{klass} flex items-center justify-center bg-muted">
		<Icon class={iconClass} />
	</div>
{/if}
