<script lang="ts">
	import { page } from '$app/stores';
	import { listen } from '@tauri-apps/api/event';
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Progress } from '$lib/components/ui/progress';
	import * as Tabs from '$lib/components/ui/tabs';

	import {
		Download,
		Disc,
		Link,
		Loader2,
		CheckCircle2,
		XCircle,
		RotateCcw,
		X,
		Trash2,
		PackageOpen,
		RefreshCw,
		ListMusic,
		Plus,
		Play,
		SkipForward,
		Clock,
		ExternalLink,
		ChevronRight,
		ChevronLeft,
		ArrowDownToLine,
		Square,
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
	import { assetUrl, formatDate, formatSeconds, timeAgo, platformLabel, platformColor } from '$lib/utils/format';
	import { groupDownloadsByAlbum, type DownloadGroup } from '$lib/utils/grouping';

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

	// --- Active tab (read from URL param) ---
	const validTabs = ['playlists', 'downloads', 'history'];
	let activeTab = $state($page.url.searchParams.get('tab') || 'playlists');

	// Sync tab with URL param changes
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
			// Reload entries to get local thumbnail paths
			if (selectedPlaylistId === event.payload.playlist_id) {
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
			// Remove the failed placeholder playlist
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

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') handleSubmit();
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

	function nextHistoryPage() {
		const maxPage = Math.ceil(historyTotal / historyPageSize) - 1;
		if (historyPage < maxPage) {
			historyPage++;
			loadHistory();
		}
	}

	function prevHistoryPage() {
		if (historyPage > 0) {
			historyPage--;
			loadHistory();
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

	function handlePlaylistKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') handleAddPlaylist();
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
				} catch {
					// Continue with others
				}
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

	function nextEntriesPage() {
		const maxPage = Math.ceil(sortedEntries.length / entriesPageSize) - 1;
		if (entriesPage < maxPage) {
			entriesPage++;
		}
	}

	function prevEntriesPage() {
		if (entriesPage > 0) {
			entriesPage--;
		}
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
			if (selectedPlaylist?.id === playlistId) {
				await loadEntries(playlistId);
			}
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
			if (selectedPlaylist?.id === playlistId) {
				await loadEntries(playlistId);
			}
			await loadPlaylists();
			await downloadStore.refresh();
			toast.success(`Cancelled ${count} downloads`);
		} catch (e) {
			toast.error('Failed to cancel downloads', { description: String(e) });
		}
	}

	let retryingAll = $state(false);

	async function handleRetryAllFailed() {
		const failed = selectedEntries.filter((e) => e.status === 'failed');
		if (failed.length === 0) return;
		retryingAll = true;
		try {
			for (const entry of failed) {
				await handleDownloadEntry(entry.id);
			}
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

	const completedTotalPages = $derived(Math.ceil(allCompletedDownloads.length / completedPageSize));
	const completedDownloads = $derived(
		allCompletedDownloads.slice(completedPage * completedPageSize, (completedPage + 1) * completedPageSize)
	);

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
						if (album) {
							albumNames = { ...albumNames, [id]: album.title };
						}
					});
				}
			});
		}
	});

	// Sort entries: new/queued/downloading first, then failed, then downloaded/skipped
	const statusOrder: Record<string, number> = {
		new: 0,
		queued: 1,
		downloading: 2,
		failed: 3,
		downloaded: 4,
		skipped: 5,
	};
	const sortedEntries = $derived(
		[...selectedEntries].sort((a, b) => (statusOrder[a.status] ?? 9) - (statusOrder[b.status] ?? 9))
	);

	const paginatedEntries = $derived(
		sortedEntries.slice(entriesPage * entriesPageSize, (entriesPage + 1) * entriesPageSize)
	);

	const newEntries = $derived(selectedEntries.filter((e) => e.status === 'new'));
	const failedEntries = $derived(selectedEntries.filter((e) => e.status === 'failed'));
	const downloadedEntries = $derived(selectedEntries.filter((e) => e.status === 'downloaded'));
	const queuedEntries = $derived(selectedEntries.filter((e) => e.status === 'queued'));
	const downloadingEntries = $derived(selectedEntries.filter((e) => e.status === 'downloading'));
	const totalNewAcrossPlaylists = $derived(playlists.reduce((sum, p) => sum + p.new_count, 0));

	// Auto-load history when the history tab is selected
	$effect(() => {
		if (activeTab === 'history' && !historyLoaded) {
			loadHistory();
		}
	});

	function entryStatusColor(status: string): string {
		switch (status) {
			case 'downloaded': return 'text-green-500';
			case 'new': return 'text-blue-400';
			case 'queued': return 'text-muted-foreground';
			case 'downloading': return 'text-blue-400';
			case 'failed': return 'text-destructive';
			case 'skipped': return 'text-muted-foreground/60';
			default: return 'text-muted-foreground';
		}
	}

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
		<!-- Main content - tabbed interface -->
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

			<!-- ==================== PLAYLISTS TAB ==================== -->
			<Tabs.Content value="playlists" class="space-y-5 mt-4">
				{#if selectedPlaylist}
					<!-- Playlist detail view -->
					<div class="space-y-4">
						<!-- Detail header -->
						<div class="flex items-start gap-4">
							<Button variant="ghost" size="icon" onclick={closePlaylistDetail} class="shrink-0 mt-0.5 rounded-full">
								<ChevronLeft class="size-5" />
							</Button>
							<div class="flex-1 min-w-0">
								<div class="flex items-center gap-2 mb-1">
									<h2 class="text-xl font-semibold truncate">{selectedPlaylist.name}</h2>
									<Badge variant={platformColor(selectedPlaylist.source_platform ?? 'other')} class="text-xs shrink-0">
										{platformLabel(selectedPlaylist.source_platform ?? 'other')}
									</Badge>
								</div>
								<!-- Stats row -->
								<div class="flex items-center gap-2 text-xs text-muted-foreground flex-wrap">
									<span>{selectedEntries.length} total</span>
									{#if downloadedEntries.length > 0}
										<span class="opacity-40">&middot;</span>
										<span class="text-green-500">{downloadedEntries.length} downloaded</span>
									{/if}
									{#if downloadingEntries.length > 0}
										<span class="opacity-40">&middot;</span>
										<span class="text-blue-400">{downloadingEntries.length} downloading</span>
									{/if}
									{#if queuedEntries.length > 0}
										<span class="opacity-40">&middot;</span>
										<span>{queuedEntries.length} queued</span>
									{/if}
									{#if newEntries.length > 0}
										<span class="opacity-40">&middot;</span>
										<span class="text-blue-400">{newEntries.length} new</span>
									{/if}
									{#if failedEntries.length > 0}
										<span class="opacity-40">&middot;</span>
										<span class="text-destructive">{failedEntries.length} failed</span>
									{/if}
									<span class="opacity-40">&middot;</span>
									<span>Synced {timeAgo(selectedPlaylist.last_synced_at)}</span>
								</div>
								<!-- Progress bar showing completion -->
								{#if selectedEntries.length > 0}
									{@const pct = Math.round((downloadedEntries.length / selectedEntries.length) * 100)}
									<div class="flex items-center gap-2 mt-2">
										<Progress value={pct} class="h-1.5 flex-1 max-w-xs" />
										<span class="text-xs text-muted-foreground tabular-nums">{pct}%</span>
									</div>
								{/if}
							</div>
							<div class="flex items-center gap-2 shrink-0">
								{#if queuedEntries.length > 0 || downloadingEntries.length > 0}
									<Button
										variant="destructive"
										size="sm"
										onclick={() => handleCancelAll(selectedPlaylist!.id)}
										class="gap-1.5"
									>
										<Square class="size-3.5" />
										Stop all
									</Button>
								{/if}
								{#if failedEntries.length > 0}
									<Button
										variant="outline"
										size="sm"
										onclick={handleRetryAllFailed}
										disabled={retryingAll}
										class="gap-1.5"
									>
										{#if retryingAll}
											<Loader2 class="size-3.5 animate-spin" />
										{:else}
											<RotateCcw class="size-3.5" />
										{/if}
										Retry {failedEntries.length}
									</Button>
								{/if}
								{#if newEntries.length > 0}
									<Button size="sm" onclick={() => handleDownloadAllNew(selectedPlaylist!.id)} class="gap-1.5">
										<ArrowDownToLine class="size-4" />
										Download {newEntries.length} new
									</Button>
								{/if}
								<Button
									variant="outline"
									size="sm"
									onclick={() => handleSync(selectedPlaylist!.id)}
									disabled={syncingIds.has(selectedPlaylist.id)}
									class="gap-1.5"
								>
									<RefreshCw class="size-3.5 {syncingIds.has(selectedPlaylist.id) ? 'animate-spin' : ''}" />
									Sync
								</Button>
							</div>
						</div>

						{#if loadingEntries}
							<div class="flex justify-center p-12">
								<Loader2 class="size-6 animate-spin text-muted-foreground" />
							</div>
						{:else if selectedEntries.length === 0}
							<div class="flex flex-col items-center justify-center py-16 rounded-xl border border-dashed border-border/60 gap-3">
								<ListMusic class="size-8 text-muted-foreground/40" />
								<p class="text-muted-foreground text-sm">No tracks in this playlist</p>
							</div>
						{:else}
							<!-- Entries table -->
							<div class="rounded-xl border border-border/60 overflow-hidden">
								<!-- Table header -->
								<div class="border-b border-border bg-muted/30 text-xs text-muted-foreground uppercase tracking-wider flex items-center px-4 py-2.5">
									<div class="w-8 text-center">#</div>
									<div class="w-7"></div>
									<div class="flex-1 pl-3">Title</div>
									<div class="w-24 text-right">Duration</div>
									<div class="w-24 text-center">Status</div>
									<div class="w-20"></div>
								</div>
								<!-- Table body -->
								<div class="divide-y divide-border/40">
									{#each paginatedEntries as entry, i (entry.id)}
										<div class="flex items-center px-4 py-2.5 hover:bg-muted/30 transition-colors group">
											<!-- Index -->
											<div class="w-8 text-center text-xs text-muted-foreground tabular-nums">
												{entriesPage * entriesPageSize + i + 1}
											</div>
											<!-- Status icon -->
											<div class="w-7 flex items-center justify-center">
												{#if entry.status === 'downloaded'}
													<CheckCircle2 class="size-4 text-green-500" />
												{:else if entry.status === 'new'}
													<div class="size-2.5 rounded-full bg-blue-400"></div>
												{:else if entry.status === 'queued'}
													<Clock class="size-3.5 text-muted-foreground" />
												{:else if entry.status === 'downloading'}
													<Loader2 class="size-3.5 animate-spin text-blue-400" />
												{:else if entry.status === 'failed'}
													<XCircle class="size-4 text-destructive" />
												{:else if entry.status === 'skipped'}
													<SkipForward class="size-3.5 text-muted-foreground/50" />
												{/if}
											</div>
											<!-- Thumbnail -->
											<div class="w-9 h-9 rounded overflow-hidden flex-shrink-0 ml-2 bg-muted/30">
												{#if entry.thumbnail}
													<img
														src={entry.thumbnail.startsWith('/') ? assetUrl(entry.thumbnail) : entry.thumbnail}
														alt=""
														class="w-full h-full object-cover"
													/>
												{/if}
											</div>
											<!-- Title & artist -->
											<div class="flex-1 min-w-0 pl-3">
												<p class="text-sm truncate {entry.status === 'skipped' ? 'text-muted-foreground/50 line-through' : ''}">{entry.title || entry.source_url}</p>
												{#if entry.artist}
													<p class="text-xs text-muted-foreground truncate mt-0.5">{entry.artist}</p>
												{/if}
											</div>
											<!-- Duration -->
											<div class="w-24 text-right text-xs text-muted-foreground tabular-nums">
												{entry.duration_seconds ? formatSeconds(entry.duration_seconds) : '--:--'}
											</div>
											<!-- Status badge -->
											<div class="w-24 flex justify-center">
												<Badge variant="outline" class="text-xs capitalize {entryStatusColor(entry.status)}">
													{entry.status}
												</Badge>
											</div>
											<!-- Actions -->
											<div class="w-20 flex items-center justify-end gap-1">
												{#if entry.status === 'new'}
													<Button
														variant="ghost"
														size="icon-sm"
														class="size-7 opacity-0 group-hover:opacity-100 transition-opacity"
														onclick={() => handleDownloadEntry(entry.id)}
														disabled={downloadingEntryIds.has(entry.id)}
													>
														{#if downloadingEntryIds.has(entry.id)}
															<Loader2 class="size-3.5 animate-spin" />
														{:else}
															<Download class="size-3.5" />
														{/if}
													</Button>
													<Button
														variant="ghost"
														size="icon-sm"
														class="size-7 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground"
														onclick={() => handleSkipEntry(entry.id)}
													>
														<SkipForward class="size-3.5" />
													</Button>
												{:else if entry.status === 'queued' || entry.status === 'downloading'}
													<Button
														variant="ghost"
														size="icon-sm"
														class="size-7 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground"
														onclick={() => handleCancelEntry(entry.id)}
													>
														<X class="size-3.5" />
													</Button>
												{:else if entry.status === 'failed'}
													<Button
														variant="ghost"
														size="icon-sm"
														class="size-7 text-destructive"
														onclick={() => handleDownloadEntry(entry.id)}
													>
														<RotateCcw class="size-3.5" />
													</Button>
												{/if}
											</div>
										</div>
									{/each}
								</div>
							</div>

							{#if sortedEntries.length > entriesPageSize}
								{@const totalPages = Math.ceil(sortedEntries.length / entriesPageSize)}
								<div class="flex items-center justify-center gap-3 pt-2">
									<Button variant="outline" size="sm" onclick={prevEntriesPage} disabled={entriesPage === 0} class="gap-1">
										<ChevronLeft class="size-4" />
										Previous
									</Button>
									<span class="text-sm text-muted-foreground tabular-nums">
										{entriesPage + 1} / {totalPages}
									</span>
									<Button variant="outline" size="sm" onclick={nextEntriesPage} disabled={entriesPage >= totalPages - 1} class="gap-1">
										Next
										<ChevronRight class="size-4" />
									</Button>
								</div>
							{/if}
						{/if}
					</div>
				{:else}
					<!-- Playlist list view -->

					<!-- Action banner: new tracks ready -->
					{#if totalNewAcrossPlaylists > 0}
						{@const playlistsWithNew = playlists.filter(p => p.new_count > 0)}
						<div class="flex items-center gap-4 rounded-xl bg-primary/8 border border-primary/20 px-5 py-3.5">
							<div class="flex items-center gap-3 flex-1 min-w-0">
								<div class="flex items-center justify-center size-10 rounded-full bg-primary/15 shrink-0">
									<ArrowDownToLine class="size-5 text-primary" />
								</div>
								<div>
									<p class="text-sm font-medium">
										{totalNewAcrossPlaylists} new track{totalNewAcrossPlaylists !== 1 ? 's' : ''} found
									</p>
									<p class="text-xs text-muted-foreground mt-0.5">
										Across {playlistsWithNew.length} playlist{playlistsWithNew.length !== 1 ? 's' : ''}
									</p>
								</div>
							</div>
							<Button
								size="sm"
								class="gap-1.5 shrink-0"
								onclick={() => { for (const pl of playlistsWithNew) handleDownloadAllNew(pl.id); }}
							>
								<ArrowDownToLine class="size-3.5" />
								Download all
							</Button>
						</div>
					{/if}

					<!-- Active downloads indicator -->
					{#if activeDownloads.length > 0}
						<div class="flex items-center gap-3 rounded-lg bg-muted/30 px-4 py-2.5">
							<span class="relative flex size-2 shrink-0">
								<span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-blue-400 opacity-75"></span>
								<span class="relative inline-flex rounded-full size-2 bg-blue-400"></span>
							</span>
							<span class="text-sm text-muted-foreground">
								{activeDownloads.length} download{activeDownloads.length !== 1 ? 's' : ''} in progress
							</span>
						</div>
					{/if}

					<!-- Add playlist + actions row -->
					<div class="flex gap-2">
						<div class="relative flex-1">
							<ListMusic class="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
							<Input
								placeholder="Paste a playlist URL..."
								class="pl-10"
								bind:value={playlistUrlInput}
								onkeydown={handlePlaylistKeydown}
							/>
						</div>
						<Button
							onclick={handleAddPlaylist}
							disabled={!playlistUrlInput.trim() || addingPlaylist}
							class="gap-1.5"
						>
							{#if addingPlaylist}
								<Loader2 class="size-4 animate-spin" />
							{:else}
								<Plus class="size-4" />
							{/if}
							Add
						</Button>
						{#if playlists.length > 0}
							<Button
								variant="outline"
								onclick={handleSyncAll}
								disabled={syncingAll}
								class="gap-1.5"
							>
								{#if syncingAll}
									<Loader2 class="size-4 animate-spin" />
									{syncAllProgress.current}/{syncAllProgress.total}
								{:else}
									<RefreshCw class="size-4" />
									Sync all
								{/if}
							</Button>
						{/if}
					</div>

					{#if playlists.length === 0}
						<div class="flex flex-col items-center justify-center py-20 rounded-xl border border-dashed border-border/40 gap-5">
							<div class="size-16 rounded-2xl bg-muted/30 flex items-center justify-center">
								<ListMusic class="size-8 text-muted-foreground/30" />
							</div>
							<div class="text-center space-y-1.5 max-w-sm">
								<p class="font-medium">No playlists yet</p>
								<p class="text-muted-foreground/50 text-sm leading-relaxed">
									Paste a playlist URL above to start monitoring. Supports YouTube, SoundCloud, Bandcamp, Spotify, and more.
								</p>
							</div>
						</div>
					{:else}
						<!-- Playlist grid -->
						<div class="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 gap-3">
							{#each playlists as pl (pl.id)}
								{@const pct = pl.total_entries > 0 ? Math.round((pl.downloaded_count / pl.total_entries) * 100) : 0}
								<div
									class="group rounded-xl border border-border/40 bg-card hover:bg-muted/20 transition-all cursor-pointer overflow-hidden"
									role="button"
									tabindex="0"
									onclick={() => openPlaylist(pl)}
									onkeydown={(e) => { if (e.key === 'Enter') openPlaylist(pl); }}
								>
									<!-- Cover art -->
									<div class="aspect-[16/10] bg-muted/40 relative overflow-hidden">
										{#if pl.total_entries === 0}
											<div class="size-full bg-gradient-to-br from-muted/80 to-muted/20 flex flex-col items-center justify-center gap-2">
												<Loader2 class="size-8 text-muted-foreground/40 animate-spin" />
												<span class="text-[11px] text-muted-foreground/60">Fetching tracks...</span>
											</div>
										{:else if pl.cover_art_path}
											<img
												src={assetUrl(pl.cover_art_path)}
												alt={pl.name}
												class="size-full object-cover group-hover:scale-105 transition-transform duration-300"
												loading="lazy"
											/>
										{:else}
											<div class="size-full bg-gradient-to-br from-muted/80 to-muted/20 flex items-center justify-center">
												<ListMusic class="size-10 text-muted-foreground/20" />
											</div>
										{/if}
										<!-- Overlay badges -->
										{#if pl.new_count > 0}
											<div class="absolute top-2 right-2">
												<span class="inline-flex items-center gap-1 rounded-full bg-primary px-2 py-0.5 text-[11px] font-semibold text-primary-foreground shadow-sm">
													{pl.new_count} new
												</span>
											</div>
										{/if}
										{#if pl.active_count > 0}
											<div class="absolute top-2 left-2">
												<span class="inline-flex items-center gap-1 rounded-full bg-black/60 backdrop-blur-sm px-2 py-0.5 text-[11px] text-white">
													<Loader2 class="size-3 animate-spin" />
													{pl.active_count}
												</span>
											</div>
										{/if}
										<!-- Progress bar at bottom of image -->
										{#if pct > 0 && pct < 100}
											<div class="absolute bottom-0 left-0 right-0 h-1 bg-black/30">
												<div class="h-full bg-primary transition-all" style="width: {pct}%"></div>
											</div>
										{:else if pct === 100}
											<div class="absolute bottom-0 left-0 right-0 h-1 bg-green-500"></div>
										{/if}
									</div>
									<!-- Card body -->
									<div class="p-3 space-y-1.5">
										<div class="flex items-start justify-between gap-2">
											<p class="text-sm font-medium leading-snug line-clamp-2">{pl.name}</p>
											<!-- Hover actions -->
											<div class="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity -mt-0.5 -mr-1">
												<Button
													variant="ghost"
													size="icon-sm"
													class="size-6"
													onclick={(e) => { e.stopPropagation(); handleSync(pl.id); }}
													disabled={syncingIds.has(pl.id)}
												>
													<RefreshCw class="size-3 {syncingIds.has(pl.id) ? 'animate-spin' : ''}" />
												</Button>
												<Button
													variant="ghost"
													size="icon-sm"
													class="size-6 text-muted-foreground hover:text-destructive"
													onclick={(e) => { e.stopPropagation(); handleRemovePlaylist(pl.id); }}
												>
													<Trash2 class="size-3" />
												</Button>
											</div>
										</div>
										<div class="flex items-center gap-1.5 text-[11px] text-muted-foreground">
											<!-- Platform dot -->
											<span class="size-1.5 rounded-full shrink-0 {
												pl.source_platform === 'youtube' ? 'bg-red-400' :
												pl.source_platform === 'soundcloud' ? 'bg-orange-400' :
												pl.source_platform === 'spotify' ? 'bg-green-400' :
												pl.source_platform === 'bandcamp' ? 'bg-cyan-400' :
												'bg-muted-foreground/40'
											}"></span>
											<span>{pl.downloaded_count}/{pl.total_entries} tracks</span>
											{#if pl.last_synced_at}
												<span class="opacity-40">&middot;</span>
												<span>{timeAgo(pl.last_synced_at)}</span>
											{/if}
										</div>
										<!-- Download CTA -->
										{#if pl.new_count > 0}
											<Button
												size="sm"
												class="w-full gap-1.5 h-7 text-xs mt-1"
												onclick={(e) => { e.stopPropagation(); handleDownloadAllNew(pl.id); }}
											>
												<ArrowDownToLine class="size-3" />
												Download {pl.new_count} new
											</Button>
										{/if}
									</div>
								</div>
							{/each}
						</div>
						<!-- Single-track items collapsed into a compact list -->
						{@const singles = playlists.filter(pl => pl.total_entries <= 1)}
						{#if singles.length > 0}
							<div class="space-y-1.5">
								<p class="text-xs font-medium uppercase tracking-wider text-muted-foreground/50 px-1">
									Singles ({singles.length})
								</p>
								<div class="rounded-lg border border-border/30 divide-y divide-border/20 overflow-hidden">
									{#each singles as pl (pl.id)}
										<div
											class="flex items-center gap-3 px-3 py-2 hover:bg-muted/20 transition-colors cursor-pointer group text-sm"
											role="button"
											tabindex="0"
											onclick={() => openPlaylist(pl)}
											onkeydown={(e) => { if (e.key === 'Enter') openPlaylist(pl); }}
										>
											<span class="size-1.5 rounded-full shrink-0 {
												pl.source_platform === 'youtube' ? 'bg-red-400' :
												pl.source_platform === 'soundcloud' ? 'bg-orange-400' :
												pl.source_platform === 'spotify' ? 'bg-green-400' :
												pl.source_platform === 'bandcamp' ? 'bg-cyan-400' :
												'bg-muted-foreground/40'
											}"></span>
											<span class="truncate flex-1">{pl.name}</span>
											{#if pl.downloaded_count > 0}
												<CheckCircle2 class="size-3.5 text-green-500/60 shrink-0" />
											{:else if pl.new_count > 0}
												<Button
													variant="ghost"
													size="icon-sm"
													class="size-6 shrink-0"
													onclick={(e) => { e.stopPropagation(); handleDownloadAllNew(pl.id); }}
												>
													<ArrowDownToLine class="size-3" />
												</Button>
											{/if}
											<Button
												variant="ghost"
												size="icon-sm"
												class="size-6 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground hover:text-destructive"
												onclick={(e) => { e.stopPropagation(); handleRemovePlaylist(pl.id); }}
											>
												<Trash2 class="size-3" />
											</Button>
										</div>
									{/each}
								</div>
							</div>
						{/if}
					{/if}
				{/if}
			</Tabs.Content>

			<!-- ==================== DOWNLOADS TAB ==================== -->
			<Tabs.Content value="downloads" class="space-y-5 mt-4">
				<!-- URL Input -->
				<div class="flex gap-2 max-w-2xl">
					<div class="relative flex-1">
						<Link class="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
						<Input
							placeholder="Paste a URL (YouTube, SoundCloud, Bandcamp...)"
							class="pl-10"
							bind:value={urlInput}
							onkeydown={handleKeydown}
						/>
					</div>
					<Button onclick={handleSubmit} disabled={!urlInput.trim() || submitting} class="gap-1.5">
						{#if submitting}
							<Loader2 class="size-4 animate-spin" />
						{:else}
							<Download class="size-4" />
						{/if}
						Download
					</Button>
				</div>

				<!-- Active Downloads -->
				{#if activeDownloads.length > 0}
					<div class="space-y-3">
						<h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
							Active ({activeDownloads.length})
						</h2>
						{#each groupedActiveDownloads as group}
							{#if group.albumId}
								<!-- Album group -->
								<div class="rounded-xl border border-border/60 overflow-hidden">
									<div class="flex items-center gap-3 px-4 py-2.5 bg-muted/30 border-b border-border/40">
										<Disc class="size-4 text-muted-foreground shrink-0" />
										<span class="text-sm font-medium truncate">{group.albumTitle}</span>
										<Badge variant="secondary" class="text-xs shrink-0 ml-auto">
											{group.downloads.length} track{group.downloads.length !== 1 ? 's' : ''}
										</Badge>
									</div>
									<!-- Album group table header -->
									<div class="border-b border-border/30 bg-muted/15 text-[10px] text-muted-foreground/70 uppercase tracking-wider flex items-center px-4 py-1.5">
										<div class="w-7"></div>
										<div class="flex-1 pl-3">Title</div>
										<div class="w-24 text-center">Status</div>
										<div class="w-8"></div>
									</div>
									<div class="divide-y divide-border/20">
										{#each group.downloads.slice(0, 30) as dl (dl.id)}
											<div class="flex items-center px-4 py-2.5 hover:bg-muted/20 transition-colors group">
												<!-- Status icon -->
												<div class="w-7 flex items-center justify-center shrink-0">
													{#if dl.status === 'downloading'}
														<Loader2 class="size-3.5 animate-spin text-blue-400" />
													{:else if dl.status === 'processing'}
														<Loader2 class="size-3.5 animate-spin text-green-500" />
													{:else}
														<Clock class="size-3.5 text-muted-foreground/40" />
													{/if}
												</div>
												<!-- Title + progress -->
												<div class="flex-1 min-w-0 pl-3">
													<div class="flex items-center gap-2">
														<p class="text-sm truncate">{dl.title || dl.url}</p>
														{#if dl.artist}
															<span class="text-xs text-muted-foreground/60 truncate shrink-0">{dl.artist}</span>
														{/if}
													</div>
													{#if dl.status === 'downloading'}
														<div class="flex items-center gap-2 mt-1">
															<Progress value={dl.progress} class="h-1 flex-1" />
															<span class="text-[10px] text-blue-400 tabular-nums shrink-0">{Math.round(dl.progress)}%</span>
														</div>
													{/if}
												</div>
												<!-- Status -->
												<div class="w-24 flex justify-center">
													{#if dl.status === 'downloading'}
														<Badge variant="outline" class="text-[10px] text-blue-400 border-blue-400/30">downloading</Badge>
													{:else if dl.status === 'processing'}
														<Badge variant="outline" class="text-[10px] text-green-500 border-green-500/30">importing</Badge>
													{:else}
														<Badge variant="outline" class="text-[10px] text-muted-foreground/60">queued</Badge>
													{/if}
												</div>
												<!-- Cancel -->
												<div class="w-8 flex justify-center">
													<Button
														variant="ghost"
														size="icon-sm"
														class="size-6 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground"
														onclick={() => handleCancel(dl.id)}
													>
														<X class="size-3.5" />
													</Button>
												</div>
											</div>
										{/each}
										{#if group.downloads.length > 30}
											<div class="text-xs text-muted-foreground/50 text-center py-2.5 bg-muted/10">
												+{group.downloads.length - 30} more queued
											</div>
										{/if}
									</div>
								</div>
							{:else}
								<!-- Ungrouped downloads table -->
								<div class="rounded-xl border border-border/60 overflow-hidden">
									<div class="divide-y divide-border/20">
										{#each group.downloads.slice(0, 30) as dl (dl.id)}
											<div class="flex items-center px-4 py-2.5 hover:bg-muted/20 transition-colors group">
												<!-- Status icon -->
												<div class="w-7 flex items-center justify-center shrink-0">
													{#if dl.status === 'downloading'}
														<Loader2 class="size-3.5 animate-spin text-blue-400" />
													{:else if dl.status === 'processing'}
														<Loader2 class="size-3.5 animate-spin text-green-500" />
													{:else}
														<Clock class="size-3.5 text-muted-foreground/40" />
													{/if}
												</div>
												<!-- Title + progress -->
												<div class="flex-1 min-w-0 pl-3">
													<div class="flex items-center gap-2">
														<p class="text-sm truncate">{dl.title || dl.url}</p>
														{#if dl.artist}
															<span class="text-xs text-muted-foreground/60 truncate shrink-0">{dl.artist}</span>
														{/if}
													</div>
													{#if dl.status === 'downloading'}
														<div class="flex items-center gap-2 mt-1">
															<Progress value={dl.progress} class="h-1 flex-1" />
															<span class="text-[10px] text-blue-400 tabular-nums shrink-0">{Math.round(dl.progress)}%</span>
														</div>
													{/if}
												</div>
												<!-- Platform -->
												<div class="w-24 flex justify-center">
													<Badge variant={platformColor(dl.platform)} class="text-[10px] shrink-0">
														{platformLabel(dl.platform)}
													</Badge>
												</div>
												<!-- Status -->
												<div class="w-24 flex justify-center">
													{#if dl.status === 'downloading'}
														<Badge variant="outline" class="text-[10px] text-blue-400 border-blue-400/30">downloading</Badge>
													{:else if dl.status === 'processing'}
														<Badge variant="outline" class="text-[10px] text-green-500 border-green-500/30">importing</Badge>
													{:else}
														<Badge variant="outline" class="text-[10px] text-muted-foreground/60">queued</Badge>
													{/if}
												</div>
												<!-- Cancel -->
												<div class="w-8 flex justify-center">
													<Button
														variant="ghost"
														size="icon-sm"
														class="size-6 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground"
														onclick={() => handleCancel(dl.id)}
													>
														<X class="size-3.5" />
													</Button>
												</div>
											</div>
										{/each}
									</div>
									{#if group.downloads.length > 30}
										<div class="text-xs text-muted-foreground/50 text-center py-2.5 border-t border-border/20 bg-muted/10">
											+{group.downloads.length - 30} more queued
										</div>
									{/if}
								</div>
							{/if}
						{/each}
					</div>
				{/if}

				<!-- Completed / Recent -->
				{#if allCompletedDownloads.length > 0}
					<div class="space-y-3">
						<h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
							Recent ({allCompletedDownloads.length})
						</h2>
						<div class="rounded-xl border border-border/60 overflow-hidden">
							<!-- Table header -->
							<div class="border-b border-border/40 bg-muted/30 text-[10px] text-muted-foreground/70 uppercase tracking-wider flex items-center px-4 py-2">
								<div class="w-7"></div>
								<div class="flex-1 pl-3">Title</div>
								<div class="w-20 text-center hidden sm:block">Format</div>
								<div class="w-24 text-center">Platform</div>
								<div class="w-24 text-center">Status</div>
								<div class="w-8"></div>
							</div>
							<div class="divide-y divide-border/20">
								{#each completedDownloads as dl (dl.id)}
									<div class="flex items-center px-4 py-2.5 hover:bg-muted/20 transition-colors group">
										<!-- Status icon -->
										<div class="w-7 flex items-center justify-center shrink-0">
											{#if dl.status === 'completed'}
												<CheckCircle2 class="size-4 text-green-500" />
											{:else if dl.status === 'failed'}
												<XCircle class="size-4 text-destructive" />
											{:else}
												<X class="size-4 text-muted-foreground/30" />
											{/if}
										</div>
										<!-- Title & artist -->
										<div class="flex-1 min-w-0 pl-3">
											<p class="text-sm truncate">{dl.title || dl.url}</p>
											<div class="flex items-center gap-2 mt-0.5">
												{#if dl.artist}
													<span class="text-xs text-muted-foreground/60 truncate">{dl.artist}</span>
												{/if}
												{#if dl.status === 'failed' && dl.error_message}
													<span class="text-xs text-destructive/80 truncate">{dl.error_message}</span>
												{/if}
											</div>
										</div>
										<!-- Format -->
										<div class="w-20 text-center hidden sm:block">
											{#if dl.status !== 'failed'}
												<span class="text-[10px] text-muted-foreground/50 font-mono uppercase">{dl.format}</span>
											{:else}
												<span class="text-[10px] text-muted-foreground/30">--</span>
											{/if}
										</div>
										<!-- Platform -->
										<div class="w-24 flex justify-center">
											<Badge variant={platformColor(dl.platform)} class="text-[10px]">
												{platformLabel(dl.platform)}
											</Badge>
										</div>
										<!-- Status -->
										<div class="w-24 flex justify-center">
											<Badge
												variant="outline"
												class="text-[10px] capitalize
													{dl.status === 'completed' ? 'text-green-500 border-green-500/30' : ''}
													{dl.status === 'failed' ? 'text-destructive border-destructive/30' : ''}
													{dl.status === 'cancelled' ? 'text-muted-foreground/50 border-border/40' : ''}"
											>
												{dl.status}
											</Badge>
										</div>
										<!-- Retry -->
										<div class="w-8 flex justify-center">
											{#if dl.status === 'failed'}
												<Button
													variant="ghost"
													size="icon-sm"
													class="size-6 text-muted-foreground hover:text-foreground"
													onclick={() => handleRetry(dl.id)}
												>
													<RotateCcw class="size-3" />
												</Button>
											{/if}
										</div>
									</div>
								{/each}
							</div>
						</div>

						{#if completedTotalPages > 1}
							<div class="flex items-center justify-center gap-3 pt-2">
								<Button variant="outline" size="sm" disabled={completedPage === 0} onclick={() => completedPage--} class="gap-1">
									<ChevronLeft class="size-4" />
									Previous
								</Button>
								<span class="text-sm text-muted-foreground tabular-nums">
									{completedPage + 1} / {completedTotalPages}
								</span>
								<Button variant="outline" size="sm" disabled={completedPage >= completedTotalPages - 1} onclick={() => completedPage++} class="gap-1">
									Next
									<ChevronRight class="size-4" />
								</Button>
							</div>
						{/if}
					</div>
				{/if}

				<!-- Empty state -->
				{#if activeDownloads.length === 0 && allCompletedDownloads.length === 0}
					<div class="flex flex-col items-center justify-center py-20 rounded-xl border border-dashed border-border/40 gap-5">
						<div class="size-16 rounded-2xl bg-muted/30 flex items-center justify-center">
							<Download class="size-8 text-muted-foreground/30" />
						</div>
						<div class="text-center space-y-1.5 max-w-sm">
							<p class="font-medium">No downloads yet</p>
							<p class="text-muted-foreground/50 text-sm leading-relaxed">Paste a URL above to download a single track or video</p>
						</div>
					</div>
				{/if}
			</Tabs.Content>

			<!-- ==================== HISTORY TAB ==================== -->
			<Tabs.Content value="history" class="space-y-4 mt-4">
				{#if !historyLoaded}
					<div class="flex justify-center p-12">
						<Loader2 class="size-6 animate-spin text-muted-foreground" />
					</div>
				{:else}
					<div class="flex items-center justify-between">
						<p class="text-sm text-muted-foreground">{historyTotal} total download{historyTotal !== 1 ? 's' : ''}</p>
						{#if history.length > 0}
							<Button variant="ghost" size="sm" onclick={handleClearHistory} class="gap-1.5 text-muted-foreground hover:text-destructive">
								<Trash2 class="size-3.5" />
								Clear history
							</Button>
						{/if}
					</div>

					{#if history.length === 0}
						<div class="flex flex-col items-center justify-center py-20 rounded-xl border border-dashed border-border/40 gap-5">
							<div class="size-16 rounded-2xl bg-muted/30 flex items-center justify-center">
								<Clock class="size-8 text-muted-foreground/30" />
							</div>
							<p class="text-muted-foreground/60">No download history</p>
						</div>
					{:else}
						<div class="rounded-xl border border-border/60 overflow-hidden">
							<!-- Table header -->
							<div class="border-b border-border/40 bg-muted/30 text-[10px] text-muted-foreground/70 uppercase tracking-wider flex items-center px-4 py-2">
								<div class="w-7"></div>
								<div class="flex-1 pl-3">Title</div>
								<div class="w-28 text-center hidden sm:block">Date</div>
								<div class="w-24 text-center">Platform</div>
								<div class="w-24 text-center">Status</div>
							</div>
							<div class="divide-y divide-border/20">
								{#each history as dl (dl.id)}
									<div class="flex items-center px-4 py-2.5 hover:bg-muted/20 transition-colors">
										<!-- Status icon -->
										<div class="w-7 flex items-center justify-center shrink-0">
											{#if dl.status === 'completed'}
												<CheckCircle2 class="size-4 text-green-500" />
											{:else if dl.status === 'failed'}
												<XCircle class="size-4 text-destructive" />
											{:else if dl.status === 'cancelled'}
												<X class="size-4 text-muted-foreground/30" />
											{:else}
												<Clock class="size-4 text-muted-foreground/40" />
											{/if}
										</div>
										<!-- Title & artist -->
										<div class="flex-1 min-w-0 pl-3">
											<p class="text-sm truncate">{dl.title || dl.url}</p>
											{#if dl.artist}
												<p class="text-xs text-muted-foreground/60 truncate mt-0.5">{dl.artist}</p>
											{/if}
										</div>
										<!-- Date -->
										<div class="w-28 text-center hidden sm:block">
											<span class="text-xs text-muted-foreground/50">{formatDate(dl.created_at)}</span>
										</div>
										<!-- Platform -->
										<div class="w-24 flex justify-center">
											<Badge variant={platformColor(dl.platform)} class="text-[10px]">
												{platformLabel(dl.platform)}
											</Badge>
										</div>
										<!-- Status -->
										<div class="w-24 flex justify-center">
											<Badge
												variant="outline"
												class="text-[10px] capitalize
													{dl.status === 'completed' ? 'text-green-500 border-green-500/30' : ''}
													{dl.status === 'failed' ? 'text-destructive border-destructive/30' : ''}
													{dl.status === 'cancelled' ? 'text-muted-foreground/50 border-border/40' : ''}"
											>
												{dl.status}
											</Badge>
										</div>
									</div>
								{/each}
							</div>
						</div>

						{#if historyTotal > historyPageSize}
							{@const totalPages = Math.ceil(historyTotal / historyPageSize)}
							<div class="flex items-center justify-center gap-3 pt-2">
								<Button variant="outline" size="sm" onclick={prevHistoryPage} disabled={historyPage === 0} class="gap-1">
									<ChevronLeft class="size-4" />
									Previous
								</Button>
								<span class="text-sm text-muted-foreground tabular-nums">
									{historyPage + 1} / {totalPages}
								</span>
								<Button variant="outline" size="sm" onclick={nextHistoryPage} disabled={historyPage >= totalPages - 1} class="gap-1">
									Next
									<ChevronRight class="size-4" />
								</Button>
							</div>
						{/if}
					{/if}
				{/if}
			</Tabs.Content>

		</Tabs.Root>
	{/if}
</div>
