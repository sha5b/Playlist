<script lang="ts">
	import { listen } from '@tauri-apps/api/event';
	import { formatFileSize } from '$lib/utils/format';
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Progress } from '$lib/components/ui/progress';
	import {
		Loader2,
		RefreshCw,
		ChevronLeft,
		ChevronRight,
		Trash2,
		X,
		Usb,
		HardDrive,
		Settings2,
		FolderSync,
		Check,
		Smartphone,
		ArrowDownToLine,
	} from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import {
		scanDevices,
		getDeviceDetail,
		configureDevice,
		addDevicePlaylist,
		removeDevicePlaylist,
		syncDevice,
		cancelDeviceSync,
		clearDeviceSyncHistory,
	} from '$lib/api/devices';
	import { getPlaylists } from '$lib/api/library';
	import type {
		ScannedDevice,
		DeviceDetail,
		DeviceSyncProgress,
		Playlist,
	} from '$lib/types';

	let scannedDevices: ScannedDevice[] = $state([]);
	let scanningDevices = $state(false);
	let selectedDevice = $state<DeviceDetail | null>(null);
	let loadingDevice = $state(false);
	let localPlaylists: Playlist[] = $state([]);
	let deviceSyncProgress = $state<DeviceSyncProgress | null>(null);
	let syncingAll = $state(false);
	let deviceDetails = $state<Map<number, DeviceDetail>>(new Map());

	// Includes new/replaced tracks and stale files that need removing.
	let totalPendingChanges = $derived(
		selectedDevice
			? selectedDevice.playlists
					.filter((p) => p.enabled)
					.reduce((sum, p) => sum + p.pending_changes, 0)
			: 0
	);

	let playlistsWithChanges = $derived(
		selectedDevice
			? selectedDevice.playlists.filter((p) => p.enabled && p.pending_changes > 0)
			: []
	);

	async function handleScanDevices(silent = false) {
		// Background polls pass silent=true so the Scan button doesn't strobe
		if (!silent) scanningDevices = true;
		try {
			scannedDevices = await scanDevices();
			// Load details for known devices to show sync status
			for (const d of scannedDevices) {
				if (d.device_id && !deviceDetails.has(d.device_id)) {
					try {
						const detail = await getDeviceDetail(d.device_id);
						deviceDetails.set(d.device_id, detail);
						deviceDetails = new Map(deviceDetails);
					} catch {}
				}
			}
		} catch (e) {
			if (!silent) toast.error('Failed to scan devices', { description: String(e) });
		} finally {
			if (!silent) scanningDevices = false;
		}
	}

	async function selectDevice(deviceId: number) {
		loadingDevice = true;
		selectedDevice = null;
		try {
			selectedDevice = await getDeviceDetail(deviceId);
			localPlaylists = await getPlaylists();
			// Auto-link all playlists ONLY when the device is first configured
			// (no linked playlists and no sync history). Doing it on every open
			// would resurrect playlists the user deliberately unlinked.
			const firstConfiguration =
				selectedDevice.playlists.length === 0 && selectedDevice.synced_track_count === 0;
			if (firstConfiguration) {
				for (const pl of localPlaylists) {
					await addDevicePlaylist(deviceId, pl.id);
				}
				selectedDevice = await getDeviceDetail(deviceId);
			}
		} catch (e) {
			toast.error('Failed to load device', { description: String(e) });
		} finally {
			loadingDevice = false;
		}
	}

	async function handleConfigureDevice() {
		if (!selectedDevice) return;
		const d = selectedDevice.device;
		try {
			await configureDevice(d.id, d.music_dir, d.output_format, d.output_bitrate, d.generate_m3u);
			selectedDevice = await getDeviceDetail(d.id);
			deviceDetails.set(d.id, selectedDevice);
			deviceDetails = new Map(deviceDetails);
			toast.success('Device settings saved');
		} catch (e) {
			toast.error('Failed to save settings', { description: String(e) });
		}
	}

	async function saveVisibleDeviceSettings(): Promise<DeviceDetail> {
		if (!selectedDevice) throw new Error('No device selected');
		const d = selectedDevice.device;
		await configureDevice(d.id, d.music_dir, d.output_format, d.output_bitrate, d.generate_m3u);
		const detail = await getDeviceDetail(d.id);
		selectedDevice = detail;
		deviceDetails.set(d.id, detail);
		deviceDetails = new Map(deviceDetails);
		return detail;
	}

	async function togglePlaylistSync(playlistId: number, linked: boolean) {
		if (!selectedDevice) return;
		try {
			if (linked) {
				await removeDevicePlaylist(selectedDevice.device.id, playlistId);
			} else {
				await addDevicePlaylist(selectedDevice.device.id, playlistId);
			}
			selectedDevice = await getDeviceDetail(selectedDevice.device.id);
		} catch (e) {
			toast.error('Failed to update playlist link', { description: String(e) });
		}
	}

	function isCancelled(e: unknown): boolean {
		return String(e).toLowerCase().includes('cancel');
	}

	async function handleSyncPlaylist(playlistId: number) {
		if (!selectedDevice) return;
		try {
			// Sync exactly what the settings panel currently shows. Previously an
			// unsaved "Original" selection still used the old stored Opus setting.
			await saveVisibleDeviceSettings();
			await syncDevice(selectedDevice.device.id, playlistId);
			toast.success('Playlist synced');
		} catch (e) {
			// Belt-and-braces: if the backend failed without emitting a terminal
			// progress event, don't leave the sync buttons frozen.
			deviceSyncProgress = null;
			if (isCancelled(e)) return;
			toast.error('Failed to sync', { description: String(e) });
		}
	}

	async function handleSyncAll() {
		if (!selectedDevice) return;
		syncingAll = true;
		try {
			const detail = await saveVisibleDeviceSettings();
			const pending = detail.playlists.filter((p) => p.enabled && p.pending_changes > 0);
			if (pending.length === 0) {
				toast.success('Everything is already synced');
				return;
			}
			for (const pl of pending) {
				// Syncs run sequentially; each call resolves when that playlist finishes.
				await syncDevice(selectedDevice.device.id, pl.playlist_id);
			}
			toast.success('All playlists synced');
		} catch (e) {
			deviceSyncProgress = null;
			if (!isCancelled(e)) toast.error('Failed to sync', { description: String(e) });
		} finally {
			syncingAll = false;
		}
	}

	async function handleCancelSync() {
		try {
			await cancelDeviceSync();
			deviceSyncProgress = null;
		} catch (e) {
			toast.error('Failed to cancel sync', { description: String(e) });
		}
	}

	async function handleClearSyncHistory() {
		if (!selectedDevice) return;
		try {
			const cleared = await clearDeviceSyncHistory(selectedDevice.device.id);
			toast.success(`Cleared sync history (${cleared} tracks)`);
			selectedDevice = await getDeviceDetail(selectedDevice.device.id);
		} catch (e) {
			toast.error('Failed to clear history', { description: String(e) });
		}
	}

	function getDeviceUnsynced(deviceId: number | null): number {
		if (!deviceId) return 0;
		const detail = deviceDetails.get(deviceId);
		if (!detail) return 0;
		return detail.playlists
			.filter((p) => p.enabled)
			.reduce((sum, p) => sum + p.pending_changes, 0);
	}

	async function refreshDeviceDetail(deviceId: number) {
		try {
			const detail = await getDeviceDetail(deviceId);
			deviceDetails.set(deviceId, detail);
			deviceDetails = new Map(deviceDetails);
		} catch {}
	}

	async function handleQuickSyncDevice(e: Event, deviceId: number) {
		e.stopPropagation();
		const detail = deviceDetails.get(deviceId);
		if (!detail) return;
		const unsynced = detail.playlists.filter((p) => p.enabled && p.pending_changes > 0);
		try {
			for (const pl of unsynced) {
				await syncDevice(deviceId, pl.playlist_id);
			}
			toast.success('Device synced');
		} catch (e) {
			deviceSyncProgress = null;
			if (!isCancelled(e)) toast.error('Failed to sync', { description: String(e) });
		} finally {
			// Refresh the list-view "Sync N" badge so it reflects the new state.
			await refreshDeviceDetail(deviceId);
		}
	}

	// Auto-scan and poll while page is active
	$effect(() => {
		handleScanDevices();
		const interval = setInterval(() => handleScanDevices(true), 5000);

		// Delayed clear of the terminal progress message. The handle is
		// tracked so a NEW sync starting within the 3s window cancels the
		// clear — otherwise playlist A's leftover timer wipes playlist B's
		// in-flight progress bar mid-"Sync all".
		let progressClearTimer: ReturnType<typeof setTimeout> | undefined;

		let disposed = false;
		let unlistenSync: (() => void) | null = null;
		listen<DeviceSyncProgress>('device-sync-progress', (event) => {
			const p = event.payload;
			const terminal = p.status === 'done' || p.status === 'error' || p.status === 'cancelled';
			clearTimeout(progressClearTimer);
			deviceSyncProgress = p;
			if (terminal) {
				if (selectedDevice) {
					getDeviceDetail(selectedDevice.device.id).then((d) => { selectedDevice = d; });
				}
				if (p.status === 'done') {
					// Keep the list-view "Sync N" badge in step with the new state.
					refreshDeviceDetail(p.device_id);
				}
				progressClearTimer = setTimeout(() => { deviceSyncProgress = null; }, 3000);
			}
		}).then((fn) => {
			if (disposed) fn();
			else unlistenSync = fn;
		});

		return () => {
			disposed = true;
			clearInterval(interval);
			clearTimeout(progressClearTimer);
			unlistenSync?.();
		};
	});
	// Phones/cameras mount via GVfs (MTP/PTP/AFC) — their device uid is
	// prefixed "gvfs:" by the scanner.
	function isPortableDevice(uid: string | undefined): boolean {
		return !!uid?.startsWith('gvfs:');
	}
