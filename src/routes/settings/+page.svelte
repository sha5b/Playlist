<script lang="ts">
	import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Separator } from '$lib/components/ui/separator';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Slider } from '$lib/components/ui/slider';
	import { FolderOpen, Volume2, RotateCcw, Music, Trash2 } from 'lucide-svelte';
	import { getSetting, setSetting, resetLibrary } from '$lib/api/library';
	import { player } from '$lib/stores/player.svelte';
	import { open } from '@tauri-apps/plugin-dialog';
	import { toast } from 'svelte-sonner';

	let downloadDir = $state('');
	let downloadFormat = $state('mp3');
	let defaultVolume = $state(75);

	const formatOptions = ['mp3', 'opus', 'flac', 'm4a'] as const;

	async function load() {
		try {
			const dir = await getSetting('download_dir');
			if (dir) downloadDir = dir;
			const fmt = await getSetting('download_format');
			if (fmt) downloadFormat = fmt;
			const vol = await getSetting('default_volume');
			if (vol) defaultVolume = Math.round(parseFloat(vol) * 100);
		} catch (e) {
			console.error('Failed to load settings:', e);
		}
	}

	async function setFormat(fmt: string) {
		downloadFormat = fmt;
		await setSetting('download_format', fmt);
		toast.success(`Download format set to ${fmt.toUpperCase()}`);
	}

	async function chooseDownloadDir() {
		const selected = await open({ directory: true, title: 'Choose download folder' });
		if (selected) {
			downloadDir = selected as string;
			await setSetting('download_dir', downloadDir);
			toast.success('Download folder updated');
		}
	}

	async function handleVolumeChange(values: number[]) {
		defaultVolume = values[0];
		const vol = values[0] / 100;
		await setSetting('default_volume', String(vol));
		player.setVolume(vol);
	}

	let resettingLibrary = $state(false);

	async function handleResetLibrary() {
		resettingLibrary = true;
		try {
			await resetLibrary(true);
			toast.success('Library cleared', { description: 'All tracks, playlists, and downloads have been removed' });
		} catch (e) {
			toast.error('Failed to reset library', { description: String(e) });
		} finally {
			resettingLibrary = false;
		}
	}

	async function resetSettings() {
		downloadDir = '';
		downloadFormat = 'mp3';
		defaultVolume = 75;
		await setSetting('default_volume', '0.75');
		await setSetting('download_format', 'mp3');
		player.setVolume(0.75);
		toast.success('Settings reset to defaults');
	}

	$effect(() => {
		load();
	});
</script>

<div class="space-y-6 max-w-2xl">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Settings</h1>
		<p class="text-muted-foreground mt-1">Configure your preferences</p>
	</div>

	<Card>
		<CardHeader>
			<CardTitle>Downloads</CardTitle>
			<CardDescription>Download folder and audio format</CardDescription>
		</CardHeader>
		<CardContent class="space-y-4">
			<div class="space-y-2">
				<label class="text-sm font-medium flex items-center gap-2">
					<FolderOpen class="size-4 text-muted-foreground" />
					Download Folder
				</label>
				<div class="flex items-center gap-2">
					<Input
						value={downloadDir}
						placeholder="Default download location"
						readonly
						class="flex-1"
					/>
					<Button variant="outline" onclick={chooseDownloadDir}>
						<FolderOpen class="size-4" />
						Browse
					</Button>
				</div>
			</div>
			<div class="space-y-2">
				<label class="text-sm font-medium flex items-center gap-2">
					<Music class="size-4 text-muted-foreground" />
					Audio Format
				</label>
				<div class="flex gap-2">
					{#each formatOptions as fmt}
						<Button
							variant={downloadFormat === fmt ? 'default' : 'outline'}
							size="sm"
							onclick={() => setFormat(fmt)}
						>
							{fmt.toUpperCase()}
						</Button>
					{/each}
				</div>
				<p class="text-xs text-muted-foreground">
					MP3 is most compatible. FLAC is lossless. OPUS has best quality-to-size ratio.
				</p>
			</div>
		</CardContent>
	</Card>

	<Card>
		<CardHeader>
			<CardTitle>Playback</CardTitle>
			<CardDescription>Audio playback settings</CardDescription>
		</CardHeader>
		<CardContent class="space-y-4">
			<div class="space-y-2">
				<div class="flex items-center justify-between">
					<label class="text-sm font-medium flex items-center gap-2">
						<Volume2 class="size-4 text-muted-foreground" />
						Default Volume
					</label>
					<span class="text-sm text-muted-foreground tabular-nums">{defaultVolume}%</span>
				</div>
				<Slider
					value={[defaultVolume]}
					max={100}
					step={1}
					onValueChange={handleVolumeChange}
				/>
			</div>
		</CardContent>
	</Card>

	<Card>
		<CardHeader>
			<CardTitle class="text-destructive">Danger Zone</CardTitle>
			<CardDescription>Irreversible actions</CardDescription>
		</CardHeader>
		<CardContent class="space-y-4">
			<div class="flex items-center justify-between gap-4">
				<div>
					<p class="text-sm font-medium">Reset Library</p>
					<p class="text-xs text-muted-foreground">Delete all tracks, playlists, downloads, and downloaded files</p>
				</div>
				<Button variant="destructive" size="sm" onclick={handleResetLibrary} disabled={resettingLibrary}>
					{#if resettingLibrary}
						<RotateCcw class="size-4 animate-spin" />
						Resetting...
					{:else}
						<Trash2 class="size-4" />
						Reset Library
					{/if}
				</Button>
			</div>
			<Separator />
			<div class="flex items-center justify-between gap-4">
				<div>
					<p class="text-sm font-medium">Reset Settings</p>
					<p class="text-xs text-muted-foreground">Restore all settings to their default values</p>
				</div>
				<Button variant="outline" size="sm" onclick={resetSettings}>
					<RotateCcw class="size-4" />
					Reset Settings
				</Button>
			</div>
		</CardContent>
	</Card>

	<Card>
		<CardHeader>
			<CardTitle>About</CardTitle>
			<CardDescription>Application information</CardDescription>
		</CardHeader>
		<CardContent class="space-y-2">
			<div class="flex justify-between text-sm">
				<span class="text-muted-foreground">Version</span>
				<span>0.1.0</span>
			</div>
			<Separator />
			<div class="flex justify-between text-sm">
				<span class="text-muted-foreground">Stack</span>
				<span>Tauri 2.x + SvelteKit + Svelte 5</span>
			</div>
		</CardContent>
	</Card>
</div>
