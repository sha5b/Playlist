<script lang="ts">
	import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Separator } from '$lib/components/ui/separator';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Slider } from '$lib/components/ui/slider';
	import { FolderOpen, Volume2, RotateCcw, Music, Trash2, Cookie, Sparkles, Loader2, CircleX, Speaker } from 'lucide-svelte';
	import { getSetting, setSetting, resetLibrary, getMetadataStats, scanMissingMetadata, stopMetadataScan, deleteAllMetadata } from '$lib/api/library';
	import { getAudioDevices, setAudioDevice } from '$lib/api/player';
	import type { MetadataStats } from '$lib/types';
	import { player } from '$lib/stores/player.svelte';
	import { metadataScanStore } from '$lib/stores/metadataScan.svelte';
	import { open } from '@tauri-apps/plugin-dialog';
	import { toast } from 'svelte-sonner';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { downloadStore } from '$lib/stores/downloads.svelte';

	let resetLibraryOpen = $state(false);
	let deleteMetadataOpen = $state(false);

	let downloadDir = $state('');
	let downloadFormat = $state('mp3');
	let cookiesBrowser = $state('chrome');
	let defaultVolume = $state(75);
	let autoDownloadMV = $state(false);
	let audioDevices = $state<[string, boolean][]>([]);
	let selectedDevice = $state<string>('');

	const formatOptions = ['mp3', 'opus', 'flac', 'm4a'] as const;
	const browserOptions = [
		{ value: '', label: 'None' },
		{ value: 'chrome', label: 'Chrome' },
		{ value: 'firefox', label: 'Firefox' },
		{ value: 'edge', label: 'Edge' },
		{ value: 'brave', label: 'Brave' },
		{ value: 'opera', label: 'Opera' },
		{ value: 'vivaldi', label: 'Vivaldi' },
	] as const;

	async function load() {
		try {
			const dir = await getSetting('download_dir');
			if (dir) downloadDir = dir;
			const fmt = await getSetting('download_format');
			if (fmt) downloadFormat = fmt;
			const cookies = await getSetting('cookies_from_browser');
			if (cookies) cookiesBrowser = cookies;
			const vol = await getSetting('default_volume');
			if (vol) defaultVolume = Math.round(parseFloat(vol) * 100);
			const mvSetting = await getSetting('auto_download_music_videos');
			autoDownloadMV = mvSetting === 'true';
		} catch (e) {
			console.error('Failed to load settings:', e);
		}
	}

	async function setFormat(fmt: string) {
		downloadFormat = fmt;
		await setSetting('download_format', fmt);
		toast.success(`Download format set to ${fmt.toUpperCase()}`);
	}

	async function setCookiesBrowser(browser: string) {
		cookiesBrowser = browser;
		await setSetting('cookies_from_browser', browser);
		toast.success(browser ? `Using cookies from ${browser}` : 'Browser cookies disabled');
	}

	async function chooseDownloadDir() {
		const selected = await open({ directory: true, title: 'Choose download folder' });
		if (selected) {
			downloadDir = selected as string;
			await setSetting('download_dir', downloadDir);
			toast.success('Download folder updated');
		}
	}

	async function handleVolumeChange(value: number) {
		defaultVolume = value;
		const vol = value / 100;
		await setSetting('default_volume', String(vol));
		player.setVolume(vol);
	}

	async function loadAudioDevices() {
		try {
			audioDevices = await getAudioDevices();
			const saved = await getSetting('audio_device');
			selectedDevice = saved || '';
		} catch (e) {
			console.error('Failed to load audio devices:', e);
		}
	}

	async function handleDeviceChange(deviceName: string) {
		selectedDevice = deviceName;
		await setAudioDevice(deviceName || null);
		await setSetting('audio_device', deviceName);
		toast.success(deviceName ? `Audio output: ${deviceName}` : 'Using system default audio device');
	}

	let metadataStats = $state<MetadataStats | null>(null);
	let scanning = $derived(metadataScanStore.scanning);
	let scanProgress = $derived(metadataScanStore.progress);

	async function loadMetadataStats() {
		try {
			metadataStats = await getMetadataStats();
		} catch (e) {
			console.error('Failed to load metadata stats:', e);
		}
	}

	async function handleScanMetadata() {
		if (metadataScanStore.scanning) return;
		metadataScanStore.markScanning();

		try {
			const result = await scanMissingMetadata();
			toast.success(`Metadata scan complete`, {
				description: `${result.enriched} tracks enriched, ${result.failed} failed. Average completeness: ${result.completeness_avg}%`
			});
			await loadMetadataStats();
		} catch (e) {
			toast.error('Metadata scan failed', { description: String(e) });
		} finally {
			metadataScanStore.markDone();
		}
	}

	let deletingMetadata = $state(false);

	async function handleStopMetadata() {
		try {
			await stopMetadataScan();
			toast.success('Metadata scan stopped');
		} catch (e) {
			toast.error('Failed to stop scan', { description: String(e) });
		}
	}

	async function handleDeleteMetadata() {
		deletingMetadata = true;
		try {
			await deleteAllMetadata();
			toast.success('All metadata deleted', { description: 'Any running scan was stopped. Enriched data has been cleared from all tracks, albums, and artists' });
			await loadMetadataStats();
		} catch (e) {
			toast.error('Failed to delete metadata', { description: String(e) });
		} finally {
			deletingMetadata = false;
		}
	}

	let resettingLibrary = $state(false);

	async function handleResetLibrary() {
		resettingLibrary = true;
		try {
			await resetLibrary(true);
			downloadStore.reset();
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
		cookiesBrowser = 'chrome';
		defaultVolume = 75;
		await setSetting('default_volume', '0.75');
		await setSetting('download_format', 'mp3');
		await setSetting('cookies_from_browser', 'chrome');
		player.setVolume(0.75);
		toast.success('Settings reset to defaults');
	}

	$effect(() => {
		load();
		loadMetadataStats();
		loadAudioDevices();
	});
</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Settings</h1>
		<p class="text-muted-foreground mt-1">Configure your preferences</p>
	</div>

	<Card>
		<CardHeader>
			<CardTitle>Downloads</CardTitle>
			<CardDescription>Download folder and audio format. Playlists are downloaded via YouTube (no login required)</CardDescription>
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
			<div class="space-y-2">
				<label class="text-sm font-medium flex items-center gap-2">
					<Cookie class="size-4 text-muted-foreground" />
					Browser Cookies
				</label>
				<div class="flex gap-2 flex-wrap">
					{#each browserOptions as opt}
						<Button
							variant={cookiesBrowser === opt.value ? 'default' : 'outline'}
							size="sm"
							onclick={() => setCookiesBrowser(opt.value)}
						>
							{opt.label}
						</Button>
					{/each}
				</div>
				<p class="text-xs text-muted-foreground">
					Use cookies from your browser to bypass YouTube's bot detection. Pick the browser where you're logged in to YouTube.
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
					type="single"
					value={defaultVolume}
					max={100}
					step={1}
					onValueChange={handleVolumeChange}
				/>
			</div>
			<div class="space-y-2">
				<label class="text-sm font-medium flex items-center gap-2">
					<Speaker class="size-4 text-muted-foreground" />
					Audio Output Device
				</label>
				<select
					class="w-full rounded-md border bg-background px-3 py-2 text-sm"
					value={selectedDevice}
					onchange={(e) => handleDeviceChange(e.currentTarget.value)}
				>
					<option value="">System Default</option>
					{#each audioDevices as [name, isDefault]}
						<option value={name}>{name}{isDefault ? ' (Default)' : ''}</option>
					{/each}
				</select>
				<p class="text-xs text-muted-foreground">
					Select which audio device to use for playback. Change takes effect immediately.
				</p>
			</div>
		</CardContent>
	</Card>

	<Card id="metadata">
		<CardHeader>
			<CardTitle>Metadata</CardTitle>
			<CardDescription>Enrich your library with MusicBrainz data</CardDescription>
		</CardHeader>
		<CardContent class="space-y-4">
			{#if metadataStats}
				<div class="space-y-3">
					<div class="flex items-center justify-between">
						<span class="text-sm text-muted-foreground">Average Completeness</span>
						<span class="text-sm font-medium tabular-nums {metadataStats.average_completeness >= 80 ? 'text-green-500' : metadataStats.average_completeness >= 50 ? 'text-yellow-500' : 'text-red-500'}">
							{metadataStats.average_completeness}%
						</span>
					</div>
					<div class="h-2 rounded-full bg-muted overflow-hidden">
						<div
							class="h-full rounded-full transition-all duration-500 {metadataStats.average_completeness >= 80 ? 'bg-green-500' : metadataStats.average_completeness >= 50 ? 'bg-yellow-500' : 'bg-red-500'}"
							style="width: {metadataStats.average_completeness}%"
						></div>
					</div>
					<div class="grid grid-cols-3 gap-4 text-center">
						<div>
							<p class="text-2xl font-bold tabular-nums">{metadataStats.total_tracks}</p>
							<p class="text-xs text-muted-foreground">Total Tracks</p>
						</div>
						<div>
							<p class="text-2xl font-bold tabular-nums text-green-500">{metadataStats.complete_tracks}</p>
							<p class="text-xs text-muted-foreground">Complete (80%+)</p>
						</div>
						<div>
							<p class="text-2xl font-bold tabular-nums text-red-500">{metadataStats.incomplete_tracks}</p>
							<p class="text-xs text-muted-foreground">Incomplete (&lt;50%)</p>
						</div>
					</div>
				</div>
			{/if}

			{#if scanning && scanProgress}
				<div class="space-y-2 rounded-md bg-muted/50 p-3">
					<div class="flex items-center justify-between text-sm">
						<span class="text-muted-foreground">Scanning...</span>
						<span class="tabular-nums font-medium">{scanProgress.current}/{scanProgress.total}</span>
					</div>
					<div class="h-1.5 rounded-full bg-muted overflow-hidden">
						<div class="h-full rounded-full bg-primary transition-all" style="width: {(scanProgress.current / scanProgress.total) * 100}%"></div>
					</div>
					<p class="text-xs text-muted-foreground truncate">Enriching: {scanProgress.track_title}</p>
				</div>
			{/if}

			<div class="flex items-center justify-between gap-4">
				<div>
					<p class="text-sm font-medium">Auto-download Music Videos</p>
					<p class="text-xs text-muted-foreground">Download music videos automatically during metadata enrichment</p>
				</div>
				<Button
					variant={autoDownloadMV ? 'default' : 'outline'}
					size="sm"
					onclick={async () => { autoDownloadMV = !autoDownloadMV; await setSetting('auto_download_music_videos', autoDownloadMV ? 'true' : 'false'); }}
				>
					{autoDownloadMV ? 'On' : 'Off'}
				</Button>
			</div>

			<div class="flex items-center justify-between gap-4">
				<div>
					<p class="text-sm font-medium">Scan for Missing Metadata</p>
					<p class="text-xs text-muted-foreground">Looks up all tracks with incomplete data on MusicBrainz and Last.fm</p>
				</div>
				<div class="flex gap-2">
					<Button variant="outline" size="sm" onclick={handleScanMetadata} disabled={scanning}>
						{#if scanning}
							<Loader2 class="size-4 animate-spin" />
							Scanning...
						{:else}
							<Sparkles class="size-4" />
							Scan
						{/if}
					</Button>
					<Button variant="outline" size="sm" onclick={handleStopMetadata} disabled={!scanning}>
						<CircleX class="size-4" />
						Stop
					</Button>
				</div>
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
					<p class="text-sm font-medium">Delete All Metadata</p>
					<p class="text-xs text-muted-foreground">Automatically stops any running scan, then clears all enriched metadata from tracks, albums, and artists</p>
				</div>
				<AlertDialog.Root bind:open={deleteMetadataOpen}>
					<AlertDialog.Trigger>
						{#snippet child({ props })}
							<Button variant="destructive" size="sm" disabled={deletingMetadata} {...props}>
								{#if deletingMetadata}
									<Loader2 class="size-4 animate-spin" />
									Deleting...
								{:else}
									<Trash2 class="size-4" />
									Delete Metadata
								{/if}
							</Button>
						{/snippet}
					</AlertDialog.Trigger>
					<AlertDialog.Content>
						<AlertDialog.Header>
							<AlertDialog.Title>Delete All Metadata</AlertDialog.Title>
							<AlertDialog.Description>
								This will stop any running scans and clear all enriched metadata from tracks, albums, and artists. This cannot be undone.
							</AlertDialog.Description>
						</AlertDialog.Header>
						<AlertDialog.Footer>
							<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
							<AlertDialog.Action onclick={handleDeleteMetadata}>Delete All Metadata</AlertDialog.Action>
						</AlertDialog.Footer>
					</AlertDialog.Content>
				</AlertDialog.Root>
			</div>
			<Separator />
			<div class="flex items-center justify-between gap-4">
				<div>
					<p class="text-sm font-medium">Reset Library</p>
					<p class="text-xs text-muted-foreground">Delete all tracks, playlists, downloads, and downloaded files</p>
				</div>
				<AlertDialog.Root bind:open={resetLibraryOpen}>
					<AlertDialog.Trigger>
						{#snippet child({ props })}
							<Button variant="destructive" size="sm" disabled={resettingLibrary} {...props}>
								{#if resettingLibrary}
									<RotateCcw class="size-4 animate-spin" />
									Resetting...
								{:else}
									<Trash2 class="size-4" />
									Reset Library
								{/if}
							</Button>
						{/snippet}
					</AlertDialog.Trigger>
					<AlertDialog.Content>
						<AlertDialog.Header>
							<AlertDialog.Title>Reset Library</AlertDialog.Title>
							<AlertDialog.Description>
								This will permanently delete all tracks, playlists, downloads, and downloaded files. This cannot be undone.
							</AlertDialog.Description>
						</AlertDialog.Header>
						<AlertDialog.Footer>
							<AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
							<AlertDialog.Action onclick={handleResetLibrary}>Reset Library</AlertDialog.Action>
						</AlertDialog.Footer>
					</AlertDialog.Content>
				</AlertDialog.Root>
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
				<span>0.2.1</span>
			</div>
			<Separator />
			<div class="flex justify-between text-sm">
				<span class="text-muted-foreground">Stack</span>
				<span>Tauri 2.x + SvelteKit + Svelte 5</span>
			</div>
			<Separator />
			<div class="text-sm text-muted-foreground pt-2">
				<p>Liberate your music from streaming platforms. Own your data, build your library, support artists directly. Buy their albums, back them on Patreon, go to their shows.</p>
			</div>
		</CardContent>
	</Card>
</div>
