<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onMount } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Progress } from '$lib/components/ui/progress';
	import * as Card from '$lib/components/ui/card';
	import {
		Sparkles,
		Music,
		Disc,
		CheckCircle2,
		XCircle,
		AlertTriangle,
		Loader2,
		Square,
		Play,
		Database,
		Globe,
		Radio,
		TrendingUp,
		CircleCheck,
		CircleAlert,
	} from 'lucide-svelte';
	import { metadataScanStore } from '$lib/stores/metadataScan.svelte';
	import { getMetadataStats, scanMissingMetadata, stopMetadataScan } from '$lib/api/library';
	import { toast } from 'svelte-sonner';
	import type { MetadataStats } from '$lib/types';

	interface EnrichDetail {
		item_type: 'track' | 'album';
		id: number;
		title: string;
		artist: string | null;
		status: 'success' | 'partial' | 'failed';
		sources?: Record<string, string[]>;
		error?: string;
		note?: string;
		timestamp: number;
	}

	let stats = $state<MetadataStats | null>(null);
	let feed = $state<EnrichDetail[]>([]);
	let maxFeed = 200;
	let filterType = $state<'all' | 'track' | 'album'>('all');
	let filterStatus = $state<'all' | 'success' | 'partial' | 'failed'>('all');

	let filteredFeed = $derived(
		feed.filter((item) => {
			if (filterType !== 'all' && item.item_type !== filterType) return false;
			if (filterStatus !== 'all' && item.status !== filterStatus) return false;
			return true;
		})
	);

	let successCount = $derived(feed.filter((f) => f.status === 'success').length);
	let partialCount = $derived(feed.filter((f) => f.status === 'partial').length);
	let failedCount = $derived(feed.filter((f) => f.status === 'failed').length);

	const scanning = $derived(metadataScanStore.scanning);
	const autoEnriching = $derived(metadataScanStore.autoEnriching);
	const active = $derived(metadataScanStore.active);
	const scanProgress = $derived(metadataScanStore.progress);

	async function loadStats() {
		try {
			stats = await getMetadataStats();
		} catch (e) {
			console.error('Failed to load metadata stats:', e);
		}
	}

	async function handleScan() {
		if (metadataScanStore.scanning) return;
		metadataScanStore.markScanning();
		try {
			const result = await scanMissingMetadata();
			toast.success(`Scan complete: ${result.enriched} enriched, ${result.failed} failed`);
			await loadStats();
		} catch (e) {
			toast.error('Scan failed', { description: String(e) });
		} finally {
			metadataScanStore.markDone();
		}
	}

	async function handleStop() {
		try {
			await stopMetadataScan();
			toast.success('Scan stopped');
		} catch (e) {
			console.error('Failed to stop scan:', e);
		}
	}

	function clearFeed() {
		feed = [];
	}

	const fieldLabels: Record<string, string> = {
		musicbrainz_id: 'MB ID',
		genre: 'Genre',
		release_date: 'Release Date',
		isrc: 'ISRC',
		description: 'Description',
		label: 'Label',
		language: 'Language',
		tags: 'Tags',
		music_video_url: 'Music Video',
		artist_website: 'Artist Website',
		album_purchase_url: 'Purchase URL',
		album_type: 'Album Type',
		tracklist: 'Tracklist',
		total_tracks: 'Total Tracks',
		cover_art: 'Cover Art',
	};

	let unlistenDetail: UnlistenFn | null = null;

	onMount(() => {
		loadStats();

		const interval = setInterval(loadStats, 10000);

		listen<Omit<EnrichDetail, 'timestamp'>>('metadata-enrich-detail', (event) => {
			const detail: EnrichDetail = {
				...event.payload,
				timestamp: Date.now(),
			};
			feed = [detail, ...feed].slice(0, maxFeed);
		}).then((fn) => (unlistenDetail = fn));

		return () => {
			clearInterval(interval);
			unlistenDetail?.();
		};
	});
