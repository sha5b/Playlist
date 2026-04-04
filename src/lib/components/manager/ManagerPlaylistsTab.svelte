<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Progress } from '$lib/components/ui/progress';
	import {
		Download,
		Loader2,
		CheckCircle2,
		XCircle,
		RotateCcw,
		Trash2,
		RefreshCw,
		ListMusic,
		Plus,
		SkipForward,
		Clock,
		ChevronRight,
		ChevronLeft,
		ArrowDownToLine,
		Square,
	} from 'lucide-svelte';
	import type { MonitoredPlaylist, MonitoredEntry } from '$lib/types';
	import { assetUrl, formatSeconds, timeAgo, platformLabel, platformColor } from '$lib/utils/format';

	let {
		playlists,
		playlistUrlInput = $bindable(),
		addingPlaylist,
		syncingAll,
		syncAllProgress,
		syncingIds,
		selectedPlaylist = $bindable(),
		selectedEntries,
		loadingEntries,
		downloadingEntryIds,
		entriesPage = $bindable(),
		entriesPageSize,
		retryingAll,
		activeDownloads,
		totalNewAcrossPlaylists,
		onaddPlaylist,
		onsyncPlaylist,
		onsyncAll,
		onremovePlaylist,
		onopenPlaylist,
		oncloseDetail,
		ondownloadEntry,
		ondownloadAllNew,
		oncancelEntry,
		oncancelAll,
		onskipEntry,
		onretryAllFailed,
	}: {
		playlists: MonitoredPlaylist[];
		playlistUrlInput: string;
		addingPlaylist: boolean;
		syncingAll: boolean;
		syncAllProgress: { current: number; total: number; name: string };
		syncingIds: Set<number>;
		selectedPlaylist: MonitoredPlaylist | null;
		selectedEntries: MonitoredEntry[];
		loadingEntries: boolean;
		downloadingEntryIds: Set<number>;
		entriesPage: number;
		entriesPageSize: number;
		retryingAll: boolean;
		activeDownloads: { length: number };
		totalNewAcrossPlaylists: number;
		onaddPlaylist: () => void;
		onsyncPlaylist: (id: number) => void;
		onsyncAll: () => void;
		onremovePlaylist: (id: number) => void;
		onopenPlaylist: (pl: MonitoredPlaylist) => void;
		oncloseDetail: () => void;
		ondownloadEntry: (id: number) => void;
		ondownloadAllNew: (id: number) => void;
		oncancelEntry: (id: number) => void;
		oncancelAll: (id: number) => void;
		onskipEntry: (id: number) => void;
		onretryAllFailed: () => void;
	} = $props();

	function handlePlaylistKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') onaddPlaylist();
	}

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

	const statusOrder: Record<string, number> = {
		new: 0, queued: 1, downloading: 2, failed: 3, downloaded: 4, skipped: 5,
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

	function nextEntriesPage() {
		const maxPage = Math.ceil(sortedEntries.length / entriesPageSize) - 1;
		if (entriesPage < maxPage) entriesPage++;
	}

	function prevEntriesPage() {
		if (entriesPage > 0) entriesPage--;
	}
</script>

{#if selectedPlaylist}
	<!-- Playlist detail view -->
	<div class="space-y-4">
		<div class="flex items-start gap-4">
			<Button variant="ghost" size="icon" onclick={oncloseDetail} class="shrink-0 mt-0.5 rounded-full">
				<ChevronLeft class="size-5" />
			</Button>
			<div class="flex-1 min-w-0">
				<div class="flex items-center gap-2 mb-1">
					<h2 class="text-xl font-semibold truncate">{selectedPlaylist.name}</h2>
					<Badge variant={platformColor(selectedPlaylist.source_platform ?? 'other')} class="text-xs shrink-0">
						{platformLabel(selectedPlaylist.source_platform ?? 'other')}
					</Badge>
				</div>
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
					<Button variant="destructive" size="sm" onclick={() => oncancelAll(selectedPlaylist!.id)} class="gap-1.5">
						<Square class="size-3.5" />
						Stop all
					</Button>
				{/if}
				{#if failedEntries.length > 0}
					<Button variant="outline" size="sm" onclick={onretryAllFailed} disabled={retryingAll} class="gap-1.5">
						{#if retryingAll}
							<Loader2 class="size-3.5 animate-spin" />
						{:else}
							<RotateCcw class="size-3.5" />
						{/if}
						Retry {failedEntries.length}
					</Button>
				{/if}
				{#if newEntries.length > 0}
					<Button size="sm" onclick={() => ondownloadAllNew(selectedPlaylist!.id)} class="gap-1.5">
						<ArrowDownToLine class="size-4" />
						Download {newEntries.length} new
					</Button>
				{/if}
				<Button variant="outline" size="sm" onclick={() => onsyncPlaylist(selectedPlaylist!.id)} disabled={syncingIds.has(selectedPlaylist.id)} class="gap-1.5">
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
			<div class="rounded-xl border border-border/60 overflow-hidden">
				<div class="border-b border-border bg-muted/30 text-xs text-muted-foreground uppercase tracking-wider flex items-center px-4 py-2.5">
					<div class="w-8 text-center">#</div>
					<div class="w-7"></div>
					<div class="flex-1 pl-3">Title</div>
					<div class="w-24 text-right">Duration</div>
					<div class="w-24 text-center">Status</div>
					<div class="w-20"></div>
				</div>
				<div class="divide-y divide-border/40">
					{#each paginatedEntries as entry, i (entry.id)}
						<div class="flex items-center px-4 py-2.5 hover:bg-muted/30 transition-colors group">
							<div class="w-8 text-center text-xs text-muted-foreground tabular-nums">
								{entriesPage * entriesPageSize + i + 1}
							</div>
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
							<div class="w-9 h-9 rounded overflow-hidden flex-shrink-0 ml-2 bg-muted/30">
								{#if entry.thumbnail}
									<img
										src={entry.thumbnail.startsWith('/') ? assetUrl(entry.thumbnail) : entry.thumbnail}
										alt=""
										class="w-full h-full object-cover"
									/>
								{/if}
							</div>
							<div class="flex-1 min-w-0 pl-3">
								<p class="text-sm truncate {entry.status === 'skipped' ? 'text-muted-foreground/50 line-through' : ''}">{entry.title || entry.source_url}</p>
								{#if entry.artist}
									<p class="text-xs text-muted-foreground truncate mt-0.5">{entry.artist}</p>
								{/if}
							</div>
							<div class="w-24 text-right text-xs text-muted-foreground tabular-nums">
								{entry.duration_seconds ? formatSeconds(entry.duration_seconds) : '--:--'}
							</div>
							<div class="w-24 flex justify-center">
								<Badge variant="outline" class="text-xs capitalize {entryStatusColor(entry.status)}">
									{entry.status}
								</Badge>
							</div>
							<div class="w-20 flex items-center justify-end gap-1">
								{#if entry.status === 'new'}
									<Button variant="ghost" size="icon-sm" class="size-7 opacity-0 group-hover:opacity-100 transition-opacity" onclick={() => ondownloadEntry(entry.id)} disabled={downloadingEntryIds.has(entry.id)}>
										{#if downloadingEntryIds.has(entry.id)}
											<Loader2 class="size-3.5 animate-spin" />
										{:else}
											<Download class="size-3.5" />
										{/if}
									</Button>
									<Button variant="ghost" size="icon-sm" class="size-7 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground" onclick={() => onskipEntry(entry.id)}>
										<SkipForward class="size-3.5" />
									</Button>
								{:else if entry.status === 'queued' || entry.status === 'downloading'}
									<Button variant="ghost" size="icon-sm" class="size-7 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground" onclick={() => oncancelEntry(entry.id)}>
										<XCircle class="size-3.5" />
									</Button>
								{:else if entry.status === 'failed'}
									<Button variant="ghost" size="icon-sm" class="size-7 text-destructive" onclick={() => ondownloadEntry(entry.id)}>
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
					<span class="text-sm text-muted-foreground tabular-nums">{entriesPage + 1} / {totalPages}</span>
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
					<p class="text-sm font-medium">{totalNewAcrossPlaylists} new track{totalNewAcrossPlaylists !== 1 ? 's' : ''} found</p>
					<p class="text-xs text-muted-foreground mt-0.5">Across {playlistsWithNew.length} playlist{playlistsWithNew.length !== 1 ? 's' : ''}</p>
				</div>
			</div>
			<Button size="sm" class="gap-1.5 shrink-0" onclick={() => { for (const pl of playlistsWithNew) ondownloadAllNew(pl.id); }}>
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
			<Input placeholder="Paste a playlist URL..." class="pl-10" bind:value={playlistUrlInput} onkeydown={handlePlaylistKeydown} />
		</div>
		<Button onclick={onaddPlaylist} disabled={!playlistUrlInput.trim() || addingPlaylist} class="gap-1.5">
			{#if addingPlaylist}
				<Loader2 class="size-4 animate-spin" />
			{:else}
				<Plus class="size-4" />
			{/if}
			Add
		</Button>
		{#if playlists.length > 0}
			<Button variant="outline" onclick={onsyncAll} disabled={syncingAll} class="gap-1.5">
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
					onclick={() => onopenPlaylist(pl)}
					onkeydown={(e) => { if (e.key === 'Enter') onopenPlaylist(pl); }}
				>
					<div class="aspect-[16/10] bg-muted/40 relative overflow-hidden">
						{#if pl.total_entries === 0}
							<div class="size-full bg-gradient-to-br from-muted/80 to-muted/20 flex flex-col items-center justify-center gap-2">
								<Loader2 class="size-8 text-muted-foreground/40 animate-spin" />
								<span class="text-[11px] text-muted-foreground/60">Fetching tracks...</span>
							</div>
						{:else if pl.cover_art_path}
							<img src={assetUrl(pl.cover_art_path)} alt={pl.name} class="size-full object-cover group-hover:scale-105 transition-transform duration-300" loading="lazy" />
						{:else}
							<div class="size-full bg-gradient-to-br from-muted/80 to-muted/20 flex items-center justify-center">
								<ListMusic class="size-10 text-muted-foreground/20" />
							</div>
						{/if}
						{#if pl.new_count > 0}
							<div class="absolute top-2 right-2">
								<span class="inline-flex items-center gap-1 rounded-full bg-primary px-2 py-0.5 text-[11px] font-semibold text-primary-foreground shadow-sm">{pl.new_count} new</span>
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
						{#if pct > 0 && pct < 100}
							<div class="absolute bottom-0 left-0 right-0 h-1 bg-black/30">
								<div class="h-full bg-primary transition-all" style="width: {pct}%"></div>
							</div>
						{:else if pct === 100}
							<div class="absolute bottom-0 left-0 right-0 h-1 bg-green-500"></div>
						{/if}
					</div>
					<div class="p-3 space-y-1.5">
						<div class="flex items-start justify-between gap-2">
							<p class="text-sm font-medium leading-snug line-clamp-2">{pl.name}</p>
							<div class="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity -mt-0.5 -mr-1">
								<Button variant="ghost" size="icon-sm" class="size-6" onclick={(e) => { e.stopPropagation(); onsyncPlaylist(pl.id); }} disabled={syncingIds.has(pl.id)}>
									<RefreshCw class="size-3 {syncingIds.has(pl.id) ? 'animate-spin' : ''}" />
								</Button>
								<Button variant="ghost" size="icon-sm" class="size-6 text-muted-foreground hover:text-destructive" onclick={(e) => { e.stopPropagation(); onremovePlaylist(pl.id); }}>
									<Trash2 class="size-3" />
								</Button>
							</div>
						</div>
						<div class="flex items-center gap-1.5 text-[11px] text-muted-foreground">
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
						{#if pl.new_count > 0}
							<Button size="sm" class="w-full gap-1.5 h-7 text-xs mt-1" onclick={(e) => { e.stopPropagation(); ondownloadAllNew(pl.id); }}>
								<ArrowDownToLine class="size-3" />
								Download {pl.new_count} new
							</Button>
						{/if}
					</div>
				</div>
			{/each}
		</div>
		<!-- Singles list -->
		{@const singles = playlists.filter(pl => pl.total_entries <= 1)}
		{#if singles.length > 0}
			<div class="space-y-1.5">
				<p class="text-xs font-medium uppercase tracking-wider text-muted-foreground/50 px-1">Singles ({singles.length})</p>
				<div class="rounded-lg border border-border/30 divide-y divide-border/20 overflow-hidden">
					{#each singles as pl (pl.id)}
						<div
							class="flex items-center gap-3 px-3 py-2 hover:bg-muted/20 transition-colors cursor-pointer group text-sm"
							role="button"
							tabindex="0"
							onclick={() => onopenPlaylist(pl)}
							onkeydown={(e) => { if (e.key === 'Enter') onopenPlaylist(pl); }}
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
								<Button variant="ghost" size="icon-sm" class="size-6 shrink-0" onclick={(e) => { e.stopPropagation(); ondownloadAllNew(pl.id); }}>
									<ArrowDownToLine class="size-3" />
								</Button>
							{/if}
							<Button variant="ghost" size="icon-sm" class="size-6 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground hover:text-destructive" onclick={(e) => { e.stopPropagation(); onremovePlaylist(pl.id); }}>
								<Trash2 class="size-3" />
							</Button>
						</div>
					{/each}
				</div>
			</div>
		{/if}
	{/if}
{/if}
