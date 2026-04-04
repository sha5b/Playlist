<script lang="ts">
	import { page } from '$app/stores';
	import { listen } from '@tauri-apps/api/event';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Progress } from '$lib/components/ui/progress';
	import * as Tabs from '$lib/components/ui/tabs';

	import {
		Download,
		Loader2,
		RotateCcw,
		PackageOpen,
		ListMusic,
		Clock,
		AlertCircle,
	} from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import {
		startDownload,
		cancelDownload,
		retryDownload,
		getDownloadHistory,
		clearHistory,
	} from '$lib/api/downloads';
	import {
		getMonitoredPlaylists,
		addPlaylist,
		syncPlaylist,
		getEntries,
		downloadEntry,
		downloadNew,
		skipEntry,
		cancelEntry,
		cancelAllDownloads,
		removePlaylist,
	} from '$lib/api/manager';
	import { downloadStore } from '$lib/stores/downloads.svelte';
	import { depsStore } from '$lib/stores/deps.svelte';
	import type {
		Download as DownloadType,
		MonitoredPlaylist,
		MonitoredEntry,
		ManagerEntryEvent,
		MonitoredEntryStatus,
	} from '$lib/types';
	import { groupDownloadsByAlbum } from '$lib/utils/grouping';
	import ManagerPlaylistsTab from '$lib/components/manager/ManagerPlaylistsTab.svelte';
	import ManagerDownloadsTab from '$lib/components/manager/ManagerDownloadsTab.svelte';
	import ManagerHistoryTab from '$lib/components/manager/ManagerHistoryTab.svelte';

	// --- Deps state (from global store, checked once at app startup) ---
	const depsStatus = $derived(depsStore.status);
	const depsChecking = $derived(depsStore.checking);
	const setupInProgress = $derived(depsStore.setupInProgress);
	const setupMessage = $derived(depsStore.setupMessage);
	const setupProgress = $derived(depsStore.setupProgress);
	const setupError = $derived(depsStore.setupError);
	const depsReady = $derived(depsStore.ready);

	// --- Download tab state ---
	let urlInput = $state('');
	let submitting = $state(false);
	let completedPage = $state(0);
	const completedPageSize = 50;

	// --- History tab state ---
	let history: DownloadType[] = $state([]);
	let historyTotal = $state(0);
	let historyLoaded = $state(false);
	let historyPage = $state(0);
	const historyPageSize = 50;

	// --- Playlists tab state ---
	let playlists: MonitoredPlaylist[] = $state([]);
	let playlistUrlInput = $state('');
	let addingPlaylist = $state(false);
	let syncingAll = $state(false);
	let syncAllProgress = $state({ current: 0, total: 0, name: '' });
	let syncingIds = $state<Set<number>>(new Set());
	let selectedPlaylist = $state<MonitoredPlaylist | null>(null);
	let selectedEntries: MonitoredEntry[] = $state([]);
	let loadingEntries = $state(false);
	let downloadingEntryIds = $state<Set<number>>(new Set());
	let entriesPage = $state(0);
	const entriesPageSize = 50;
	let retryingAll = $state(false);

	// --- Active tab (read from URL param) ---
	const validTabs = ['playlists', 'downloads', 'history'];
	let activeTab = $state($page.url.searchParams.get('tab') || 'playlists');

	$effect(() => {
		const tabParam = $page.url.searchParams.get('tab');
		if (tabParam && validTabs.includes(tabParam)) {
			activeTab = tabParam;
		}
	});

	// --- Initialize ---
	$effect(() => {
		downloadStore.init();

		if (depsReady) {
			loadPlaylists();
		}

		let unlistenEntry: (() => void) | null = null;
		listen<ManagerEntryEvent>('manager-entry-updated', (event) => {
			const { entry_id, status } = event.payload;
			selectedEntries = selectedEntries.map((e) =>
				e.id === entry_id ? { ...e, status: status as MonitoredEntryStatus } : e
			);
			loadPlaylists();
		}).then((fn) => {
			unlistenEntry = fn;
		});

		let unlistenThumbs: (() => void) | null = null;
		listen<{ playlist_id: number }>('manager-thumbnails-ready', (event) => {
			if (selectedPlaylist?.id === event.payload.playlist_id) {
				loadEntries(event.payload.playlist_id);
			}
			loadPlaylists();
		}).then((fn) => {
			unlistenThumbs = fn;
		});

		let unlistenUpdated: (() => void) | null = null;
		listen<MonitoredPlaylist>('manager-playlist-updated', (event) => {
			const updated = event.payload;
			playlists = playlists.map((p) => (p.id === updated.id ? updated : p));
			toast.success('Playlist ready', { description: `${updated.name} (${updated.total_entries} tracks)` });
		}).then((fn) => {
			unlistenUpdated = fn;
		});

		let unlistenAddError: (() => void) | null = null;
		listen<{ url: string; error: string; playlist_id: number }>('manager-playlist-error', (event) => {
			playlists = playlists.filter((p) => p.id !== event.payload.playlist_id);
			toast.error('Failed to add playlist', { description: event.payload.error });
		}).then((fn) => {
			unlistenAddError = fn;
		});

		return () => {
			unlistenEntry?.();
			unlistenThumbs?.();
			unlistenUpdated?.();
			unlistenAddError?.();
		};
	});

	// --- Download actions ---
	async function handleSubmit() {
		const url = urlInput.trim();
		if (!url) return;
		submitting = true;
		try {
			const download = await startDownload(url);
			downloadStore.addDownload(download);
			urlInput = '';
			toast.success('Download started', { description: download.title || url });
		} catch (e) {
			toast.error('Failed to start download', { description: String(e) });
		} finally {
			submitting = false;
		}
	}

	async function handleCancel(id: number) {
		try {
			await cancelDownload(id);
			downloadStore.removeDownload(id);
			toast.info('Download cancelled');
		} catch (e) {
			toast.error('Failed to cancel', { description: String(e) });
		}
	}

	async function handleRetry(id: number) {
		try {
			const dl = await retryDownload(id);
			downloadStore.addDownload(dl);
			toast.success('Download retried');
		} catch (e) {
			toast.error('Failed to retry', { description: String(e) });
		}
	}

	// --- History actions ---
	async function loadHistory() {
		try {
			const offset = historyPage * historyPageSize;
			const [data, total] = await getDownloadHistory(offset, historyPageSize);
			history = data;
			historyTotal = total;
			historyLoaded = true;
		} catch (e) {
			console.error('Failed to load history:', e);
		}
	}

	async function handleClearHistory() {
		try {
			await clearHistory();
			history = [];
			historyTotal = 0;
			downloadStore.clearCompleted();
			toast.success('History cleared');
		} catch {
			toast.error('Failed to clear history');
		}
	}

	// --- Playlist actions ---
	async function loadPlaylists() {
		try {
			playlists = await getMonitoredPlaylists();
		} catch {
			// Ignore on first load
		}
	}

	async function handleAddPlaylist() {
		const url = playlistUrlInput.trim();
		if (!url) return;
		addingPlaylist = true;
		try {
			const pl = await addPlaylist(url);
			playlists = [pl, ...playlists];
			playlistUrlInput = '';
			toast.info('Fetching tracks...', { description: 'Importing in the background. You can navigate away.' });
		} catch (e) {
			toast.error('Failed to add playlist', { description: String(e) });
		} finally {
			addingPlaylist = false;
		}
	}

	async function handleSync(playlistId: number) {
		syncingIds = new Set([...syncingIds, playlistId]);
		try {
			const result = await syncPlaylist(playlistId);
			await loadPlaylists();
			if (result.new_count > 0) {
				toast.success('New tracks found', { description: `${result.new_count} new tracks` });
			} else {
				toast.info('Playlist is up to date');
			}
			if (selectedPlaylist?.id === playlistId) {
				await loadEntries(playlistId);
			}
		} catch (e) {
			toast.error('Sync failed', { description: String(e) });
		} finally {
			const next = new Set(syncingIds);
			next.delete(playlistId);
			syncingIds = next;
		}
	}

	async function handleSyncAll() {
		syncingAll = true;
		syncAllProgress = { current: 0, total: playlists.length, name: '' };
		try {
			let totalNew = 0;
			for (let i = 0; i < playlists.length; i++) {
				const pl = playlists[i];
				syncAllProgress = { current: i + 1, total: playlists.length, name: pl.name };
				try {
					const result = await syncPlaylist(pl.id);
					totalNew += result.new_count;
				} catch { /* Continue with others */ }
			}
			await loadPlaylists();
			if (totalNew > 0) {
				toast.success(`Found ${totalNew} new tracks across all playlists`);
			} else {
				toast.info('All playlists are up to date');
			}
		} finally {
			syncingAll = false;
		}
	}

	async function handleRemovePlaylist(playlistId: number) {
		try {
			await removePlaylist(playlistId);
			playlists = playlists.filter((p) => p.id !== playlistId);
			if (selectedPlaylist?.id === playlistId) {
				selectedPlaylist = null;
				selectedEntries = [];
			}
			toast.success('Playlist removed');
		} catch (e) {
			toast.error('Failed to remove', { description: String(e) });
		}
	}

	async function openPlaylist(pl: MonitoredPlaylist) {
		selectedPlaylist = pl;
		entriesPage = 0;
		await loadEntries(pl.id);
	}

	async function loadEntries(playlistId: number) {
		loadingEntries = true;
		try {
			selectedEntries = await getEntries(playlistId);
		} catch (e) {
			toast.error('Failed to load entries', { description: String(e) });
		} finally {
			loadingEntries = false;
		}
	}

	function closePlaylistDetail() {
		selectedPlaylist = null;
		selectedEntries = [];
		entriesPage = 0;
	}

	async function handleDownloadEntry(entryId: number) {
		downloadingEntryIds = new Set([...downloadingEntryIds, entryId]);
		try {
			const dl = await downloadEntry(entryId);
			downloadStore.addDownload(dl);
			selectedEntries = selectedEntries.map((e) =>
				e.id === entryId ? { ...e, status: 'queued' as const, download_id: dl.id } : e
			);
			toast.success('Download started');
		} catch (e) {
			toast.error('Download failed', { description: String(e) });
		} finally {
			const next = new Set(downloadingEntryIds);
			next.delete(entryId);
			downloadingEntryIds = next;
		}
	}

	async function handleDownloadAllNew(playlistId: number) {
		try {
			const result = await downloadNew(playlistId);
			if (selectedPlaylist?.id === playlistId) await loadEntries(playlistId);
			await loadPlaylists();
			toast.success(`Queued ${result.queued} downloads`, {
				description: 'Downloads will process automatically (3 at a time)',
			});
		} catch (e) {
			toast.error('Failed to start downloads', { description: String(e) });
		}
	}

	async function handleCancelEntry(entryId: number) {
		try {
			await cancelEntry(entryId);
			selectedEntries = selectedEntries.map((e) =>
				e.id === entryId ? { ...e, status: 'new' as const, download_id: null } : e
			);
			await loadPlaylists();
			await downloadStore.refresh();
		} catch (e) {
			toast.error('Failed to cancel', { description: String(e) });
		}
	}

	async function handleCancelAll(playlistId: number) {
		try {
			const count = await cancelAllDownloads(playlistId);
			if (selectedPlaylist?.id === playlistId) await loadEntries(playlistId);
			await loadPlaylists();
			await downloadStore.refresh();
			toast.success(`Cancelled ${count} downloads`);
		} catch (e) {
			toast.error('Failed to cancel downloads', { description: String(e) });
		}
	}

	async function handleRetryAllFailed() {
		const failed = selectedEntries.filter((e) => e.status === 'failed');
		if (failed.length === 0) return;
		retryingAll = true;
		try {
			for (const entry of failed) await handleDownloadEntry(entry.id);
			toast.success(`Retrying ${failed.length} failed downloads`);
		} catch (e) {
			toast.error('Failed to retry some downloads', { description: String(e) });
		} finally {
			retryingAll = false;
		}
	}

	async function handleSkipEntry(entryId: number) {
		try {
			await skipEntry(entryId);
			selectedEntries = selectedEntries.map((e) =>
				e.id === entryId ? { ...e, status: 'skipped' as const } : e
			);
		} catch {
			toast.error('Failed to skip entry');
		}
	}

	// --- Derived state ---
	const activeDownloads = $derived(
		downloadStore.downloads.filter(
			(d) => d.status === 'queued' || d.status === 'downloading' || d.status === 'processing'
		)
	);
	const allCompletedDownloads = $derived(
		downloadStore.downloads.filter(
			(d) => d.status === 'completed' || d.status === 'failed' || d.status === 'cancelled'
		)
	);
	const totalNewAcrossPlaylists = $derived(playlists.reduce((sum, p) => sum + p.new_count, 0));

	// Cache album names for grouped downloads
	let albumNames: Record<number, string> = $state({});
	const groupedActiveDownloads = $derived(groupDownloadsByAlbum(activeDownloads, albumNames));

	$effect(() => {
		const albumIds = new Set<number>();
		for (const dl of activeDownloads) {
			if (dl.target_album_id && !albumNames[dl.target_album_id]) {
				albumIds.add(dl.target_album_id);
			}
		}
		if (albumIds.size > 0) {
			import('$lib/api/library').then(({ getAlbum }) => {
				for (const id of albumIds) {
					getAlbum(id).then((album) => {
						if (album) albumNames = { ...albumNames, [id]: album.title };
					});
				}
			});
		}
	});

	// Auto-load history when the history tab is selected
	$effect(() => {
		if (activeTab === 'history' && !historyLoaded) loadHistory();
	});