</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Devices</h1>
		<p class="text-muted-foreground mt-1">
			Sync your music to connected devices
		</p>
	</div>

	{#if selectedDevice}
		<!-- Device detail view -->
		<div class="space-y-4">
			<!-- Header -->
			<div class="flex items-start gap-4">
				<Button variant="ghost" size="icon" onclick={() => { selectedDevice = null; }} class="shrink-0 mt-0.5 rounded-full">
					<ChevronLeft class="size-5" />
				</Button>
				<div class="flex-1 min-w-0">
					<h2 class="text-xl font-semibold truncate">{selectedDevice.device.name}</h2>
					<p class="text-sm text-muted-foreground mt-0.5">
						{selectedDevice.device.mount_path ?? 'Not connected'}
						{#if selectedDevice.device.capacity_bytes}
							&middot; {formatFileSize(selectedDevice.device.capacity_bytes)}
						{/if}
					</p>
				</div>
				<div class="flex items-center gap-2">
					<Badge variant="secondary" class="text-xs">{selectedDevice.synced_track_count} synced</Badge>
				</div>
			</div>

			<!-- Sync banner (like manager's "X new tracks" banner) -->
			{#if totalPendingChanges > 0}
				<div class="flex items-center gap-4 rounded-xl bg-primary/8 border border-primary/20 px-5 py-3.5">
					<div class="flex items-center gap-3 flex-1 min-w-0">
						<div class="flex items-center justify-center size-10 rounded-full bg-primary/15 shrink-0">
							<FolderSync class="size-5 text-primary" />
						</div>
						<div>
							<p class="text-sm font-medium">
								{totalPendingChanges} change{totalPendingChanges !== 1 ? 's' : ''} to sync
							</p>
							<p class="text-xs text-muted-foreground mt-0.5">
								Across {playlistsWithChanges.length} playlist{playlistsWithChanges.length !== 1 ? 's' : ''}
							</p>
						</div>
					</div>
					<Button
						size="sm"
						class="gap-1.5 shrink-0"
						disabled={syncingAll || !!deviceSyncProgress}
						onclick={handleSyncAll}
					>
						{#if syncingAll}
							<Loader2 class="size-3.5 animate-spin" />
						{:else}
							<FolderSync class="size-3.5" />
						{/if}
						Sync all
					</Button>
				</div>
			{/if}

			<!-- Sync progress -->
			{#if deviceSyncProgress && deviceSyncProgress.device_id === selectedDevice.device.id}
				<div class="flex items-center gap-3 rounded-lg bg-muted/30 px-4 py-2.5">
					{#if deviceSyncProgress.status === 'done' || deviceSyncProgress.status === 'cancelled' || deviceSyncProgress.status === 'error'}
						<span class="text-sm font-medium">
							{#if deviceSyncProgress.status === 'done'}
								Sync complete!
							{:else if deviceSyncProgress.status === 'cancelled'}
								Sync cancelled
							{:else}
								Sync error{#if deviceSyncProgress.error}: {deviceSyncProgress.error}{/if}
							{/if}
						</span>
					{:else}
						<span class="relative flex size-2 shrink-0">
							<span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-info opacity-75"></span>
							<span class="relative inline-flex rounded-full size-2 bg-info"></span>
						</span>
						<div class="flex-1 min-w-0">
							<div class="flex items-center justify-between text-sm">
								<span class="font-medium truncate">
									{#if deviceSyncProgress.status === 'copying'}
										Copying: {deviceSyncProgress.track_title}
									{:else if deviceSyncProgress.status === 'converting'}
										Converting: {deviceSyncProgress.track_title}
									{:else if deviceSyncProgress.status === 'generating_playlist'}
										Generating playlist...
									{/if}
								</span>
								<span class="text-muted-foreground tabular-nums shrink-0 ml-2">{deviceSyncProgress.current} / {deviceSyncProgress.total}</span>
							</div>
							<Progress value={deviceSyncProgress.total > 0 ? (deviceSyncProgress.current / deviceSyncProgress.total) * 100 : 0} class="h-1.5 mt-1.5" />
						</div>
						<Button variant="ghost" size="sm" onclick={handleCancelSync} class="gap-1 text-destructive h-7 shrink-0">
							<X class="size-3" />
							Cancel
						</Button>
					{/if}
				</div>
			{/if}

			<!-- Playlists to sync -->
			<div class="rounded-lg border p-4 space-y-3">
				<h3 class="text-sm font-medium flex items-center gap-2">
					<FolderSync class="size-4" />
					Playlists to Sync
				</h3>
				{#if localPlaylists.length === 0}
					<p class="text-sm text-muted-foreground">No playlists in your library yet.</p>
				{:else}
					<div class="space-y-1">
						{#each localPlaylists as pl}
							{@const linked = selectedDevice.playlists.find((p) => p.playlist_id === pl.id)}
							{@const pendingChanges = linked?.pending_changes ?? 0}
							<div class="flex items-center justify-between p-2.5 rounded-md hover:bg-muted/50 transition-colors">
								<!-- svelte-ignore a11y_click_events_have_key_events -->
							<!-- svelte-ignore a11y_no_static_element_interactions -->
							<div class="flex items-center gap-3 cursor-pointer" onclick={() => togglePlaylistSync(pl.id, !!linked)}>
								<input type="checkbox" checked={!!linked}
									class="rounded border-input pointer-events-none" tabindex={-1} />
								<div>
										<p class="text-sm font-medium">{pl.name}</p>
										<p class="text-xs text-muted-foreground">{pl.track_count} track{pl.track_count !== 1 ? 's' : ''}</p>
									</div>
								</div>
								{#if linked}
									<div class="flex items-center gap-2">
										<span class="text-xs text-muted-foreground tabular-nums">
											{linked.synced_tracks}/{linked.total_tracks} synced
										</span>
									{#if pendingChanges > 0}
											<Button
												variant="outline"
												size="sm"
												onclick={() => handleSyncPlaylist(pl.id)}
												disabled={!!deviceSyncProgress}
												class="gap-1.5 h-7"
											>
												<FolderSync class="size-3" />
											Sync {pendingChanges} change{pendingChanges !== 1 ? 's' : ''}
											</Button>
										{:else}
											<Badge variant="secondary" class="text-xs gap-1">
												<Check class="size-3" />
												Synced
											</Badge>
										{/if}
									</div>
								{/if}
							</div>
						{/each}
					</div>
				{/if}
			</div>

			<!-- Device settings (collapsible feel, below playlists) -->
			<div class="rounded-lg border p-4 space-y-4">
				<h3 class="text-sm font-medium flex items-center gap-2">
					<Settings2 class="size-4" />
					Sync Settings
				</h3>
				<div class="grid grid-cols-2 gap-4">
					<div class="space-y-1.5">
						<label for="music-dir" class="text-xs text-muted-foreground">Music Directory</label>
						<Input id="music-dir" bind:value={selectedDevice.device.music_dir} placeholder="Music" class="h-8 text-sm" />
					</div>
					<div class="space-y-1.5">
						<label for="output-format" class="text-xs text-muted-foreground">Output Format</label>
						<select id="output-format" bind:value={selectedDevice.device.output_format} onchange={handleConfigureDevice}
							class="flex h-8 w-full rounded-md border border-input bg-background px-3 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
							<option value="original">Original (no conversion)</option>
							<option value="mp3">MP3</option>
							<option value="opus">Opus</option>
							<option value="flac">FLAC</option>
						</select>
					</div>
					{#if selectedDevice.device.output_format !== 'original' && selectedDevice.device.output_format !== 'flac'}
						<div class="space-y-1.5">
							<label for="output-bitrate" class="text-xs text-muted-foreground">Bitrate (kbps)</label>
							<select id="output-bitrate" bind:value={selectedDevice.device.output_bitrate}
								class="flex h-8 w-full rounded-md border border-input bg-background px-3 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
								<option value="128">128</option>
								<option value="192">192</option>
								<option value="256">256</option>
								<option value="320">320</option>
							</select>
						</div>
					{/if}
					<div class="flex items-center gap-2 pt-5">
						<input type="checkbox" id="generate-m3u" bind:checked={selectedDevice.device.generate_m3u}
							class="rounded border-input" />
						<label for="generate-m3u" class="text-sm">Generate M3U playlists</label>
					</div>
				</div>
				<div class="flex justify-end">
					<Button variant="outline" size="sm" onclick={handleConfigureDevice} class="gap-1.5">
						<Check class="size-3" />
						Save Settings
					</Button>
				</div>
			</div>

			<!-- Actions -->
			<div class="flex items-center justify-between pt-2">
				<Button variant="ghost" size="sm" onclick={handleClearSyncHistory} class="gap-1.5 text-muted-foreground hover:text-destructive">
					<Trash2 class="size-3.5" />
					Clear Sync History
				</Button>
			</div>
		</div>
	{:else}
		<!-- Device list view -->
		<div class="space-y-4">
			<div class="flex items-center justify-between">
				<p class="text-sm text-muted-foreground">
					Connect a phone, MP3 player, or USB drive to sync your music.
				</p>
				<Button variant="outline" size="sm" onclick={() => handleScanDevices()} disabled={scanningDevices} class="gap-1.5">
					{#if scanningDevices}
						<Loader2 class="size-3 animate-spin" />
					{:else}
						<RefreshCw class="size-3" />
					{/if}
					Scan
				</Button>
			</div>

			{#if scannedDevices.length === 0}
				<div class="flex flex-col items-center justify-center py-16 text-center space-y-3">
					<div class="rounded-full bg-muted p-4">
						<Usb class="size-8 text-muted-foreground" />
					</div>
					<div>
						<h3 class="font-medium">No devices found</h3>
						<p class="text-sm text-muted-foreground mt-1 max-w-xs">
							Connect a USB device and click Scan, or it will be detected automatically.
						</p>
					</div>
				</div>
			{:else}
				<div class="grid gap-3">
					{#each scannedDevices as device}
						{@const unsynced = getDeviceUnsynced(device.device_id)}
						<!-- div[role=button] instead of <button>: the quick-sync Button inside
							would otherwise be invalid nested-interactive HTML -->
						<div
							role="button"
							tabindex="0"
							class="flex items-center gap-4 p-4 rounded-lg border hover:bg-muted/50 transition-colors text-left w-full cursor-pointer"
							onclick={() => { if (device.device_id) selectDevice(device.device_id); }}
							onkeydown={(e) => {
								if ((e.key === 'Enter' || e.key === ' ') && device.device_id) {
									e.preventDefault();
									selectDevice(device.device_id);
								}
							}}
						>
							<div class="rounded-full bg-muted p-2.5">
								{#if isPortableDevice(device.device_uid)}
									<Smartphone class="size-5 text-muted-foreground" />
								{:else}
									<HardDrive class="size-5 text-muted-foreground" />
								{/if}
							</div>
							<div class="flex-1 min-w-0">
								<p class="font-medium truncate">{device.name}</p>
								<p class="text-xs text-muted-foreground truncate">{device.mount_path}</p>
							</div>
							{#if unsynced > 0}
								<Button
									variant="default"
									size="sm"
									class="gap-1.5 shrink-0"
									onclick={(e) => { if (device.device_id) handleQuickSyncDevice(e, device.device_id); }}
								>
									<FolderSync class="size-3.5" />
									Sync {unsynced}
								</Button>
							{/if}
							<div class="text-right shrink-0">
								{#if device.capacity_bytes}
									<p class="text-sm tabular-nums">{formatFileSize(device.free_bytes)} free</p>
									<p class="text-xs text-muted-foreground tabular-nums">{formatFileSize(device.capacity_bytes)} total</p>
								{/if}
							</div>
							<ChevronRight class="size-4 text-muted-foreground shrink-0" />
						</div>
					{/each}
				</div>
			{/if}
		</div>
	{/if}
</div>
