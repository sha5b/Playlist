<script lang="ts">
	import { Separator } from '$lib/components/ui/separator';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Slider } from '$lib/components/ui/slider';
	import { FolderOpen, Volume2, RotateCcw, Music, Trash2, Cookie, Sparkles, Loader2, CircleX, Speaker, HardDriveDownload, Eye, FolderPlus, X } from 'lucide-svelte';
	import { watchGetStatus, watchSetEnabled, watchAddFolder, watchRemoveFolder } from '$lib/api/library';
	import { getSetting, setSetting, resetLibrary, getMetadataStats, scanMissingMetadata, stopMetadataScan, deleteAllMetadata, exportLibrary } from '$lib/api/library';
	import { startLastfmAuth, finishLastfmAuth, getLastfmStatus, disconnectLastfm, setLastfmScrobbling } from '$lib/api/lastfm';
	import type { LastfmStatus } from '$lib/types';
	import { Radio } from 'lucide-svelte';
	import { listen } from '@tauri-apps/api/event';
	import { getAudioDevices, setAudioDevice } from '$lib/api/player';
	import type { MetadataStats } from '$lib/types';
	import { player } from '$lib/stores/player.svelte';
	import { metadataScanStore } from '$lib/stores/metadataScan.svelte';
	import { open } from '@tauri-apps/plugin-dialog';
	import { toast } from 'svelte-sonner';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import { downloadStore } from '$lib/stores/downloads.svelte';

	import { getVersion } from '@tauri-apps/api/app';

	let resetLibraryOpen = $state(false);
	let deleteMetadataOpen = $state(false);
	let appVersion = $state('');
	getVersion().then((v) => { appVersion = v; }).catch(() => {});

	let downloadDir = $state('');
	let downloadFormat = $state('mp3');
	let cookiesBrowser = $state('');
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

	// --- Folder watch / auto-import ---
	let watchEnabled = $state(false);
	let watchFolders = $state<string[]>([]);

	async function loadWatch() {
		try {
			const status = await watchGetStatus();
			watchEnabled = status.enabled;
			watchFolders = status.folders;
		} catch (e) {
			console.error('Failed to load watched folders:', e);
		}
	}

	async function toggleWatch() {
		try {
			const status = await watchSetEnabled(!watchEnabled);
			watchEnabled = status.enabled;
			watchFolders = status.folders;
			toast.success(watchEnabled ? 'Folder watch enabled' : 'Folder watch disabled');
		} catch (e) {
			toast.error('Failed to update folder watch', { description: String(e) });
		}
	}

	async function addWatchFolder() {
		// Suggest the download folder as a starting point, but never auto-watch
		// it — downloads already import themselves.
		const selected = await open({
			directory: true,
			title: 'Choose folder to watch',
			defaultPath: downloadDir || undefined,
		});
		if (!selected) return;
		try {
			const status = await watchAddFolder(selected as string);
			watchFolders = status.folders;
			toast.success('Folder added to watch list');
		} catch (e) {
			toast.error('Failed to add folder', { description: String(e) });
		}
	}

	async function removeWatchFolder(path: string) {
		try {
			const status = await watchRemoveFolder(path);
			watchFolders = status.folders;
			toast.success('Folder removed from watch list');
		} catch (e) {
			toast.error('Failed to remove folder', { description: String(e) });
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

	// --- Export state ---
	let exporting = $state(false);
	let exportProgress = $state<{ current: number; total: number; track_title: string } | null>(null);

	async function handleExportLibrary() {
		const selected = await open({ directory: true, title: 'Choose export destination' });
		if (!selected) return;

		exporting = true;
		exportProgress = null;

		const unlisten = await listen<{ current: number; total: number; track_title: string }>('export-progress', (event) => {
			exportProgress = event.payload;
		});

		try {
			const result = await exportLibrary(selected as string);
			toast.success('Library exported', {
				description: `${result.exported} copied, ${result.skipped} skipped, ${result.failed} failed`,
			});
		} catch (e) {
			toast.error('Export failed', { description: String(e) });
		} finally {
			unlisten();
			exporting = false;
			exportProgress = null;
		}
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
		// bits-ui v2 AlertDialog.Action does not auto-close — close explicitly
		deleteMetadataOpen = false;
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
		resetLibraryOpen = false;
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
		cookiesBrowser = '';
		defaultVolume = 75;
		autoDownloadMV = false;
		selectedDevice = '';
		await setSetting('download_dir', '');
		await setSetting('default_volume', '0.75');
		await setSetting('download_format', 'mp3');
		await setSetting('cookies_from_browser', '');
		await setSetting('auto_download_music_videos', 'false');
		await setSetting('audio_device', '');
		await setAudioDevice(null);
		player.setVolume(0.75);
		toast.success('Settings reset to defaults');
	}

	// --- Last.fm scrobbling state ---
	let lastfm = $state<LastfmStatus | null>(null);
	let lastfmToken = $state<string | null>(null);
	let lastfmBusy = $state(false);

	async function loadLastfm() {
		try {
			lastfm = await getLastfmStatus();
		} catch (e) {
			console.error('Failed to load Last.fm status:', e);
		}
	}

	async function handleLastfmConnect() {
		lastfmBusy = true;
		try {
			const auth = await startLastfmAuth();
			lastfmToken = auth.token;
			toast.info('Authorize Playlist on Last.fm in your browser, then come back and confirm.');
		} catch (e) {
			toast.error('Failed to start Last.fm authorization', { description: String(e) });
		} finally {
			lastfmBusy = false;
		}
	}

	async function handleLastfmFinish() {
		if (!lastfmToken) return;
		lastfmBusy = true;
		try {
			lastfm = await finishLastfmAuth(lastfmToken);
			lastfmToken = null;
			toast.success(`Connected to Last.fm as ${lastfm.username ?? 'unknown'}`);
		} catch (e) {
			toast.error('Last.fm authorization failed', { description: String(e) });
		} finally {
			lastfmBusy = false;
		}
	}

	async function handleLastfmDisconnect() {
		try {
			lastfm = await disconnectLastfm();
			lastfmToken = null;
			toast.success('Disconnected from Last.fm');
		} catch (e) {
			toast.error('Failed to disconnect', { description: String(e) });
		}
	}

	async function handleLastfmToggleScrobbling() {
		if (!lastfm) return;
		try {
			lastfm = await setLastfmScrobbling(!lastfm.scrobbling_enabled);
		} catch (e) {
			toast.error('Failed to update scrobbling', { description: String(e) });
		}
	}

	$effect(() => {
		load();
		loadMetadataStats();
		loadAudioDevices();
		loadWatch();
		loadLastfm();
	});
</script>

<div class="flex-1 min-h-0 overflow-y-auto overflow-x-hidden space-y-6 px-1">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Settings</h1>
		<p class="text-muted-foreground mt-1">Configure your preferences</p>
	</div>

	<!-- Downloads -->
	<section class="space-y-4">
		<div>
			<h2 class="text-lg font-semibold">Downloads</h2>
			<p class="text-sm text-muted-foreground">Download folder and audio format</p>
		</div>

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
	</section>

	<Separator />

	<!-- Watched Folders -->
	<section class="space-y-4">
		<div>
			<h2 class="text-lg font-semibold">Watched Folders</h2>
			<p class="text-sm text-muted-foreground">Automatically import new audio files from these folders</p>
		</div>

		<div class="flex items-center justify-between gap-4">
			<div>
				<p class="text-sm font-medium flex items-center gap-2">
					<Eye class="size-4 text-muted-foreground" />
					Watch folders for new music
				</p>
				<p class="text-xs text-muted-foreground">
					New files (e.g. Bandcamp purchases) are imported automatically. Your download folder already self-imports and doesn't need watching.
				</p>
			</div>
			<Button
				variant={watchEnabled ? 'default' : 'outline'}
				size="sm"
				onclick={toggleWatch}
			>
				{watchEnabled ? 'On' : 'Off'}
			</Button>
		</div>

		<div class="space-y-2">
			{#if watchFolders.length === 0}
				<p class="text-xs text-muted-foreground italic">No folders are being watched yet.</p>
			{:else}
				<ul class="space-y-1">
					{#each watchFolders as folder (folder)}
						<li class="flex items-center justify-between gap-2 rounded-md bg-muted/40 px-3 py-1.5">
							<span class="flex items-center gap-2 min-w-0 text-sm">
								<FolderOpen class="size-4 shrink-0 text-muted-foreground" />
								<span class="truncate">{folder}</span>
							</span>
							<Button
								variant="ghost"
								size="sm"
								class="shrink-0 h-7 w-7 p-0"
								title="Stop watching this folder"
								onclick={() => removeWatchFolder(folder)}
							>
								<X class="size-4" />
							</Button>
						</li>
					{/each}
				</ul>
			{/if}
			<Button variant="outline" size="sm" onclick={addWatchFolder}>
				<FolderPlus class="size-4" />
				Add Folder
			</Button>
		</div>
	</section>

	<Separator />

	<!-- Export Library -->
	<section class="space-y-4">
		<div>
			<h2 class="text-lg font-semibold">Export Library</h2>
			<p class="text-sm text-muted-foreground">Copy all your music into a clean folder structure</p>
		</div>

		<div class="flex items-center justify-between gap-4">
			<div>
				<p class="text-sm font-medium">Export all tracks</p>
				<p class="text-xs text-muted-foreground">
					Copies files into <code class="text-[11px] bg-muted px-1 py-0.5 rounded">Artist/Album/01 - Title.ext</code> — originals are not moved
				</p>
			</div>
			<Button variant="outline" size="sm" onclick={handleExportLibrary} disabled={exporting} class="gap-1.5 shrink-0">
				{#if exporting}
					<Loader2 class="size-4 animate-spin" />
					Exporting...
				{:else}
					<HardDriveDownload class="size-4" />
					Export
				{/if}
			</Button>
		</div>
		{#if exporting && exportProgress}
			<div class="space-y-2 rounded-md bg-muted/50 p-3">
				<div class="flex items-center justify-between text-sm">
					<span class="text-muted-foreground">Copying files...</span>
					<span class="tabular-nums font-medium">{exportProgress.current}/{exportProgress.total}</span>
				</div>
				<div class="h-1.5 rounded-full bg-muted overflow-hidden">
					<div class="h-full rounded-full bg-primary transition-all" style="width: {(exportProgress.current / exportProgress.total) * 100}%"></div>
				</div>
				<p class="text-xs text-muted-foreground truncate">{exportProgress.track_title}</p>
			</div>
		{/if}
	</section>

	<Separator />

	<!-- Playback -->
	<section class="space-y-4">
		<div>
			<h2 class="text-lg font-semibold">Playback</h2>
			<p class="text-sm text-muted-foreground">Audio playback settings</p>
		</div>

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
	</section>

	<Separator />

	<!-- Metadata -->
	<section class="space-y-4" id="metadata">
		<div>
			<h2 class="text-lg font-semibold">Metadata</h2>
			<p class="text-sm text-muted-foreground">Enrich your library with MusicBrainz data</p>
		</div>

		{#if metadataStats}
			<div class="space-y-3 rounded-lg bg-muted/30 p-4">
				<div class="flex items-center justify-between">
					<span class="text-sm text-muted-foreground">Average Completeness</span>
					<span class="text-sm font-medium tabular-nums {metadataStats.average_completeness >= 80 ? 'text-success' : metadataStats.average_completeness >= 50 ? 'text-warning' : 'text-destructive'}">
						{metadataStats.average_completeness}%
					</span>
				</div>
				<div class="h-2 rounded-full bg-muted overflow-hidden">
					<div
						class="h-full rounded-full transition-all duration-500 {metadataStats.average_completeness >= 80 ? 'bg-success' : metadataStats.average_completeness >= 50 ? 'bg-warning' : 'bg-destructive'}"
						style="width: {metadataStats.average_completeness}%"
					></div>
				</div>
				<div class="grid grid-cols-3 gap-4 text-center">
					<div>
						<p class="text-2xl font-bold tabular-nums">{metadataStats.total_tracks}</p>
						<p class="text-xs text-muted-foreground">Total Tracks</p>
					</div>
					<div>
						<p class="text-2xl font-bold tabular-nums text-success">{metadataStats.complete_tracks}</p>
						<p class="text-xs text-muted-foreground">Complete (80%+)</p>
					</div>
					<div>
						<p class="text-2xl font-bold tabular-nums text-destructive">{metadataStats.incomplete_tracks}</p>
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
	</section>

	<Separator />

	<!-- Last.fm -->
	<section class="space-y-4" id="lastfm">
		<div>
			<h2 class="text-lg font-semibold">Last.fm</h2>
			<p class="text-sm text-muted-foreground">Scrobble your plays to your Last.fm profile</p>
		</div>

		{#if lastfm?.connected}
			<div class="flex items-center justify-between gap-4">
				<div>
					<p class="text-sm font-medium flex items-center gap-2">
						<Radio class="size-4 text-success" />
						Connected as {lastfm.username ?? 'unknown'}
					</p>
					<p class="text-xs text-muted-foreground">
						{#if lastfm.pending_scrobbles > 0}
							{lastfm.pending_scrobbles} scrobble{lastfm.pending_scrobbles === 1 ? '' : 's'} queued (sent on next play)
						{:else}
							Plays are scrobbled after they count as a full play
						{/if}
					</p>
				</div>
				<Button variant="outline" size="sm" onclick={handleLastfmDisconnect}>
					<X class="size-4" />
					Disconnect
				</Button>
			</div>

			<div class="flex items-center justify-between gap-4">
				<div>
					<p class="text-sm font-medium">Scrobbling</p>
					<p class="text-xs text-muted-foreground">Send plays and now-playing updates to Last.fm</p>
				</div>
				<Button
					variant={lastfm.scrobbling_enabled ? 'default' : 'outline'}
					size="sm"
					onclick={handleLastfmToggleScrobbling}
				>
					{lastfm.scrobbling_enabled ? 'On' : 'Off'}
				</Button>
			</div>
		{:else if lastfmToken}
			<div class="space-y-3 rounded-md bg-muted/50 p-3">
				<p class="text-sm">
					A Last.fm authorization page was opened in your browser. Approve access there, then confirm below.
				</p>
				<div class="flex gap-2">
					<Button size="sm" onclick={handleLastfmFinish} disabled={lastfmBusy}>
						{#if lastfmBusy}
							<Loader2 class="size-4 animate-spin" />
						{/if}
						I've authorized
					</Button>
					<Button variant="outline" size="sm" onclick={() => (lastfmToken = null)} disabled={lastfmBusy}>
						Cancel
					</Button>
				</div>
			</div>
		{:else}
			<div class="flex items-center justify-between gap-4">
				<div>
					<p class="text-sm font-medium">Connect your Last.fm account</p>
					<p class="text-xs text-muted-foreground">Opens Last.fm in your browser to authorize Playlist</p>
				</div>
				<Button variant="outline" size="sm" onclick={handleLastfmConnect} disabled={lastfmBusy}>
					{#if lastfmBusy}
						<Loader2 class="size-4 animate-spin" />
						Connecting...
					{:else}
						<Radio class="size-4" />
						Connect
					{/if}
				</Button>
			</div>
		{/if}
	</section>

	<Separator />

	<!-- Danger Zone -->
	<section class="space-y-4">
		<div>
			<h2 class="text-lg font-semibold text-destructive">Danger Zone</h2>
			<p class="text-sm text-muted-foreground">Irreversible actions</p>
		</div>

		<div class="flex items-center justify-between gap-4">
			<div>
				<p class="text-sm font-medium">Delete All Metadata</p>
				<p class="text-xs text-muted-foreground">Stops any running scan, then clears all enriched metadata</p>
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

		<Separator class="opacity-50" />

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

		<Separator class="opacity-50" />

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
	</section>

	<Separator />

	<!-- About -->
	<section class="space-y-3 pb-4">
		<div>
			<h2 class="text-lg font-semibold">About</h2>
			<p class="text-sm text-muted-foreground">Application information</p>
		</div>

		<div class="space-y-3">
			<div class="flex justify-between text-sm">
				<span class="text-muted-foreground">Version</span>
				<span>{appVersion || '—'}</span>
			</div>
			<div class="flex justify-between text-sm">
				<span class="text-muted-foreground">Stack</span>
				<span>Tauri 2.x + SvelteKit + Svelte 5</span>
			</div>
			<p class="text-sm text-muted-foreground pt-1">
				Liberate your music from streaming platforms. Own your data, build your library, support artists directly. Buy their albums, back them on Patreon, go to their shows.
			</p>
		</div>
	</section>
</div>