</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
	<div>
		<h1 class="text-2xl font-bold tracking-tight">Manager</h1>
		<p class="text-sm text-muted-foreground/70 mt-0.5">
			Track playlists and download music
		</p>
	</div>

	<!-- Setup state -->
	{#if depsChecking || setupInProgress}
		<div class="flex flex-col items-center justify-center gap-4 rounded-xl border border-border p-10">
			<PackageOpen class="size-10 text-muted-foreground" />
			<div class="text-center space-y-3 w-full max-w-md">
				<p class="font-medium">{setupMessage}</p>
				{#if setupInProgress && setupProgress > 0}
					<Progress value={setupProgress} class="w-full" />
					<p class="text-xs text-muted-foreground">{Math.round(setupProgress)}%</p>
				{:else}
					<div class="flex justify-center">
						<Loader2 class="size-5 animate-spin text-muted-foreground" />
					</div>
				{/if}
			</div>
		</div>
	{:else if setupError}
		<div class="flex items-start gap-3 rounded-xl border border-destructive/30 bg-destructive/5 p-4">
			<AlertCircle class="size-5 text-destructive shrink-0 mt-0.5" />
			<div class="flex-1 min-w-0">
				<p class="font-medium text-destructive">Setup failed</p>
				<p class="text-sm text-muted-foreground mt-1">{setupError}</p>
			</div>
			<Button variant="outline" size="sm" onclick={() => depsStore.init(true)} class="shrink-0 gap-1.5">
				<RotateCcw class="size-3" />
				Retry
			</Button>
		</div>
	{:else if !depsReady}
		<div class="flex items-start gap-3 rounded-xl border border-destructive/30 bg-destructive/5 p-4">
			<AlertCircle class="size-5 text-destructive shrink-0 mt-0.5" />
			<div class="flex-1 min-w-0">
				<p class="font-medium text-destructive">Dependencies not available</p>
				<p class="text-sm text-muted-foreground mt-1">
					{#if !depsStatus?.ytdlp_available}yt-dlp{/if}
					{#if !depsStatus?.ytdlp_available && !depsStatus?.ffmpeg_available} and {/if}
					{#if !depsStatus?.ffmpeg_available}ffmpeg{/if}
					could not be set up automatically.
				</p>
			</div>
			<Button variant="outline" size="sm" onclick={() => depsStore.init(true)} class="shrink-0 gap-1.5">
				<RotateCcw class="size-3" />
				Retry
			</Button>
		</div>
	{:else}
		<Tabs.Root bind:value={activeTab}>
			<div class="flex items-center justify-between">
				<Tabs.List>
					<Tabs.Trigger value="playlists" class="gap-1.5">
						<ListMusic class="size-4" />
						Playlists
						{#if totalNewAcrossPlaylists > 0}
							<Badge variant="default" class="ml-1 h-5 min-w-5 px-1 text-xs">{totalNewAcrossPlaylists}</Badge>
						{/if}
					</Tabs.Trigger>
					<Tabs.Trigger value="downloads" class="gap-1.5">
						<Download class="size-4" />
						Downloads
						{#if activeDownloads.length > 0}
							<Badge variant="secondary" class="ml-1 h-5 min-w-5 px-1 text-xs">{activeDownloads.length}</Badge>
						{/if}
					</Tabs.Trigger>
					<Tabs.Trigger value="history" class="gap-1.5">
						<Clock class="size-4" />
						History
					</Tabs.Trigger>
				</Tabs.List>

				{#if depsStatus?.ytdlp_version}
					<span class="text-xs text-muted-foreground font-mono opacity-60">yt-dlp {depsStatus.ytdlp_version}</span>
				{/if}
			</div>

			<Tabs.Content value="playlists" class="space-y-5 mt-4">
				<ManagerPlaylistsTab
					{playlists}
					bind:playlistUrlInput
					{addingPlaylist}
					{syncingAll}
					{syncAllProgress}
					{syncingIds}
					bind:selectedPlaylist
					{selectedEntries}
					{loadingEntries}
					{downloadingEntryIds}
					bind:entriesPage
					{entriesPageSize}
					{retryingAll}
					{activeDownloads}
					{totalNewAcrossPlaylists}
					onaddPlaylist={handleAddPlaylist}
					onsyncPlaylist={handleSync}
					onsyncAll={handleSyncAll}
					onremovePlaylist={handleRemovePlaylist}
					onopenPlaylist={openPlaylist}
					oncloseDetail={closePlaylistDetail}
					ondownloadEntry={handleDownloadEntry}
					ondownloadAllNew={handleDownloadAllNew}
					oncancelEntry={handleCancelEntry}
					oncancelAll={handleCancelAll}
					onskipEntry={handleSkipEntry}
					onretryAllFailed={handleRetryAllFailed}
				/>
			</Tabs.Content>

			<Tabs.Content value="downloads" class="space-y-5 mt-4">
				<ManagerDownloadsTab
					bind:urlInput
					{submitting}
					{activeDownloads}
					{allCompletedDownloads}
					{groupedActiveDownloads}
					bind:completedPage
					{completedPageSize}
					onsubmit={handleSubmit}
					oncancel={handleCancel}
					onretry={handleRetry}
				/>
			</Tabs.Content>

			<Tabs.Content value="history" class="space-y-4 mt-4">
				<ManagerHistoryTab
					{history}
					{historyTotal}
					{historyLoaded}
					bind:historyPage
					{historyPageSize}
					onloadHistory={loadHistory}
					onclearHistory={handleClearHistory}
				/>
			</Tabs.Content>
		</Tabs.Root>
	{/if}
</div>
