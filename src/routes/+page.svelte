<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { greet, getLibraryStats } from '$lib/api/library';
	import type { LibraryStats } from '$lib/types';

	let greeting = $state('');
	let stats: LibraryStats | null = $state(null);
	let error = $state('');

	async function handleGreet() {
		try {
			greeting = await greet('User');
		} catch (e) {
			error = String(e);
		}
	}

	async function loadStats() {
		try {
			stats = await getLibraryStats();
		} catch (e) {
			error = String(e);
		}
	}

	$effect(() => {
		loadStats();
	});
</script>

<div class="space-y-6">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Home</h1>
		<p class="text-muted-foreground mt-1">Welcome to Playlist</p>
	</div>

	<div class="grid grid-cols-2 gap-4 md:grid-cols-4">
		<Card>
			<CardHeader class="pb-2">
				<CardDescription>Tracks</CardDescription>
				<CardTitle class="text-2xl tabular-nums">{stats?.total_tracks ?? '...'}</CardTitle>
			</CardHeader>
		</Card>
		<Card>
			<CardHeader class="pb-2">
				<CardDescription>Albums</CardDescription>
				<CardTitle class="text-2xl tabular-nums">{stats?.total_albums ?? '...'}</CardTitle>
			</CardHeader>
		</Card>
		<Card>
			<CardHeader class="pb-2">
				<CardDescription>Artists</CardDescription>
				<CardTitle class="text-2xl tabular-nums">{stats?.total_artists ?? '...'}</CardTitle>
			</CardHeader>
		</Card>
		<Card>
			<CardHeader class="pb-2">
				<CardDescription>Playlists</CardDescription>
				<CardTitle class="text-2xl tabular-nums">{stats?.total_playlists ?? '...'}</CardTitle>
			</CardHeader>
		</Card>
	</div>

	<Card>
		<CardHeader>
			<CardTitle>Getting Started</CardTitle>
			<CardDescription>Your library is empty. Import some music to get started.</CardDescription>
		</CardHeader>
		<CardContent class="flex gap-3">
			<Button onclick={handleGreet}>
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><path d="M12 5v14"/><path d="M5 12h14"/></svg>
				Test Connection
			</Button>
		</CardContent>
	</Card>

	{#if greeting}
		<Card>
			<CardContent class="pt-6">
				<p class="text-sm text-primary font-medium">{greeting}</p>
			</CardContent>
		</Card>
	{/if}

	{#if error}
		<Card class="border-destructive">
			<CardContent class="pt-6">
				<p class="text-sm text-destructive">{error}</p>
			</CardContent>
		</Card>
	{/if}
</div>
