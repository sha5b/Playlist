<script lang="ts">
	import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Separator } from '$lib/components/ui/separator';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Slider } from '$lib/components/ui/slider';
	import { FolderOpen, Volume2, RotateCcw, Music, Trash2, Cookie, Sparkles, Loader2 } from 'lucide-svelte';
	import { getSetting, setSetting, resetLibrary, getMetadataStats, scanMissingMetadata } from '$lib/api/library';
	import { listen } from '@tauri-apps/api/event';
	import type { MetadataStats, MetadataScanProgress } from '$lib/types';
	import { player } from '$lib/stores/player.svelte';
	import { open } from '@tauri-apps/plugin-dialog';
	import { toast } from 'svelte-sonner';

	let downloadDir = $state('');
	let downloadFormat = $state('mp3');
	let cookiesBrowser = $state('');
	let defaultVolume = $state(75);

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

	async function handleVolumeChange(values: number[]) {
		defaultVolume = values[0];
		const vol = values[0] / 100;
		await setSetting('default_volume', String(vol));
		player.setVolume(vol);
	}

	let metadataStats: MetadataStats | null = $state(null);
	let scanning = $state(false);
	let scanProgress: MetadataScanProgress | null = $state(null);

	async function loadMetadataStats() {
		try {
			metadataStats = await getMetadataStats();
		} catch (e) {
			console.error('Failed to load metadata stats:', e);
		}
	}

	async function handleScanMetadata() {
		if (scanning) return;
		scanning = true;
		scanProgress = null;

		const unlisten = await listen<MetadataScanProgress>('metadata-scan-progress', (event) => {
			scanProgress = event.payload;
		});

		try {
			const result = await scanMissingMetadata();
			toast.success(`Metadata scan complete`, {
				description: `${result.enriched} tracks enriched, ${result.failed} failed. Average completeness: ${result.completeness_avg}%`
			});
			await loadMetadataStats();
		} catch (e) {
			toast.error('Metadata scan failed', { description: String(e) });
		} finally {
			scanning = false;
			scanProgress = null;
			unlisten();
		}
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
		cookiesBrowser = '';
		defaultVolume = 75;
		await setSetting('default_volume', '0.75');
		await setSetting('download_format', 'mp3');
		await setSetting('cookies_from_browser', '');
		player.setVolume(0.75);
		toast.success('Settings reset to defaults');
	}

	$effect(() => {
		load();
		loadMetadataStats();
	});
</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6 max-w-2xl">
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
					<p class="text-sm font-medium">Scan for Missing Metadata</p>
					<p class="text-xs text-muted-foreground">Looks up tracks with incomplete data on MusicBrainz (up to 50 at a time)</p>
				</div>
				<Button variant="outline" size="sm" onclick={handleScanMetadata} disabled={scanning}>
					{#if scanning}
						<Loader2 class="size-4 animate-spin" />
						Scanning...
					{:else}
						<Sparkles class="size-4" />
						Scan
					{/if}
				</Button>
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
