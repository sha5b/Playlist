<script lang="ts">
	import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Slider } from '$lib/components/ui/slider';
	import { FolderOpen, Volume2, RotateCcw } from 'lucide-svelte';
	import { getSetting, setSetting } from '$lib/api/library';
	import { player } from '$lib/stores/player.svelte';
	import { open } from '@tauri-apps/plugin-dialog';
	import { toast } from 'svelte-sonner';

	let downloadDir = $state('');
	let defaultVolume = $state(75);
	let loading = $state(true);

	async function load() {
		loading = true;
		try {
			const dir = await getSetting('download_dir');
			if (dir) downloadDir = dir;
			const vol = await getSetting('default_volume');
			if (vol) defaultVolume = Math.round(parseFloat(vol) * 100);
		} catch (e) {
			console.error('Failed to load settings:', e);
		} finally {
			loading = false;
		}
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

	async function resetSettings() {
		downloadDir = '';
		defaultVolume = 75;
		await setSetting('default_volume', '0.75');
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
			<CardDescription>Where downloaded tracks are saved</CardDescription>
		</CardHeader>
		<CardContent>
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

	<div class="flex justify-end">
		<Button variant="outline" onclick={resetSettings}>
			<RotateCcw class="size-4" />
			Reset to Defaults
		</Button>
	</div>
</div>
