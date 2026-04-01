<script lang="ts">
	import '../app.css';
	import AppShell from '$lib/components/layout/AppShell.svelte';
	import { Toaster } from 'svelte-sonner';

	let { children } = $props();
</script>

<svelte:head>
	<title>Playlist</title>
</svelte:head>

<Toaster
	position="bottom-right"
	richColors
	theme="dark"
	toastOptions={{
		style: 'background: oklch(0.17 0 0); border-color: oklch(0.28 0 0); color: oklch(0.922 0 0);'
	}}
/>

<AppShell {children} />
