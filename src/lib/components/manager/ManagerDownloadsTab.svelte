<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Progress } from '$lib/components/ui/progress';
	import {
		Download,
		Disc,
		Link,
		Loader2,
		CheckCircle2,
		XCircle,
		RotateCcw,
		X,
		Clock,
		ChevronRight,
		ChevronLeft,
		Info,
		Square,
		Gauge,
		Timer,
	} from 'lucide-svelte';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import type { Download as DownloadType } from '$lib/types';
	import { platformLabel, platformColor } from '$lib/utils/format';
	import type { DownloadGroup } from '$lib/utils/grouping';

	let {
		urlInput = $bindable(),
		submitting,
		activeDownloads,
		activeCount,
		allCompletedDownloads,
		groupedActiveDownloads,
		completedPage = $bindable(),
		completedPageSize,
		onsubmit,
		oncancel,
		onretry,
		onstopAll,
	}: {
		urlInput: string;
		submitting: boolean;
		activeDownloads: DownloadType[];
		activeCount: number;
		allCompletedDownloads: DownloadType[];
		groupedActiveDownloads: DownloadGroup[];
		completedPage: number;
		completedPageSize: number;
		onsubmit: () => void;
		oncancel: (id: number) => void;
		onretry: (id: number) => void;
		onstopAll: () => void;
	} = $props();

	const completedTotalPages = $derived(Math.ceil(allCompletedDownloads.length / completedPageSize));
	const completedDownloads = $derived(
		allCompletedDownloads.slice(completedPage * completedPageSize, (completedPage + 1) * completedPageSize)
	);

	// Per-state counts for the section headers (window counts; downloading and
	// converting are always retained in the window so those are exact).
	const downloadingCount = $derived(activeDownloads.filter((d) => d.status === 'downloading').length);
	const convertingCount = $derived(activeDownloads.filter((d) => d.status === 'processing').length);
	const queuedCount = $derived(Math.max(0, activeCount - downloadingCount - convertingCount));
	const doneCount = $derived(allCompletedDownloads.filter((d) => d.status === 'completed').length);
	const failedCount = $derived(allCompletedDownloads.filter((d) => d.status === 'failed').length);

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') onsubmit();
	}
</script>

