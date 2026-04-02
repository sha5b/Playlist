<script lang="ts">
	import { page } from '$app/stores';
	import { listen } from '@tauri-apps/api/event';
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Progress } from '$lib/components/ui/progress';
	import * as Tabs from '$lib/components/ui/tabs';
	import * as Card from '$lib/components/ui/card';
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
	import { formatDate, formatSeconds, timeAgo, platformLabel, platformColor } from '$lib/utils/format';
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

		// Load playlists once deps are ready
		if (depsReady) {
			loadPlaylists();
		}

		// Listen for monitored entry status changes (from download completions)
		let unlistenEntry: (() => void) | null = null;
		listen<ManagerEntryEvent>('manager-entry-updated', (event) => {
			const { entry_id, status } = event.payload;
			selectedEntries = selectedEntries.map((e) =>
				e.id === entry_id ? { ...e, status: status as MonitoredEntryStatus } : e
			);
			// Refresh playlist counts
			loadPlaylists();
		}).then((fn) => {
			unlistenEntry = fn;
		});

		return () => {
			unlistenEntry?.();
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
			toast.success('Playlist added', { description: `${pl.name} (${pl.total_entries} tracks)` });
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
			// Refresh entries if viewing this playlist
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
		try {
			let totalNew = 0;
			for (const pl of playlists) {
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
		toast.info(`Loading ${pl.name}...`);
		selectedPlaylist = pl;
		entriesPage = 0;
		await loadEntries(pl.id);
		toast.success(`Loaded ${selectedEntries.length} entries`);
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
			// Update local entry state
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
			// Refresh entries to show queued status
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

	// Group active downloads: album-grouped first, then ungrouped
	const groupedActiveDownloads = $derived(groupDownloadsByAlbum(activeDownloads, albumNames));

	$effect(() => {
		const albumIds = new Set<number>();
		for (const dl of activeDownloads) {
			if (dl.target_album_id && !albumNames[dl.target_album_id]) {
				albumIds.add(dl.target_album_id);
			}
		}
		if (albumIds.size > 0) {
			// Fetch album names lazily
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
</script>

<div class="flex-1 min-h-0 overflow-y-auto space-y-6">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Manager</h1>
		<p class="text-muted-foreground mt-1">
			Track playlists, download music, and manage your sources
		</p>
	</div>

	<!-- Setup state -->
	{#if depsChecking || setupInProgress}
		<div class="flex flex-col items-center justify-center gap-4 rounded-lg border border-border p-8">
			<PackageOpen class="size-10 text-muted-foreground" />
			<div class="text-center space-y-2 w-full max-w-md">
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
		<div class="flex flex-col items-start gap-3 rounded-lg border border-destructive/50 bg-destructive/10 p-4">
			<div>
				<p class="font-medium text-destructive">Setup failed</p>
				<p class="text-sm text-muted-foreground mt-1">{setupError}</p>
			</div>
			<Button variant="outline" size="sm" onclick={() => depsStore.init(true)}>
				<RotateCcw class="size-3" />
				Try again
			</Button>
		</div>
	{:else if !depsReady}
		<div class="flex flex-col items-start gap-3 rounded-lg border border-destructive/50 bg-destructive/10 p-4">
			<div>
				<p class="font-medium text-destructive">Dependencies not available</p>
				<p class="text-sm text-muted-foreground mt-1">
					{#if !depsStatus?.ytdlp_available}yt-dlp{/if}
					{#if !depsStatus?.ytdlp_available && !depsStatus?.ffmpeg_available} and {/if}
					{#if !depsStatus?.ffmpeg_available}ffmpeg{/if}
					could not be set up automatically.
				</p>
			</div>
			<Button variant="outline" size="sm" onclick={() => depsStore.init(true)}>
				<RotateCcw class="size-3" />
				Retry setup
			</Button>
		</div>
	{:else}
		<!-- Dashboard Summary -->
		<div class="grid grid-cols-3 gap-4">
			<Card.Root>
				<Card.Content class="flex items-center gap-3 p-4">
					<div class="flex items-center justify-center size-10 rounded-lg bg-primary/10 shrink-0">
						<ListMusic class="size-5 text-primary" />
					</div>
					<div>
						<p class="text-2xl font-bold tabular-nums">{playlists.length}</p>
						<p class="text-xs text-muted-foreground">Playlists</p>
					</div>
				</Card.Content>
			</Card.Root>
			<Card.Root>
				<Card.Content class="flex items-center gap-3 p-4">
					<div class="flex items-center justify-center size-10 rounded-lg bg-blue-500/10 shrink-0">
						<Download class="size-5 text-blue-500" />
					</div>
					<div>
						<p class="text-2xl font-bold tabular-nums">{activeDownloads.length}</p>
						<p class="text-xs text-muted-foreground">Active Downloads</p>
					</div>
				</Card.Content>
			</Card.Root>
			<Card.Root>
				<Card.Content class="flex items-center gap-3 p-4">
					<div class="flex items-center justify-center size-10 rounded-lg bg-green-500/10 shrink-0">
						<Plus class="size-5 text-green-500" />
					</div>
					<div>
						<p class="text-2xl font-bold tabular-nums">{totalNewAcrossPlaylists}</p>
						<p class="text-xs text-muted-foreground">New Tracks</p>
					</div>
				</Card.Content>
			</Card.Root>
		</div>

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
					<span class="text-xs text-muted-foreground font-mono">yt-dlp {depsStatus.ytdlp_version}</span>
				{/if}
			</div>

			<!-- ==================== PLAYLISTS TAB ==================== -->
			<Tabs.Content value="playlists" class="space-y-4 mt-4">
				{#if selectedPlaylist}
					<!-- Playlist detail view -->
					<div class="space-y-4">
						<div class="flex items-center gap-3">
							<Button variant="ghost" size="sm" onclick={closePlaylistDetail}>
								<ChevronLeft class="size-4" />
								Back
							</Button>
							<div class="flex-1 min-w-0">
								<div class="flex items-center gap-2">
									<h2 class="text-lg font-semibold truncate">{selectedPlaylist.name}</h2>
									<Badge variant={platformColor(selectedPlaylist.source_platform ?? 'other')} class="text-xs shrink-0">
										{platformLabel(selectedPlaylist.source_platform ?? 'other')}
									</Badge>
								</div>
								<p class="text-xs text-muted-foreground truncate">
									{selectedEntries.length} tracks
									{#if downloadingEntries.length > 0}
										&middot; <span class="text-blue-400">{downloadingEntries.length} downloading</span>
									{/if}
									{#if queuedEntries.length > 0}
										&middot; {queuedEntries.length} queued
									{/if}
									{#if newEntries.length > 0}
										&middot; {newEntries.length} new
									{/if}
									{#if failedEntries.length > 0}
										&middot; <span class="text-destructive">{failedEntries.length} failed</span>
									{/if}
									&middot; {downloadedEntries.length} downloaded
									&middot; Synced {timeAgo(selectedPlaylist.last_synced_at)}
								</p>
							</div>
							<div class="flex items-center gap-2 shrink-0">
								{#if queuedEntries.length > 0 || downloadingEntries.length > 0}
									<Button
										variant="destructive"
										size="sm"
										onclick={() => handleCancelAll(selectedPlaylist!.id)}
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
									>
										{#if retryingAll}
											<Loader2 class="size-3.5 animate-spin" />
										{:else}
											<RotateCcw class="size-3.5" />
										{/if}
										Retry {failedEntries.length} failed
									</Button>
								{/if}
								{#if newEntries.length > 0}
									<Button size="sm" onclick={() => handleDownloadAllNew(selectedPlaylist!.id)}>
										<ArrowDownToLine class="size-4" />
										Download {newEntries.length} new
									</Button>
								{/if}
								<Button
									variant="outline"
									size="sm"
									onclick={() => handleSync(selectedPlaylist!.id)}
									disabled={syncingIds.has(selectedPlaylist.id)}
								>
									<RefreshCw class="size-3.5 {syncingIds.has(selectedPlaylist.id) ? 'animate-spin' : ''}" />
									Sync
								</Button>
							</div>
						</div>

						{#if loadingEntries}
							<div class="flex justify-center p-8">
								<Loader2 class="size-6 animate-spin text-muted-foreground" />
							</div>
						{:else if selectedEntries.length === 0}
							<div class="flex flex-col items-center justify-center h-32 rounded-lg border border-dashed border-border gap-2">
								<ListMusic class="size-6 text-muted-foreground" />
								<p class="text-muted-foreground text-sm">No tracks found</p>
							</div>
						{:else}
							<div class="space-y-3">
								<div class="space-y-1">
									{#each paginatedEntries as entry, i (entry.id)}
										<div class="flex items-center gap-3 rounded-lg border border-border p-3 hover:bg-accent/50 transition-colors">
											<span class="text-xs text-muted-foreground w-6 text-right shrink-0">{entriesPage * entriesPageSize + i + 1}</span>
										<div class="shrink-0">
											{#if entry.status === 'downloaded'}
												<CheckCircle2 class="size-4 text-green-500" />
											{:else if entry.status === 'new'}
												<div class="size-4 rounded-full border-2 border-blue-500 bg-blue-500/20"></div>
											{:else if entry.status === 'queued' || entry.status === 'downloading'}
												<Loader2 class="size-4 animate-spin text-muted-foreground" />
											{:else if entry.status === 'failed'}
												<XCircle class="size-4 text-destructive" />
											{:else if entry.status === 'skipped'}
												<SkipForward class="size-4 text-muted-foreground" />
											{:else}
												<div class="size-4"></div>
											{/if}
										</div>
										<div class="flex-1 min-w-0">
											<p class="text-sm truncate">{entry.title || entry.source_url}</p>
											<div class="flex items-center gap-2 mt-0.5">
												{#if entry.artist}
													<span class="text-xs text-muted-foreground truncate">{entry.artist}</span>
												{/if}
												{#if entry.duration_seconds}
													<span class="text-xs text-muted-foreground">{formatSeconds(entry.duration_seconds)}</span>
												{/if}
											</div>
										</div>
										<div class="flex items-center gap-1 shrink-0">
											{#if entry.status === 'new'}
												<Button
													variant="ghost"
													size="sm"
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
													size="sm"
													onclick={() => handleSkipEntry(entry.id)}
												>
													<SkipForward class="size-3.5" />
												</Button>
											{:else if entry.status === 'queued' || entry.status === 'downloading'}
												<Button
													variant="ghost"
													size="sm"
													onclick={() => handleCancelEntry(entry.id)}
												>
													<X class="size-3.5" />
												</Button>
											{:else if entry.status === 'failed'}
												<Button
													variant="ghost"
													size="sm"
													onclick={() => handleDownloadEntry(entry.id)}
												>
													<RotateCcw class="size-3.5" />
												</Button>
											{/if}
											<Badge variant="outline" class="text-xs">
												{entry.status}
											</Badge>
										</div>
										</div>
									{/each}
								</div>
								
								{#if sortedEntries.length > entriesPageSize}
									{@const totalPages = Math.ceil(sortedEntries.length / entriesPageSize)}
									<div class="flex items-center justify-center gap-2 pt-2 border-t">
										<Button
											variant="outline"
											size="sm"
											onclick={prevEntriesPage}
											disabled={entriesPage === 0}
										>
											Previous
										</Button>
										<span class="text-sm text-muted-foreground">
											Page {entriesPage + 1} of {totalPages}
										</span>
										<Button
											variant="outline"
											size="sm"
											onclick={nextEntriesPage}
											disabled={entriesPage >= totalPages - 1}
										>
											Next
										</Button>
									</div>
								{/if}
							</div>
						{/if}
					</div>
				{:else}
					<!-- Playlist list view -->
					<div class="flex gap-2 max-w-2xl">
						<div class="relative flex-1">
							<ListMusic class="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
							<Input
								placeholder="Paste a playlist URL to monitor..."
								class="pl-10"
								bind:value={playlistUrlInput}
								onkeydown={handlePlaylistKeydown}
							/>
						</div>
						<Button
							onclick={handleAddPlaylist}
							disabled={!playlistUrlInput.trim() || addingPlaylist}
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
							>
								{#if syncingAll}
									<Loader2 class="size-4 animate-spin" />
								{:else}
									<RefreshCw class="size-4" />
								{/if}
								Sync all
							</Button>
						{/if}
					</div>

					{#if playlists.length === 0}
						<div class="flex flex-col items-center justify-center h-48 rounded-lg border border-dashed border-border gap-3">
							<ListMusic class="size-8 text-muted-foreground" />
							<div class="text-center">
								<p class="text-muted-foreground text-sm">No playlists tracked yet</p>
								<p class="text-muted-foreground text-xs mt-1">
									Paste a playlist URL from YouTube, SoundCloud, Bandcamp, or any yt-dlp supported site
								</p>
							</div>
						</div>
					{:else}
						<div class="grid gap-3">
							{#each playlists as pl (pl.id)}
								<div
									class="flex items-center gap-4 rounded-lg border border-border p-4 hover:bg-accent/50 transition-colors cursor-pointer group"
									role="button"
									tabindex="0"
									onclick={() => openPlaylist(pl)}
									onkeydown={(e) => { if (e.key === 'Enter') openPlaylist(pl); }}
								>
									<div class="flex items-center justify-center size-12 rounded-lg bg-muted shrink-0">
										<ListMusic class="size-6 text-muted-foreground" />
									</div>
									<div class="flex-1 min-w-0">
										<div class="flex items-center gap-2">
											<p class="font-medium truncate">{pl.name}</p>
											<Badge variant={platformColor(pl.source_platform ?? 'other')} class="text-xs shrink-0">
												{platformLabel(pl.source_platform ?? 'other')}
											</Badge>
											{#if pl.active_count > 0}
												<Badge variant="secondary" class="text-xs shrink-0">
													{pl.active_count} active
												</Badge>
											{/if}
											{#if pl.new_count > 0}
												<Badge variant="default" class="text-xs shrink-0">
													{pl.new_count} new
												</Badge>
											{/if}
										</div>
										<div class="flex items-center gap-3 mt-1">
											<span class="text-xs text-muted-foreground">
												{pl.total_entries} tracks
											</span>
											<span class="text-xs text-muted-foreground">
												{pl.downloaded_count} downloaded
											</span>
											<span class="text-xs text-muted-foreground flex items-center gap-1">
												<Clock class="size-3" />
												{timeAgo(pl.last_synced_at)}
											</span>
										</div>
									</div>
									<div class="flex items-center gap-2 shrink-0">
										{#if pl.active_count > 0}
											<Button
												variant="destructive"
												size="sm"
												onclick={(e) => { e.stopPropagation(); handleCancelAll(pl.id); }}
											>
												<Square class="size-3.5" />
												Stop {pl.active_count}
											</Button>
										{/if}
										{#if pl.new_count > 0}
											<Button
												variant="default"
												size="sm"
												onclick={(e) => { e.stopPropagation(); handleDownloadAllNew(pl.id); }}
											>
												<ArrowDownToLine class="size-3.5" />
												{pl.new_count}
											</Button>
										{/if}
										<Button
											variant="ghost"
											size="sm"
											onclick={(e) => { e.stopPropagation(); handleSync(pl.id); }}
											disabled={syncingIds.has(pl.id)}
										>
											<RefreshCw class="size-3.5 {syncingIds.has(pl.id) ? 'animate-spin' : ''}" />
										</Button>
										<Button
											variant="ghost"
											size="sm"
											class="text-destructive hover:text-destructive"
											onclick={(e) => { e.stopPropagation(); handleRemovePlaylist(pl.id); }}
										>
											<Trash2 class="size-3.5" />
										</Button>
										<ChevronRight class="size-4 text-muted-foreground group-hover:text-foreground transition-colors" />
									</div>
								</div>
							{/each}
						</div>
					{/if}
				{/if}
			</Tabs.Content>

			<!-- ==================== DOWNLOADS TAB ==================== -->
			<Tabs.Content value="downloads" class="space-y-4 mt-4">
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
					<Button onclick={handleSubmit} disabled={!urlInput.trim() || submitting}>
						{#if submitting}
							<Loader2 class="size-4 animate-spin" />
						{:else}
							<Download class="size-4" />
						{/if}
						Download
					</Button>
				</div>

				<!-- Active Downloads (grouped by album when applicable) -->
				{#if activeDownloads.length > 0}
					<div class="space-y-3">
						<h2 class="text-lg font-semibold">Active ({activeDownloads.length})</h2>
						{#each groupedActiveDownloads as group}
							{#if group.albumId}
								<!-- Album group -->
								<div class="rounded-lg border border-border overflow-hidden">
									<div class="flex items-center gap-3 px-4 py-2.5 bg-muted/40 border-b border-border">
										<Disc class="size-4 text-muted-foreground shrink-0" />
										<span class="text-sm font-medium truncate">{group.albumTitle}</span>
										<Badge variant="secondary" class="text-xs shrink-0 ml-auto">
											{group.downloads.length} track{group.downloads.length !== 1 ? 's' : ''}
										</Badge>
									</div>
									<div class="divide-y divide-border">
										{#each group.downloads.slice(0, 20) as dl (dl.id)}
											<div class="flex items-center gap-3 px-4 py-3">
												<div class="shrink-0">
													{#if dl.status === 'downloading'}
														<Loader2 class="size-4 animate-spin text-muted-foreground" />
													{:else if dl.status === 'processing'}
														<Loader2 class="size-4 animate-spin text-green-500" />
													{:else}
														<Clock class="size-4 text-muted-foreground" />
													{/if}
												</div>
												<div class="flex-1 min-w-0">
													<p class="text-sm truncate">{dl.title || dl.url}</p>
													<div class="flex items-center gap-2 mt-0.5">
														{#if dl.artist}
															<span class="text-xs text-muted-foreground truncate">{dl.artist}</span>
														{/if}
														{#if dl.status === 'downloading'}
															<span class="text-xs text-muted-foreground">{Math.round(dl.progress)}%</span>
														{:else if dl.status === 'processing'}
															<span class="text-xs text-muted-foreground">Importing...</span>
														{:else}
															<span class="text-xs text-muted-foreground">Queued</span>
														{/if}
													</div>
													{#if dl.status === 'downloading'}
														<Progress value={dl.progress} class="mt-1.5 h-1" />
													{/if}
												</div>
												<Badge variant={platformColor(dl.platform)} class="text-xs shrink-0">
													{platformLabel(dl.platform)}
												</Badge>
												<Button variant="ghost" size="sm" onclick={() => handleCancel(dl.id)}>
													<X class="size-4" />
												</Button>
											</div>
										{/each}
										{#if group.downloads.length > 20}
											<p class="text-xs text-muted-foreground text-center py-2">
												and {group.downloads.length - 20} more queued...
											</p>
										{/if}
									</div>
								</div>
							{:else}
								<!-- Ungrouped downloads -->
								<div class="space-y-1">
									{#each group.downloads.slice(0, 20) as dl (dl.id)}
										<div class="flex items-center gap-3 rounded-lg border border-border p-3 hover:bg-accent/50 transition-colors">
											<div class="shrink-0">
												{#if dl.status === 'downloading'}
													<Loader2 class="size-4 animate-spin text-muted-foreground" />
												{:else if dl.status === 'processing'}
													<Loader2 class="size-4 animate-spin text-green-500" />
												{:else}
													<Clock class="size-4 text-muted-foreground" />
												{/if}
											</div>
											<div class="flex-1 min-w-0">
												<p class="text-sm truncate">{dl.title || dl.url}</p>
												<div class="flex items-center gap-2 mt-0.5">
													{#if dl.artist}
														<span class="text-xs text-muted-foreground truncate">{dl.artist}</span>
													{/if}
													{#if dl.status === 'downloading'}
														<span class="text-xs text-muted-foreground">{Math.round(dl.progress)}%</span>
													{:else if dl.status === 'processing'}
														<span class="text-xs text-muted-foreground">Importing...</span>
													{:else}
														<span class="text-xs text-muted-foreground">Queued</span>
													{/if}
												</div>
												{#if dl.status === 'downloading'}
													<Progress value={dl.progress} class="mt-1.5 h-1" />
												{/if}
											</div>
											<Badge variant={platformColor(dl.platform)} class="text-xs shrink-0">
												{platformLabel(dl.platform)}
											</Badge>
											<Button variant="ghost" size="sm" onclick={() => handleCancel(dl.id)}>
												<X class="size-4" />
											</Button>
										</div>
									{/each}
								</div>
								{#if group.downloads.length > 20}
									<p class="text-xs text-muted-foreground text-center py-2">
										and {group.downloads.length - 20} more queued...
									</p>
								{/if}
							{/if}
						{/each}
					</div>
				{/if}

				<!-- Completed -->
				{#if allCompletedDownloads.length > 0}
					<div class="space-y-3">
						<h2 class="text-lg font-semibold">Recent ({allCompletedDownloads.length})</h2>
						<div class="space-y-1">
							{#each completedDownloads as dl (dl.id)}
								<div class="flex items-center gap-3 rounded-lg border border-border p-3 hover:bg-accent/50 transition-colors">
									<div class="shrink-0">
										{#if dl.status === 'completed'}
											<CheckCircle2 class="size-4 text-green-500" />
										{:else if dl.status === 'failed'}
											<XCircle class="size-4 text-destructive" />
										{:else}
											<XCircle class="size-4 text-muted-foreground" />
										{/if}
									</div>
									<div class="flex-1 min-w-0">
										<p class="text-sm truncate">{dl.title || dl.url}</p>
										<div class="flex items-center gap-2 mt-0.5">
											{#if dl.artist}
												<span class="text-xs text-muted-foreground truncate">{dl.artist}</span>
											{/if}
											{#if dl.status === 'failed' && dl.error_message}
												<span class="text-xs text-destructive truncate">{dl.error_message}</span>
											{:else}
												<span class="text-xs text-muted-foreground">{dl.format.toUpperCase()}</span>
											{/if}
										</div>
									</div>
									<Badge variant={platformColor(dl.platform)} class="text-xs shrink-0">
										{platformLabel(dl.platform)}
									</Badge>
									{#if dl.status === 'failed'}
										<Button variant="ghost" size="sm" onclick={() => handleRetry(dl.id)}>
											<RotateCcw class="size-4" />
										</Button>
									{/if}
								</div>
							{/each}
						</div>

						{#if completedTotalPages > 1}
							<div class="flex items-center justify-center gap-2 pt-2">
								<Button
									variant="outline"
									size="sm"
									disabled={completedPage === 0}
									onclick={() => completedPage--}
								>
									Previous
								</Button>
								<span class="text-sm text-muted-foreground">
									Page {completedPage + 1} of {completedTotalPages}
								</span>
								<Button
									variant="outline"
									size="sm"
									disabled={completedPage >= completedTotalPages - 1}
									onclick={() => completedPage++}
								>
									Next
								</Button>
							</div>
						{/if}
					</div>
				{/if}

				<!-- Empty state -->
				{#if activeDownloads.length === 0 && allCompletedDownloads.length === 0}
					<div class="flex flex-col items-center justify-center h-32 rounded-lg border border-dashed border-border gap-2">
						<Download class="size-6 text-muted-foreground" />
						<p class="text-muted-foreground text-sm">Paste a URL above to start downloading</p>
					</div>
				{/if}
			</Tabs.Content>

			<!-- ==================== HISTORY TAB ==================== -->
			<Tabs.Content value="history" class="space-y-4 mt-4">
				{#if !historyLoaded}
					<div class="flex justify-center p-8">
						<Loader2 class="size-6 animate-spin text-muted-foreground" />
					</div>
				{:else}
					<div class="flex items-center justify-between">
						<p class="text-sm text-muted-foreground">{historyTotal} total downloads</p>
						{#if history.length > 0}
							<Button variant="ghost" size="sm" onclick={handleClearHistory}>
								<Trash2 class="size-4" />
								Clear history
							</Button>
						{/if}
					</div>

					{#if history.length === 0}
						<div class="flex flex-col items-center justify-center h-32 rounded-lg border border-dashed border-border gap-2">
							<Clock class="size-6 text-muted-foreground" />
							<p class="text-muted-foreground text-sm">No download history</p>
						</div>
					{:else}
						<div class="space-y-3">
							<div class="space-y-1">
								{#each history as dl (dl.id)}
									<div class="flex items-center gap-3 rounded-lg border border-border p-3 hover:bg-accent/50 transition-colors">
										<div class="shrink-0">
											{#if dl.status === 'completed'}
												<CheckCircle2 class="size-4 text-green-500" />
											{:else if dl.status === 'failed'}
												<XCircle class="size-4 text-destructive" />
											{:else if dl.status === 'cancelled'}
												<XCircle class="size-4 text-muted-foreground" />
											{:else}
												<Loader2 class="size-4 text-muted-foreground" />
											{/if}
										</div>
										<div class="flex-1 min-w-0">
											<p class="text-sm truncate">{dl.title || dl.url}</p>
											<div class="flex items-center gap-2 mt-0.5">
												{#if dl.artist}
													<span class="text-xs text-muted-foreground truncate">{dl.artist}</span>
													<span class="text-xs text-muted-foreground">&middot;</span>
												{/if}
												<span class="text-xs text-muted-foreground">{formatDate(dl.created_at)}</span>
											</div>
										</div>
										<Badge variant={platformColor(dl.platform)} class="text-xs shrink-0">
											{platformLabel(dl.platform)}
										</Badge>
										<Badge variant="outline" class="text-xs shrink-0">
											{dl.status}
										</Badge>
									</div>
								{/each}
							</div>

							{#if historyTotal > historyPageSize}
								{@const totalPages = Math.ceil(historyTotal / historyPageSize)}
								<div class="flex items-center justify-center gap-2 pt-2 border-t">
									<Button
										variant="outline"
										size="sm"
										onclick={prevHistoryPage}
										disabled={historyPage === 0}
									>
										Previous
									</Button>
									<span class="text-sm text-muted-foreground">
										Page {historyPage + 1} of {totalPages}
									</span>
									<Button
										variant="outline"
										size="sm"
										onclick={nextHistoryPage}
										disabled={historyPage >= totalPages - 1}
									>
										Next
									</Button>
								</div>
							{/if}
						</div>
					{/if}
				{/if}
			</Tabs.Content>
		</Tabs.Root>
	{/if}
</div>
