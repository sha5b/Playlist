<script lang="ts">
	import '../app.css';
	import AppShell from '$lib/components/layout/AppShell.svelte';

	let { children } = $props();
</script>

<svelte:head>
	<title>Playlist</title>
</svelte:head>

<AppShell {children} />