{#snippet statusBadge(status: string)}
	{#if status === 'downloading'}
		<Badge variant="outline" class="text-[10px] text-info border-info/30">downloading</Badge>
	{:else if status === 'processing'}
		<Badge variant="outline" class="text-[10px] text-warning border-warning/30">converting</Badge>
	{:else}
		<Badge variant="outline" class="text-[10px] text-muted-foreground/60">queued</Badge>
	{/if}
{/snippet}

{#snippet activeRow(dl: DownloadType, showPlatform: boolean)}
	<div
		class="flex items-center gap-3 px-4 py-2.5 transition-colors group border-l-2
			{dl.status === 'downloading' ? 'border-info/60 bg-info/[0.04]' : ''}
			{dl.status === 'processing' ? 'border-warning/60 bg-warning/[0.04]' : ''}
			{dl.status === 'queued' ? 'border-transparent hover:bg-muted/30' : ''}"
	>
		<!-- Cover / status tile -->
		<div class="size-9 rounded-md bg-muted/40 flex items-center justify-center shrink-0">
			{#if dl.status === 'downloading'}
				<Loader2 class="size-4 animate-spin text-info" />
			{:else if dl.status === 'processing'}
				<Loader2 class="size-4 animate-spin text-warning" />
			{:else}
				<Clock class="size-4 text-muted-foreground/40" />
			{/if}
		</div>
		<div class="flex-1 min-w-0">
			<div class="flex items-center gap-2">
				<p class="text-sm truncate">{dl.title || dl.url || 'Untitled'}</p>
				{#if dl.artist}
					<span class="text-xs text-muted-foreground/60 truncate shrink-0">{dl.artist}</span>
				{/if}
			</div>
			{#if dl.status === 'downloading'}
				<div class="flex items-center gap-2 mt-1">
					<Progress value={dl.progress} class="h-1 flex-1" />
					<span class="text-[10px] text-info tabular-nums shrink-0 w-8 text-right">{Math.round(dl.progress)}%</span>
					{#if dl.speed}
						<span class="hidden sm:inline-flex items-center gap-1 text-[10px] text-muted-foreground tabular-nums shrink-0">
							<Gauge class="size-3 text-muted-foreground/50" />
							{dl.speed}
						</span>
					{/if}
					{#if dl.eta}
						<span class="hidden sm:inline-flex items-center gap-1 text-[10px] text-muted-foreground tabular-nums shrink-0">
							<Timer class="size-3 text-muted-foreground/50" />
							{dl.eta}
						</span>
					{/if}
				</div>
			{:else if dl.status === 'processing'}
				<p class="text-[10px] text-warning/80 mt-0.5">Converting and importing into library...</p>
			{/if}
		</div>
		{#if showPlatform && dl.platform}
			<div class="w-24 hidden sm:flex justify-center shrink-0">
				<Badge variant={platformColor(dl.platform)} class="text-[10px]">
					{platformLabel(dl.platform)}
				</Badge>
			</div>
		{/if}
		<div class="w-24 flex justify-center shrink-0">
			{@render statusBadge(dl.status)}
		</div>
		<div class="w-8 flex justify-center shrink-0">
			<Button
				variant="ghost"
				size="icon-sm"
				class="size-6 opacity-0 group-hover:opacity-100 transition-opacity text-muted-foreground"
				onclick={() => oncancel(dl.id)}
			>
				<X class="size-3.5" />
			</Button>
		</div>
	</div>
{/snippet}

<!-- URL Input -->
<div class="flex gap-2">
	<div class="relative flex-1">
		<Link class="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
		<Input
			placeholder="Paste a URL (YouTube, SoundCloud, Bandcamp...)"
			class="pl-10"
			bind:value={urlInput}
			onkeydown={handleKeydown}
		/>
	</div>
	<Button onclick={onsubmit} disabled={!urlInput.trim() || submitting} class="gap-1.5">
		{#if submitting}
			<Loader2 class="size-4 animate-spin" />
		{:else}
			<Download class="size-4" />
		{/if}
		Download
	</Button>
</div>

<!-- Cookie hint — only while idle, since cookies should be changed when nothing is downloading -->
{#if activeCount === 0}
	<Tooltip.Provider delayDuration={150}>
		<Tooltip.Root>
			<Tooltip.Trigger class="inline-flex items-center gap-1.5 text-xs text-muted-foreground/70 hover:text-muted-foreground transition-colors">
				<Info class="size-3.5" />
				Downloads getting blocked? Set browser cookies
			</Tooltip.Trigger>
			<Tooltip.Content class="max-w-xs">
				<p class="text-xs leading-relaxed">
					No browser cookies are used by default. If YouTube blocks a download with a
					bot-detection error, open <span class="font-medium">Settings → Browser Cookies</span>
					and pick the browser you're signed into YouTube with. Best changed while nothing is downloading.
				</p>
			</Tooltip.Content>
		</Tooltip.Root>
	</Tooltip.Provider>
{/if}

<!-- Active Downloads -->
{#if activeCount > 0}
	<div class="space-y-3">
		<div class="flex items-center justify-between gap-2">
			<div class="flex items-center gap-3 min-w-0">
				<h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground shrink-0">
					Active ({activeCount})
				</h2>
				<div class="flex items-center gap-1.5 flex-wrap">
					{#if downloadingCount > 0}
						<Badge variant="outline" class="text-[10px] text-info border-info/30 gap-1">
							<Loader2 class="size-2.5 animate-spin" />
							{downloadingCount} downloading
						</Badge>
					{/if}
					{#if convertingCount > 0}
						<Badge variant="outline" class="text-[10px] text-warning border-warning/30">
							{convertingCount} converting
						</Badge>
					{/if}
					{#if queuedCount > 0}
						<Badge variant="outline" class="text-[10px] text-muted-foreground/60">
							{queuedCount} queued
						</Badge>
					{/if}
				</div>
			</div>
			<Button variant="destructive" size="sm" onclick={onstopAll} class="gap-1.5 shrink-0">
				<Square class="size-3.5" />
				Stop all
			</Button>
		</div>
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
					<div class="divide-y divide-border/40">
						{#each group.downloads.slice(0, 30) as dl (dl.id)}
							{@render activeRow(dl, false)}
						{/each}
						{#if group.downloads.length > 30}
							<div class="text-xs text-muted-foreground/50 text-center py-2.5 bg-muted/10">
								+{group.downloads.length - 30} more queued
							</div>
						{/if}
					</div>
				</div>
			{:else}
				<!-- Ungrouped downloads -->
				<div class="rounded-xl border border-border/60 overflow-hidden">
					<div class="divide-y divide-border/40">
						{#each group.downloads.slice(0, 30) as dl (dl.id)}
							{@render activeRow(dl, true)}
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
		<div class="flex items-center gap-3">
			<h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
				Recent
			</h2>
			<div class="flex items-center gap-1.5">
				{#if doneCount > 0}
					<Badge variant="outline" class="text-[10px] text-success border-success/30">{doneCount} done</Badge>
				{/if}
				{#if failedCount > 0}
					<Badge variant="outline" class="text-[10px] text-destructive border-destructive/30">{failedCount} failed</Badge>
				{/if}
			</div>
		</div>
		<div class="rounded-xl border border-border/60 overflow-hidden">
			<div class="border-b border-border bg-muted/30 text-xs text-muted-foreground uppercase tracking-wider flex items-center gap-3 px-4 py-2.5">
				<div class="w-9 shrink-0"></div>
				<div class="flex-1">Title</div>
				<div class="w-20 text-center hidden sm:block shrink-0">Format</div>
				<div class="w-24 text-center shrink-0">Platform</div>
				<div class="w-24 text-center shrink-0">Status</div>
				<div class="w-16 shrink-0"></div>
			</div>
			<div class="divide-y divide-border/40">
				{#each completedDownloads as dl (dl.id)}
					<div
						class="flex items-center gap-3 px-4 py-2.5 transition-colors group border-l-2
							{dl.status === 'failed' ? 'border-destructive/50 bg-destructive/[0.04] hover:bg-destructive/[0.07]' : 'border-transparent hover:bg-muted/30'}"
					>
						<div class="size-9 rounded-md bg-muted/40 flex items-center justify-center shrink-0">
							{#if dl.status === 'completed'}
								<CheckCircle2 class="size-4 text-success" />
							{:else if dl.status === 'failed'}
								<XCircle class="size-4 text-destructive" />
							{:else}
								<X class="size-4 text-muted-foreground/30" />
							{/if}
						</div>
						<div class="flex-1 min-w-0">
							<div class="flex items-center gap-2">
								<p class="text-sm truncate">{dl.title || dl.url || 'Untitled'}</p>
								{#if dl.artist}
									<span class="text-xs text-muted-foreground/60 truncate shrink-0">{dl.artist}</span>
								{/if}
							</div>
							{#if dl.status === 'failed' && dl.error_message}
								<p class="text-xs text-destructive/80 truncate mt-0.5" title={dl.error_message}>{dl.error_message}</p>
							{/if}
						</div>
						<div class="w-20 text-center hidden sm:block shrink-0">
							{#if dl.status !== 'failed'}
								<span class="text-[10px] text-muted-foreground/50 font-mono uppercase">{dl.format}</span>
							{:else}
								<span class="text-[10px] text-muted-foreground/30">--</span>
							{/if}
						</div>
						<div class="w-24 flex justify-center shrink-0">
							<Badge variant={platformColor(dl.platform)} class="text-[10px]">
								{platformLabel(dl.platform)}
							</Badge>
						</div>
						<div class="w-24 flex justify-center shrink-0">
							<Badge
								variant="outline"
								class="text-[10px] capitalize
									{dl.status === 'completed' ? 'text-success border-success/30' : ''}
									{dl.status === 'failed' ? 'text-destructive border-destructive/30' : ''}
									{dl.status === 'cancelled' ? 'text-muted-foreground/50 border-border/40' : ''}"
							>
								{dl.status === 'completed' ? 'done' : dl.status === 'failed' ? 'error' : dl.status}
							</Badge>
						</div>
						<div class="w-16 flex justify-end shrink-0">
							{#if dl.status === 'failed'}
								<Button
									variant="outline"
									size="sm"
									class="h-6 px-2 gap-1 text-xs"
									onclick={() => onretry(dl.id)}
								>
									<RotateCcw class="size-3" />
									Retry
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
{#if activeCount === 0 && allCompletedDownloads.length === 0}
	<div class="flex flex-col items-center justify-center py-20 rounded-xl border border-dashed border-border/60 gap-4">
		<div class="size-16 rounded-2xl bg-muted/30 flex items-center justify-center">
			<Download class="size-8 text-muted-foreground/30" />
		</div>
		<div class="text-center space-y-1.5 max-w-sm">
			<p class="font-medium">No downloads yet</p>
			<p class="text-muted-foreground/50 text-sm leading-relaxed">Paste a URL above to download a single track or video</p>
		</div>
	</div>
{/if}
