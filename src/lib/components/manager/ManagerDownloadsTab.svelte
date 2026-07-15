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

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') onsubmit();
	}
</script>

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
			<h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
				Active ({activeCount})
			</h2>
			<Button variant="destructive" size="sm" onclick={onstopAll} class="gap-1.5">
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
					<div class="border-b border-border/40 bg-muted/15 text-xs text-muted-foreground uppercase tracking-wider flex items-center px-4 py-2">
						<div class="w-7"></div>
						<div class="flex-1 pl-3">Title</div>
						<div class="w-24 text-center">Status</div>
						<div class="w-8"></div>
					</div>
					<div class="divide-y divide-border/40">
						{#each group.downloads.slice(0, 30) as dl (dl.id)}
							<div class="flex items-center px-4 py-2.5 hover:bg-muted/30 transition-colors group">
								<div class="w-7 flex items-center justify-center shrink-0">
									{#if dl.status === 'downloading'}
										<Loader2 class="size-3.5 animate-spin text-info" />
									{:else if dl.status === 'processing'}
										<Loader2 class="size-3.5 animate-spin text-success" />
									{:else}
										<Clock class="size-3.5 text-muted-foreground/40" />
									{/if}
								</div>
								<div class="flex-1 min-w-0 pl-3">
									<div class="flex items-center gap-2">
										<p class="text-sm truncate">{dl.title || dl.url || 'Untitled'}</p>
										{#if dl.artist}
											<span class="text-xs text-muted-foreground/60 truncate shrink-0">{dl.artist}</span>
										{/if}
									</div>
									{#if dl.status === 'downloading'}
										<div class="flex items-center gap-2 mt-1">
											<Progress value={dl.progress} class="h-1 flex-1" />
											<span class="text-[10px] text-info tabular-nums shrink-0">{Math.round(dl.progress)}%</span>
										</div>
									{/if}
								</div>
								<div class="w-24 flex justify-center">
									{#if dl.status === 'downloading'}
										<Badge variant="outline" class="text-[10px] text-info border-info/30">downloading</Badge>
									{:else if dl.status === 'processing'}
										<Badge variant="outline" class="text-[10px] text-success border-success/30">importing</Badge>
									{:else}
										<Badge variant="outline" class="text-[10px] text-muted-foreground/60">queued</Badge>
									{/if}
								</div>
								<div class="w-8 flex justify-center">
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
							<div class="flex items-center px-4 py-2.5 hover:bg-muted/30 transition-colors group">
								<div class="w-7 flex items-center justify-center shrink-0">
									{#if dl.status === 'downloading'}
										<Loader2 class="size-3.5 animate-spin text-info" />
									{:else if dl.status === 'processing'}
										<Loader2 class="size-3.5 animate-spin text-success" />
									{:else}
										<Clock class="size-3.5 text-muted-foreground/40" />
									{/if}
								</div>
								<div class="flex-1 min-w-0 pl-3">
									<div class="flex items-center gap-2">
										<p class="text-sm truncate">{dl.title || dl.url || 'Untitled'}</p>
										{#if dl.artist}
											<span class="text-xs text-muted-foreground/60 truncate shrink-0">{dl.artist}</span>
										{/if}
									</div>
									{#if dl.status === 'downloading'}
										<div class="flex items-center gap-2 mt-1">
											<Progress value={dl.progress} class="h-1 flex-1" />
											<span class="text-[10px] text-info tabular-nums shrink-0">{Math.round(dl.progress)}%</span>
										</div>
									{/if}
								</div>
								<div class="w-24 flex justify-center">
									<Badge variant={platformColor(dl.platform)} class="text-[10px] shrink-0">
										{platformLabel(dl.platform)}
									</Badge>
								</div>
								<div class="w-24 flex justify-center">
									{#if dl.status === 'downloading'}
										<Badge variant="outline" class="text-[10px] text-info border-info/30">downloading</Badge>
									{:else if dl.status === 'processing'}
										<Badge variant="outline" class="text-[10px] text-success border-success/30">importing</Badge>
									{:else}
										<Badge variant="outline" class="text-[10px] text-muted-foreground/60">queued</Badge>
									{/if}
								</div>
								<div class="w-8 flex justify-center">
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
			<div class="border-b border-border bg-muted/30 text-xs text-muted-foreground uppercase tracking-wider flex items-center px-4 py-2.5">
				<div class="w-7"></div>
				<div class="flex-1 pl-3">Title</div>
				<div class="w-20 text-center hidden sm:block">Format</div>
				<div class="w-24 text-center">Platform</div>
				<div class="w-24 text-center">Status</div>
				<div class="w-8"></div>
			</div>
			<div class="divide-y divide-border/40">
				{#each completedDownloads as dl (dl.id)}
					<div class="flex items-center px-4 py-2.5 hover:bg-muted/30 transition-colors group">
						<div class="w-7 flex items-center justify-center shrink-0">
							{#if dl.status === 'completed'}
								<CheckCircle2 class="size-4 text-success" />
							{:else if dl.status === 'failed'}
								<XCircle class="size-4 text-destructive" />
							{:else}
								<X class="size-4 text-muted-foreground/30" />
							{/if}
						</div>
						<div class="flex-1 min-w-0 pl-3">
							<p class="text-sm truncate">{dl.title || dl.url || 'Untitled'}</p>
							<div class="flex items-center gap-2 mt-0.5">
								{#if dl.artist}
									<span class="text-xs text-muted-foreground/60 truncate">{dl.artist}</span>
								{/if}
								{#if dl.status === 'failed' && dl.error_message}
									<span class="text-xs text-destructive/80 truncate">{dl.error_message}</span>
								{/if}
							</div>
						</div>
						<div class="w-20 text-center hidden sm:block">
							{#if dl.status !== 'failed'}
								<span class="text-[10px] text-muted-foreground/50 font-mono uppercase">{dl.format}</span>
							{:else}
								<span class="text-[10px] text-muted-foreground/30">--</span>
							{/if}
						</div>
						<div class="w-24 flex justify-center">
							<Badge variant={platformColor(dl.platform)} class="text-[10px]">
								{platformLabel(dl.platform)}
							</Badge>
						</div>
						<div class="w-24 flex justify-center">
							<Badge
								variant="outline"
								class="text-[10px] capitalize
									{dl.status === 'completed' ? 'text-success border-success/30' : ''}
									{dl.status === 'failed' ? 'text-destructive border-destructive/30' : ''}
									{dl.status === 'cancelled' ? 'text-muted-foreground/50 border-border/40' : ''}"
							>
								{dl.status}
							</Badge>
						</div>
						<div class="w-8 flex justify-center">
							{#if dl.status === 'failed'}
								<Button
									variant="ghost"
									size="icon-sm"
									class="size-6 text-muted-foreground hover:text-foreground"
									onclick={() => onretry(dl.id)}
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