</script>

<div class="flex h-full flex-col gap-6 overflow-y-auto">
	<!-- Header -->
	<div class="flex items-center justify-between">
		<div>
			<h1 class="text-3xl font-bold tracking-tight">Metadata</h1>
			<p class="text-muted-foreground mt-1">Enrich your library with metadata from MusicBrainz and Last.fm</p>
		</div>
		<div class="flex items-center gap-2">
			{#if active}
				<Badge variant="outline" class="gap-1.5 border-warning/40 text-warning py-1">
					<Loader2 class="size-3 animate-spin" />
					{#if scanning && scanProgress}
						Scanning {scanProgress.current}/{scanProgress.total}
					{:else if autoEnriching}
						Enriching {metadataScanStore.autoPhase} {metadataScanStore.autoCurrent}/{metadataScanStore.autoTotal}
					{:else}
						Starting...
					{/if}
				</Badge>
			{/if}
			{#if active}
				<Button variant="outline" size="sm" onclick={handleStop} class="gap-1.5">
					<Square class="size-3.5" />
					Stop
				</Button>
			{:else}
				<Button size="sm" onclick={handleScan} disabled={scanning} class="gap-1.5">
					{#if scanning}
						<Loader2 class="size-4 animate-spin" />
					{:else}
						<Play class="size-4" />
					{/if}
					Scan Missing
				</Button>
			{/if}
		</div>
	</div>

	<!-- Stats Cards -->
	{#if stats}
		<div class="grid grid-cols-2 gap-4 sm:grid-cols-4">
			<Card.Root class="border-border/60">
				<Card.Content class="p-4">
					<div class="flex items-center gap-2 mb-2">
						<div class="size-8 rounded-lg bg-primary/10 flex items-center justify-center">
							<Music class="size-4 text-primary" />
						</div>
					</div>
					<div class="text-2xl font-bold tabular-nums">{stats.total_tracks}</div>
					<div class="text-xs text-muted-foreground mt-0.5">Total Tracks</div>
				</Card.Content>
			</Card.Root>
			<Card.Root class="border-border/60">
				<Card.Content class="p-4">
					<div class="flex items-center gap-2 mb-2">
						<div class="size-8 rounded-lg flex items-center justify-center {stats.average_completeness >= 70 ? 'bg-success/10' : stats.average_completeness >= 40 ? 'bg-warning/10' : 'bg-destructive/10'}">
							<TrendingUp class="size-4 {stats.average_completeness >= 70 ? 'text-success' : stats.average_completeness >= 40 ? 'text-warning' : 'text-destructive'}" />
						</div>
					</div>
					<div class="flex items-baseline gap-1">
						<span class="text-2xl font-bold tabular-nums">{stats.average_completeness}%</span>
					</div>
					<Progress
						value={stats.average_completeness}
						class="mt-2 h-1.5"
					/>
					<div class="text-xs text-muted-foreground mt-1.5">Avg Completeness</div>
				</Card.Content>
			</Card.Root>
			<Card.Root class="border-border/60">
				<Card.Content class="p-4">
					<div class="flex items-center gap-2 mb-2">
						<div class="size-8 rounded-lg bg-success/10 flex items-center justify-center">
							<CircleCheck class="size-4 text-success" />
						</div>
					</div>
					<div class="text-2xl font-bold text-success tabular-nums">{stats.complete_tracks}</div>
					<div class="text-xs text-muted-foreground mt-0.5">Complete ({'\u2265'}80%)</div>
				</Card.Content>
			</Card.Root>
			<Card.Root class="border-border/60">
				<Card.Content class="p-4">
					<div class="flex items-center gap-2 mb-2">
						<div class="size-8 rounded-lg bg-destructive/10 flex items-center justify-center">
							<CircleAlert class="size-4 text-destructive" />
						</div>
					</div>
					<div class="text-2xl font-bold text-destructive tabular-nums">{stats.incomplete_tracks}</div>
					<div class="text-xs text-muted-foreground mt-0.5">Incomplete ({'<'}50%)</div>
				</Card.Content>
			</Card.Root>
		</div>
	{/if}

	<!-- Active Progress Banner -->
	{#if active}
		<div class="rounded-xl border border-warning/20 bg-warning/5 p-4">
			<div class="flex items-center gap-3">
				<Sparkles class="size-5 shrink-0 text-warning animate-pulse" />
				<div class="flex-1 min-w-0">
					{#if scanning && scanProgress}
						<div class="flex items-center justify-between text-sm mb-1.5">
							<span class="font-medium truncate">{scanProgress.track_title}</span>
							<span class="text-xs text-muted-foreground tabular-nums shrink-0 ml-2">
								{scanProgress.current}/{scanProgress.total}
							</span>
						</div>
						{@const pct = Math.round((scanProgress.current / scanProgress.total) * 100)}
						<div class="flex items-center gap-2">
							<Progress value={pct} class="h-1.5 flex-1" />
							<span class="text-xs text-muted-foreground tabular-nums">{pct}%</span>
						</div>
					{:else if autoEnriching}
						<div class="flex items-center justify-between text-sm mb-1.5">
							<span class="font-medium truncate">
								{metadataScanStore.autoPhase}: {metadataScanStore.autoTitle}
							</span>
							<span class="text-xs text-muted-foreground tabular-nums shrink-0 ml-2">
								{metadataScanStore.autoCurrent}/{metadataScanStore.autoTotal}
							</span>
						</div>
						{@const pct = metadataScanStore.autoTotal > 0 ? Math.round((metadataScanStore.autoCurrent / metadataScanStore.autoTotal) * 100) : 0}
						<div class="flex items-center gap-2">
							<Progress value={pct} class="h-1.5 flex-1" />
							<span class="text-xs text-muted-foreground tabular-nums">{pct}%</span>
						</div>
					{:else}
						<span class="text-sm font-medium">Starting enrichment...</span>
					{/if}
				</div>
			</div>
		</div>
	{/if}

	<!-- Feed Section -->
	<div class="flex flex-1 flex-col gap-3 min-h-0">
		<div class="flex items-center justify-between">
			<h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">Enrichment Feed</h2>
			<div class="flex items-center gap-2">
				<!-- Session summary badges -->
				{#if feed.length > 0}
					<div class="flex items-center gap-1.5 mr-2">
						<Badge variant="outline" class="gap-1 text-success border-success/30 py-0.5">
							<CheckCircle2 class="size-3" />
							{successCount}
						</Badge>
						{#if partialCount > 0}
							<Badge variant="outline" class="gap-1 text-warning border-warning/30 py-0.5">
								<AlertTriangle class="size-3" />
								{partialCount}
							</Badge>
						{/if}
						{#if failedCount > 0}
							<Badge variant="outline" class="gap-1 text-destructive border-destructive/30 py-0.5">
								<XCircle class="size-3" />
								{failedCount}
							</Badge>
						{/if}
					</div>
				{/if}

				<!-- Filters -->
				<select
					class="h-7 rounded-md border border-input bg-background px-2 text-xs focus:outline-none focus:ring-1 focus:ring-ring"
					bind:value={filterType}
				>
					<option value="all">All Types</option>
					<option value="track">Tracks</option>
					<option value="album">Albums</option>
				</select>
				<select
					class="h-7 rounded-md border border-input bg-background px-2 text-xs focus:outline-none focus:ring-1 focus:ring-ring"
					bind:value={filterStatus}
				>
					<option value="all">All Status</option>
					<option value="success">Success</option>
					<option value="partial">Partial</option>
					<option value="failed">Failed</option>
				</select>

				{#if feed.length > 0}
					<Button variant="ghost" size="sm" onclick={clearFeed} class="text-xs text-muted-foreground h-7 px-2">
						Clear
					</Button>
				{/if}
			</div>
		</div>

		<!-- Feed items -->
		<div class="flex-1 overflow-y-auto min-h-0">
			{#if filteredFeed.length === 0}
				<div class="flex flex-col items-center justify-center py-20 gap-4">
					<div class="size-14 rounded-2xl bg-muted/50 flex items-center justify-center">
						<Sparkles class="size-7 text-muted-foreground/40" />
					</div>
					<div class="text-center space-y-1">
						<p class="text-muted-foreground font-medium">
							{#if feed.length === 0}
								No enrichment activity yet
							{:else}
								No items match current filters
							{/if}
						</p>
						<p class="text-muted-foreground/60 text-sm">
							{#if feed.length === 0}
								Start a scan or wait for background enrichment
							{:else}
								Try adjusting the filters above
							{/if}
						</p>
					</div>
				</div>
			{:else}
				<div class="rounded-xl border border-border/60 overflow-hidden divide-y divide-border/30">
					{#each filteredFeed as item (item.id + '-' + item.timestamp)}
						<div class="flex items-start gap-3 px-4 py-3 hover:bg-muted/20 transition-colors group">
							<!-- Status icon -->
							<div
								class="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-full
									{item.status === 'success' ? 'bg-success/10 text-success' :
									 item.status === 'partial' ? 'bg-warning/10 text-warning' :
									 'bg-destructive/10 text-destructive'}"
							>
								{#if item.status === 'success'}
									<CheckCircle2 class="size-3.5" />
								{:else if item.status === 'partial'}
									<AlertTriangle class="size-3.5" />
								{:else}
									<XCircle class="size-3.5" />
								{/if}
							</div>

							<!-- Content -->
							<div class="flex-1 min-w-0">
								<div class="flex items-center gap-2">
									<Badge variant="outline" class="text-[10px] px-1.5 py-0 shrink-0">
										{#if item.item_type === 'track'}
											<Music class="size-2.5 mr-0.5" />
										{:else}
											<Disc class="size-2.5 mr-0.5" />
										{/if}
										{item.item_type}
									</Badge>
									<span class="font-medium text-sm truncate">{item.title}</span>
									{#if item.artist}
										<span class="text-xs text-muted-foreground truncate shrink-0">by {item.artist}</span>
									{/if}
								</div>

								{#if item.status === 'failed'}
									<p class="mt-1 text-xs text-red-400/80">{item.error}</p>
								{:else}
									<!-- Sources & fields enriched -->
									<div class="mt-1.5 flex flex-wrap gap-x-4 gap-y-1">
										{#each Object.entries(item.sources || {}) as [source, fields]}
											{#if fields.length > 0}
												<div class="flex items-center gap-1.5">
													{#if source === 'musicbrainz'}
														<Database class="size-3 text-info" />
														<span class="text-[10px] font-medium text-info">MusicBrainz</span>
													{:else if source === 'lastfm'}
														<Radio class="size-3 text-red-400" />
														<span class="text-[10px] font-medium text-red-400">Last.fm</span>
													{:else}
														<Globe class="size-3 text-muted-foreground" />
														<span class="text-[10px] font-medium text-muted-foreground">{source}</span>
													{/if}
													<span class="text-[10px] text-muted-foreground/70">
														{fields.map((f: string) => fieldLabels[f] || f).join(', ')}
													</span>
												</div>
											{/if}
										{/each}
									</div>
									{#if item.note}
										<p class="mt-1 text-[10px] text-muted-foreground/60 italic">{item.note}</p>
									{/if}
								{/if}
							</div>

							<!-- Timestamp -->
							<span class="shrink-0 text-[10px] text-muted-foreground/40 tabular-nums mt-0.5">
								{new Date(item.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}
							</span>
						</div>
					{/each}
				</div>
			{/if}
		</div>
	</div>
</div>
