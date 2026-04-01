<script lang="ts">
	import { listen } from '@tauri-apps/api/event';
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Progress } from '$lib/components/ui/progress';
	import * as Tabs from '$lib/components/ui/tabs';
	import * as Card from '$lib/components/ui/card';
	import {
		Download,
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
	import { formatDate } from '$lib/utils/format';

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

	// --- History tab state ---
	let history: DownloadType[] = $state([]);
	let historyTotal = $state(0);
	let historyLoaded = $state(false);

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

	// --- Active tab ---
	let activeTab = $state('playlists');

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
			const [data, total] = await getDownloadHistory(0, 100);
			history = data;
			historyTotal = total;
			historyLoaded = true;
		} catch {
			toast.error('Failed to load history');
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
		selectedPlaylist = pl;
		await loadEntries(pl.id);
	}

	async function loadEntries(playlistId: number) {
		loadingEntries = true;
		try {
			selectedEntries = await getEntries(playlistId);
		} catch {
			toast.error('Failed to load entries');
		} finally {
			loadingEntries = false;
		}
	}

	function closePlaylistDetail() {
		selectedPlaylist = null;
		selectedEntries = [];
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

	// --- Helpers ---
	function platformLabel(platform: string): string {
		const labels: Record<string, string> = {
			youtube: 'YouTube',
			spotify: 'Spotify',
			soundcloud: 'SoundCloud',
			bandcamp: 'Bandcamp',
			direct: 'Direct',
			other: 'Other',
		};
		return labels[platform] ?? platform;
	}

	function platformColor(platform: string): 'default' | 'secondary' | 'outline' | 'destructive' {
		switch (platform) {
			case 'youtube': return 'destructive';
			case 'spotify': return 'default';
			case 'soundcloud': return 'secondary';
			default: return 'outline';
		}
	}

	function formatSeconds(seconds: number | null): string {
		if (!seconds) return '--:--';
		const m = Math.floor(seconds / 60);
		const s = Math.round(seconds % 60);
		return `${m}:${s.toString().padStart(2, '0')}`;
	}

	function timeAgo(dateStr: string | null): string {
		if (!dateStr) return 'Never';
		const date = new Date(dateStr + 'Z');
		const now = new Date();
		const diff = now.getTime() - date.getTime();
		const minutes = Math.floor(diff / 60000);
		if (minutes < 1) return 'Just now';
		if (minutes < 60) return `${minutes}m ago`;
		const hours = Math.floor(minutes / 60);
		if (hours < 24) return `${hours}h ago`;
		const days = Math.floor(hours / 24);
		if (days < 7) return `${days}d ago`;
		return formatDate(dateStr);
	}

	const activeDownloads = $derived(
		downloadStore.downloads.filter(
			(d) => d.status === 'queued' || d.status === 'downloading' || d.status === 'processing'
		)
	);

	const completedDownloads = $derived(
		downloadStore.downloads.filter(
			(d) => d.status === 'completed' || d.status === 'failed' || d.status === 'cancelled'
		)
	);

	const newEntries = $derived(selectedEntries.filter((e) => e.status === 'new'));
	const downloadedEntries = $derived(selectedEntries.filter((e) => e.status === 'downloaded'));
	const queuedEntries = $derived(selectedEntries.filter((e) => e.status === 'queued'));
	const downloadingEntries = $derived(selectedEntries.filter((e) => e.status === 'downloading'));
	const totalNewAcrossPlaylists = $derived(playlists.reduce((sum, p) => sum + p.new_count, 0));
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
		<!-- Main content - tabbed interface -->
		<Tabs.Root bind:value={activeTab}>
			<div class="flex items-center justify-between">
				<Tabs.List>
					<Tabs.Trigger value="playlists">
						Playlists
						{#if totalNewAcrossPlaylists > 0}
							<Badge variant="default" class="ml-1.5 h-5 min-w-5 px-1 text-xs">{totalNewAcrossPlaylists}</Badge>
						{/if}
					</Tabs.Trigger>
					<Tabs.Trigger value="download">
						Download
						{#if activeDownloads.length > 0}
							<Badge variant="secondary" class="ml-1.5 h-5 min-w-5 px-1 text-xs">{activeDownloads.length}</Badge>
						{/if}
					</Tabs.Trigger>
					<Tabs.Trigger value="history">History</Tabs.Trigger>
				</Tabs.List>

				{#if depsStatus?.ytdlp_version}
					<span class="text-xs text-muted-foreground">yt-dlp {depsStatus.ytdlp_version}</span>
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
							<div class="space-y-1">
								{#each selectedEntries as entry, i (entry.id)}
									<div class="flex items-center gap-3 rounded-lg border border-border p-3 hover:bg-accent/50 transition-colors">
										<span class="text-xs text-muted-foreground w-6 text-right shrink-0">{i + 1}</span>
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

			<!-- ==================== DOWNLOAD TAB ==================== -->
			<Tabs.Content value="download" class="space-y-4 mt-4">
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

				<!-- Active Downloads -->
				{#if activeDownloads.length > 0}
					<div class="space-y-3">
						<h2 class="text-lg font-semibold">Active ({activeDownloads.length})</h2>
						{#each activeDownloads.slice(0, 20) as dl (dl.id)}
							<div class="flex items-center gap-4 rounded-lg border border-border p-4">
								<div class="flex-1 min-w-0 space-y-2">
									<div class="flex items-center gap-2">
										<p class="text-sm font-medium truncate">{dl.title || dl.url}</p>
										<Badge variant={platformColor(dl.platform)} class="text-xs shrink-0">
											{platformLabel(dl.platform)}
										</Badge>
									</div>
									{#if dl.status === 'downloading'}
										<div class="flex items-center gap-3">
											<Progress value={dl.progress} class="flex-1" />
											<span class="text-xs text-muted-foreground w-12 text-right">
												{Math.round(dl.progress)}%
											</span>
										</div>
									{:else if dl.status === 'processing'}
										<div class="flex items-center gap-2">
											<Loader2 class="size-3 animate-spin text-muted-foreground" />
											<span class="text-xs text-muted-foreground">Importing to library...</span>
										</div>
									{:else}
										<span class="text-xs text-muted-foreground">Queued</span>
									{/if}
								</div>
								<Button variant="ghost" size="sm" onclick={() => handleCancel(dl.id)}>
									<X class="size-4" />
								</Button>
							</div>
						{/each}
						{#if activeDownloads.length > 20}
							<p class="text-xs text-muted-foreground text-center py-2">
								and {activeDownloads.length - 20} more queued...
							</p>
						{/if}
					</div>
				{/if}

				<!-- Completed -->
				{#if completedDownloads.length > 0}
					<div class="space-y-3">
						<h2 class="text-lg font-semibold">Recent</h2>
						{#each completedDownloads as dl (dl.id)}
							<div class="flex items-center gap-4 rounded-lg border border-border p-3">
								<div class="shrink-0">
									{#if dl.status === 'completed'}
										<CheckCircle2 class="size-5 text-green-500" />
									{:else if dl.status === 'failed'}
										<XCircle class="size-5 text-destructive" />
									{:else}
										<XCircle class="size-5 text-muted-foreground" />
									{/if}
								</div>
								<div class="flex-1 min-w-0">
									<p class="text-sm font-medium truncate">{dl.title || dl.url}</p>
									<div class="flex items-center gap-2 mt-0.5">
										<Badge variant={platformColor(dl.platform)} class="text-xs">
											{platformLabel(dl.platform)}
										</Badge>
										{#if dl.status === 'failed' && dl.error_message}
											<span class="text-xs text-destructive truncate">{dl.error_message}</span>
										{:else}
											<span class="text-xs text-muted-foreground">{dl.format.toUpperCase()}</span>
										{/if}
									</div>
								</div>
								{#if dl.status === 'failed'}
									<Button variant="ghost" size="sm" onclick={() => handleRetry(dl.id)}>
										<RotateCcw class="size-4" />
									</Button>
								{/if}
							</div>
						{/each}
					</div>
				{/if}

				<!-- Empty state -->
				{#if activeDownloads.length === 0 && completedDownloads.length === 0}
					<div class="flex flex-col items-center justify-center h-32 rounded-lg border border-dashed border-border gap-2">
						<Download class="size-6 text-muted-foreground" />
						<p class="text-muted-foreground text-sm">Paste a URL above to start downloading</p>
					</div>
				{/if}
			</Tabs.Content>

			<!-- ==================== HISTORY TAB ==================== -->
			<Tabs.Content value="history" class="space-y-4 mt-4">
				{#if !historyLoaded}
					<div class="flex flex-col items-center justify-center h-32 gap-3">
						<Button variant="outline" onclick={loadHistory}>
							<Clock class="size-4" />
							Load download history
						</Button>
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
						<div class="space-y-1">
							{#each history as dl (dl.id)}
								<div class="flex items-center gap-3 rounded-lg border border-border p-3">
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
										<span class="text-xs text-muted-foreground">
											{platformLabel(dl.platform)} &middot; {formatDate(dl.created_at)}
										</span>
									</div>
									<Badge variant="outline" class="text-xs shrink-0">
										{dl.status}
									</Badge>
								</div>
							{/each}
						</div>
					{/if}
				{/if}
			</Tabs.Content>
		</Tabs.Root>
	{/if}
</div>
