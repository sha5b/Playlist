<script lang="ts">
	import { listen } from '@tauri-apps/api/event';
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Progress } from '$lib/components/ui/progress';
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
	} from 'lucide-svelte';
	import { toast } from 'svelte-sonner';
	import {
		startDownload,
		cancelDownload,
		retryDownload,
		getDownloadHistory,
		clearHistory,
		checkDeps,
		ensureDeps,
		parseUrl,
	} from '$lib/api/downloads';
	import { downloadStore } from '$lib/stores/downloads.svelte';
	import type { Download as DownloadType, DepsStatus, SetupProgress } from '$lib/types';
	import { formatDate } from '$lib/utils/format';

	let urlInput = $state('');
	let submitting = $state(false);
	let depsStatus = $state<DepsStatus | null>(null);
	let depsChecking = $state(true);
	let setupInProgress = $state(false);
	let setupMessage = $state('Checking dependencies...');
	let setupProgress = $state(0);
	let setupError = $state<string | null>(null);
	let history: DownloadType[] = $state([]);
	let historyTotal = $state(0);
	let showHistory = $state(false);

	const depsReady = $derived(
		depsStatus?.ytdlp_available && depsStatus?.ffmpeg_available
	);

	// Initialize
	$effect(() => {
		downloadStore.init();
		checkAndSetup();
	});

	async function checkAndSetup() {
		depsChecking = true;
		setupError = null;

		try {
			depsStatus = await checkDeps();

			if (!depsStatus.ytdlp_available || !depsStatus.ffmpeg_available) {
				// Auto-setup: download missing dependencies
				setupInProgress = true;
				setupMessage = 'Setting up download tools...';
				setupProgress = 0;

				const unlisten = await listen<SetupProgress>('setup-progress', (event) => {
					setupMessage = event.payload.message;
					setupProgress = event.payload.progress;
				});

				try {
					await ensureDeps();
					// Re-check deps after setup
					depsStatus = await checkDeps();
					if (depsStatus.ytdlp_available && depsStatus.ffmpeg_available) {
						toast.success('Setup complete', { description: 'Download tools are ready' });
					}
				} catch (e) {
					setupError = String(e);
				} finally {
					unlisten();
					setupInProgress = false;
				}
			}
		} catch (e) {
			setupError = String(e);
		} finally {
			depsChecking = false;
		}
	}

	async function handleSubmit() {
		const url = urlInput.trim();
		if (!url) return;

		submitting = true;
		try {
			const download = await startDownload(url);
			downloadStore.addDownload(download);
			urlInput = '';
			toast.success('Download started', {
				description: download.title || url,
			});
		} catch (e) {
			toast.error('Failed to start download', {
				description: String(e),
			});
		} finally {
			submitting = false;
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			handleSubmit();
		}
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

	async function loadHistory() {
		try {
			const [data, total] = await getDownloadHistory(0, 100);
			history = data;
			historyTotal = total;
			showHistory = true;
		} catch (e) {
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
		} catch (e) {
			toast.error('Failed to clear history');
		}
	}

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
			case 'youtube':
				return 'destructive';
			case 'spotify':
				return 'default';
			case 'soundcloud':
				return 'secondary';
			default:
				return 'outline';
		}
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
</script>

<div class="space-y-6">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Downloads</h1>
		<p class="text-muted-foreground mt-1">
			Download music from YouTube, SoundCloud, and more
		</p>
	</div>

	<!-- Setup state: checking / downloading deps / error -->
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
			<Button variant="outline" size="sm" onclick={checkAndSetup}>
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
			<Button variant="outline" size="sm" onclick={checkAndSetup}>
				<RotateCcw class="size-3" />
				Retry setup
			</Button>
		</div>
	{:else}
		<!-- URL Input — only shown when deps are ready -->
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
			<Button
				onclick={handleSubmit}
				disabled={!urlInput.trim() || submitting}
			>
				{#if submitting}
					<Loader2 class="size-4 animate-spin" />
				{:else}
					<Download class="size-4" />
				{/if}
				Download
			</Button>
		</div>
	{/if}

	<!-- Active Downloads -->
	{#if activeDownloads.length > 0}
		<div class="space-y-3">
			<h2 class="text-lg font-semibold">
				Active ({activeDownloads.length})
			</h2>
			{#each activeDownloads as dl (dl.id)}
				<div class="flex items-center gap-4 rounded-lg border border-border p-4">
					<div class="flex-1 min-w-0 space-y-2">
						<div class="flex items-center gap-2">
							<p class="text-sm font-medium truncate">
								{dl.title || dl.url}
							</p>
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
					<Button
						variant="ghost"
						size="sm"
						onclick={() => handleCancel(dl.id)}
					>
						<X class="size-4" />
					</Button>
				</div>
			{/each}
		</div>
	{/if}

	<!-- Recent / Completed -->
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
						<p class="text-sm font-medium truncate">
							{dl.title || dl.url}
						</p>
						<div class="flex items-center gap-2 mt-0.5">
							<Badge variant={platformColor(dl.platform)} class="text-xs">
								{platformLabel(dl.platform)}
							</Badge>
							{#if dl.status === 'failed' && dl.error_message}
								<span class="text-xs text-destructive truncate">{dl.error_message}</span>
							{:else}
								<span class="text-xs text-muted-foreground">
									{dl.format.toUpperCase()}
								</span>
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

	<!-- Empty State -->
	{#if depsReady && activeDownloads.length === 0 && completedDownloads.length === 0}
		<div class="flex flex-col items-center justify-center h-48 rounded-lg border border-dashed border-border gap-2">
			<Download class="size-8 text-muted-foreground" />
			<p class="text-muted-foreground text-sm">No downloads yet</p>
			<p class="text-muted-foreground text-xs">Paste a URL above to get started</p>
		</div>
	{/if}

	<!-- History Toggle -->
	{#if depsReady}
		<div class="flex items-center gap-2">
			{#if !showHistory}
				<Button variant="outline" size="sm" onclick={loadHistory}>
					View download history
				</Button>
			{:else}
				<Button variant="outline" size="sm" onclick={() => (showHistory = false)}>
					Hide history
				</Button>
				{#if history.length > 0}
					<Button variant="ghost" size="sm" onclick={handleClearHistory}>
						<Trash2 class="size-4" />
						Clear history
					</Button>
				{/if}
			{/if}
			{#if depsStatus?.ytdlp_version}
				<span class="text-xs text-muted-foreground ml-auto">yt-dlp {depsStatus.ytdlp_version}</span>
			{/if}
		</div>
	{/if}

	{#if showHistory && history.length > 0}
		<div class="space-y-2">
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
</div>
