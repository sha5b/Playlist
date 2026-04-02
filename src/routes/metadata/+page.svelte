<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onMount } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import * as Card from '$lib/components/ui/card';
	import {
		Sparkles,
		Music,
		Disc,
		CheckCircle2,
		XCircle,
		AlertTriangle,
		Loader2,
		CircleX,
		Play,
		Database,
		Globe,
		Radio,
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
		} catch {}
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
		} catch {}
	}

	function clearFeed() {
		feed = [];
	}

	const sourceLabels: Record<string, { label: string; icon: typeof Database }> = {
		musicbrainz: { label: 'MusicBrainz', icon: Database },
		lastfm: { label: 'Last.fm', icon: Radio },
	};

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

<div class="flex h-full flex-col gap-6 overflow-y-auto p-6">
	<div class="flex items-center justify-between">
		<div>
			<h1 class="text-2xl font-bold">Metadata</h1>
			<p class="text-sm text-muted-foreground">Live enrichment feed and library metadata overview</p>
		</div>
		<div class="flex items-center gap-2">
			{#if active}
				<Badge variant="outline" class="gap-1.5 border-amber-400/50 text-amber-400">
					<Loader2 class="size-3 animate-spin" />
					{#if scanning && scanProgress}
						Scanning {scanProgress.current}/{scanProgress.total}
					{:else if autoEnriching}
						Auto-enriching {metadataScanStore.autoPhase} {metadataScanStore.autoCurrent}/{metadataScanStore.autoTotal}
					{:else}
						Enriching...
					{/if}
				</Badge>
			{/if}
			<Button variant="outline" size="sm" onclick={handleScan} disabled={scanning}>
				{#if scanning}
					<Loader2 class="size-4 animate-spin" />
				{:else}
					<Play class="size-4" />
				{/if}
				Scan Missing
			</Button>
			{#if active}
				<Button variant="outline" size="sm" onclick={handleStop}>
					<CircleX class="size-4" />
					Stop
				</Button>
			{/if}
		</div>
	</div>

	<!-- Stats Cards -->
	{#if stats}
		<div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
			<Card.Root>
				<Card.Content class="p-4">
					<div class="text-sm text-muted-foreground">Total Tracks</div>
					<div class="text-2xl font-bold">{stats.total_tracks}</div>
				</Card.Content>
			</Card.Root>
			<Card.Root>
				<Card.Content class="p-4">
					<div class="text-sm text-muted-foreground">Avg Completeness</div>
					<div class="flex items-baseline gap-1">
						<span class="text-2xl font-bold">{stats.average_completeness}%</span>
					</div>
					<div class="mt-1 h-1.5 w-full rounded-full bg-muted">
						<div
							class="h-1.5 rounded-full transition-all {stats.average_completeness >= 70 ? 'bg-green-500' : stats.average_completeness >= 40 ? 'bg-amber-500' : 'bg-red-500'}"
							style="width: {stats.average_completeness}%"
						></div>
					</div>
				</Card.Content>
			</Card.Root>
			<Card.Root>
				<Card.Content class="p-4">
					<div class="text-sm text-muted-foreground">Complete ({'\u2265'}80%)</div>
					<div class="text-2xl font-bold text-green-500">{stats.complete_tracks}</div>
				</Card.Content>
			</Card.Root>
			<Card.Root>
				<Card.Content class="p-4">
					<div class="text-sm text-muted-foreground">Incomplete ({'<'}50%)</div>
					<div class="text-2xl font-bold text-red-500">{stats.incomplete_tracks}</div>
				</Card.Content>
			</Card.Root>
		</div>
	{/if}

	<!-- Progress Bar -->
	{#if active}
		<Card.Root class="border-amber-400/30 bg-amber-400/5">
			<Card.Content class="p-4">
				<div class="flex items-center gap-3">
					<Sparkles class="size-5 shrink-0 text-amber-400 animate-pulse" />
					<div class="flex-1">
						{#if scanning && scanProgress}
							<div class="flex items-center justify-between text-sm">
								<span class="font-medium">Scanning: {scanProgress.track_title}</span>
								<span class="text-muted-foreground">{scanProgress.current}/{scanProgress.total}</span>
							</div>
							<div class="mt-1.5 h-2 w-full rounded-full bg-amber-400/20">
								<div
									class="h-2 rounded-full bg-amber-400 transition-all"
									style="width: {Math.round((scanProgress.current / scanProgress.total) * 100)}%"
								></div>
							</div>
						{:else if autoEnriching}
							<div class="flex items-center justify-between text-sm">
								<span class="font-medium">
									Auto-enriching {metadataScanStore.autoPhase}: {metadataScanStore.autoTitle}
								</span>
								<span class="text-muted-foreground">
									{metadataScanStore.autoCurrent}/{metadataScanStore.autoTotal}
								</span>
							</div>
							<div class="mt-1.5 h-2 w-full rounded-full bg-amber-400/20">
								<div
									class="h-2 rounded-full bg-amber-400 transition-all"
									style="width: {metadataScanStore.autoTotal > 0 ? Math.round((metadataScanStore.autoCurrent / metadataScanStore.autoTotal) * 100) : 0}%"
								></div>
							</div>
						{:else}
							<span class="text-sm font-medium">Starting enrichment...</span>
						{/if}
					</div>
				</div>
			</Card.Content>
		</Card.Root>
	{/if}

	<!-- Feed Section -->
	<div class="flex flex-1 flex-col gap-3 min-h-0">
		<div class="flex items-center justify-between">
			<h2 class="text-lg font-semibold">Enrichment Feed</h2>
			<div class="flex items-center gap-2">
				<!-- Session summary badges -->
				{#if feed.length > 0}
					<Badge variant="outline" class="gap-1 text-green-500 border-green-500/30">
						<CheckCircle2 class="size-3" />
						{successCount}
					</Badge>
					{#if partialCount > 0}
						<Badge variant="outline" class="gap-1 text-amber-400 border-amber-400/30">
							<AlertTriangle class="size-3" />
							{partialCount}
						</Badge>
					{/if}
					{#if failedCount > 0}
						<Badge variant="outline" class="gap-1 text-red-500 border-red-500/30">
							<XCircle class="size-3" />
							{failedCount}
						</Badge>
					{/if}
				{/if}

				<!-- Filters -->
				<select
					class="h-8 rounded-md border border-input bg-background px-2 text-xs"
					bind:value={filterType}
				>
					<option value="all">All Types</option>
					<option value="track">Tracks</option>
					<option value="album">Albums</option>
				</select>
				<select
					class="h-8 rounded-md border border-input bg-background px-2 text-xs"
					bind:value={filterStatus}
				>
					<option value="all">All Status</option>
					<option value="success">Success</option>
					<option value="partial">Partial</option>
					<option value="failed">Failed</option>
				</select>

				{#if feed.length > 0}
					<Button variant="ghost" size="sm" onclick={clearFeed} class="text-xs text-muted-foreground">
						Clear
					</Button>
				{/if}
			</div>
		</div>

		<div class="flex-1 overflow-y-auto space-y-2 min-h-0">
			{#if filteredFeed.length === 0}
				<div class="flex flex-col items-center justify-center py-16 text-muted-foreground">
					<Sparkles class="size-10 mb-3 opacity-30" />
					<p class="text-sm">
						{#if feed.length === 0}
							No enrichment activity yet. Start a scan or wait for background enrichment.
						{:else}
							No items match the current filters.
						{/if}
					</p>
				</div>
			{:else}
				{#each filteredFeed as item (item.id + '-' + item.timestamp)}
					<div
						class="group rounded-lg border p-3 transition-colors
							{item.status === 'success' ? 'border-green-500/20 bg-green-500/5 hover:border-green-500/40' :
							 item.status === 'partial' ? 'border-amber-400/20 bg-amber-400/5 hover:border-amber-400/40' :
							 'border-red-500/20 bg-red-500/5 hover:border-red-500/40'}"
					>
						<div class="flex items-start gap-3">
							<!-- Icon -->
							<div
								class="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-full
									{item.status === 'success' ? 'bg-green-500/10 text-green-500' :
									 item.status === 'partial' ? 'bg-amber-400/10 text-amber-400' :
									 'bg-red-500/10 text-red-500'}"
							>
								{#if item.status === 'success'}
									<CheckCircle2 class="size-4" />
								{:else if item.status === 'partial'}
									<AlertTriangle class="size-4" />
								{:else}
									<XCircle class="size-4" />
								{/if}
							</div>

							<!-- Content -->
							<div class="flex-1 min-w-0">
								<div class="flex items-center gap-2">
									<Badge variant="outline" class="text-[10px] px-1.5 py-0">
										{#if item.item_type === 'track'}
											<Music class="size-2.5 mr-0.5" />
										{:else}
											<Disc class="size-2.5 mr-0.5" />
										{/if}
										{item.item_type}
									</Badge>
									<span class="font-medium text-sm truncate">{item.title}</span>
									{#if item.artist}
										<span class="text-xs text-muted-foreground truncate">by {item.artist}</span>
									{/if}
								</div>

								{#if item.status === 'failed'}
									<p class="mt-1 text-xs text-red-400">{item.error}</p>
								{:else}
									<!-- Sources & Fields -->
									<div class="mt-1.5 flex flex-wrap gap-x-4 gap-y-1">
										{#each Object.entries(item.sources || {}) as [source, fields]}
											{#if fields.length > 0}
												<div class="flex items-center gap-1.5">
													{#if source === 'musicbrainz'}
														<Database class="size-3 text-blue-400" />
														<span class="text-[10px] font-medium text-blue-400">MusicBrainz</span>
													{:else if source === 'lastfm'}
														<Radio class="size-3 text-red-400" />
														<span class="text-[10px] font-medium text-red-400">Last.fm</span>
													{:else}
														<Globe class="size-3 text-muted-foreground" />
														<span class="text-[10px] font-medium text-muted-foreground">{source}</span>
													{/if}
													<span class="text-[10px] text-muted-foreground">
														{fields.map((f: string) => fieldLabels[f] || f).join(', ')}
													</span>
												</div>
											{/if}
										{/each}
									</div>
									{#if item.note}
										<p class="mt-1 text-[10px] text-muted-foreground italic">{item.note}</p>
									{/if}
								{/if}
							</div>

							<!-- Timestamp -->
							<span class="shrink-0 text-[10px] text-muted-foreground/50">
								{new Date(item.timestamp).toLocaleTimeString()}
							</span>
						</div>
					</div>
				{/each}
			{/if}
		</div>
	</div>
</div>
