<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import {
		Clock,
		Loader2,
		CheckCircle2,
		XCircle,
		X,
		Trash2,
		ChevronRight,
		ChevronLeft,
	} from 'lucide-svelte';
	import type { Download as DownloadType } from '$lib/types';
	import { formatDate, platformLabel, platformColor } from '$lib/utils/format';

	let {
		history,
		historyTotal,
		historyLoaded,
		historyPage = $bindable(),
		historyPageSize,
		onloadHistory,
		onclearHistory,
	}: {
		history: DownloadType[];
		historyTotal: number;
		historyLoaded: boolean;
		historyPage: number;
		historyPageSize: number;
		onloadHistory: () => void;
		onclearHistory: () => void;
	} = $props();

	function nextPage() {
		const maxPage = Math.ceil(historyTotal / historyPageSize) - 1;
		if (historyPage < maxPage) {
			historyPage++;
			onloadHistory();
		}
	}

	function prevPage() {
		if (historyPage > 0) {
			historyPage--;
			onloadHistory();
		}
	}
</script>

{#if !historyLoaded}
	<div class="flex justify-center p-12">
		<Loader2 class="size-6 animate-spin text-muted-foreground" />
	</div>
{:else}
	<div class="flex items-center justify-between">
		<p class="text-sm text-muted-foreground">{historyTotal} total download{historyTotal !== 1 ? 's' : ''}</p>
		{#if history.length > 0}
			<Button variant="ghost" size="sm" onclick={onclearHistory} class="gap-1.5 text-muted-foreground hover:text-destructive">
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
						<div class="flex-1 min-w-0 pl-3">
							<p class="text-sm truncate">{dl.title || dl.url}</p>
							{#if dl.artist}
								<p class="text-xs text-muted-foreground/60 truncate mt-0.5">{dl.artist}</p>
							{/if}
						</div>
						<div class="w-28 text-center hidden sm:block">
							<span class="text-xs text-muted-foreground/50">{formatDate(dl.created_at)}</span>
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
				<Button variant="outline" size="sm" onclick={prevPage} disabled={historyPage === 0} class="gap-1">
					<ChevronLeft class="size-4" />
					Previous
				</Button>
				<span class="text-sm text-muted-foreground tabular-nums">
					{historyPage + 1} / {totalPages}
				</span>
				<Button variant="outline" size="sm" onclick={nextPage} disabled={historyPage >= totalPages - 1} class="gap-1">
					Next
					<ChevronRight class="size-4" />
				</Button>
			</div>
		{/if}
	{/if}
{/if}
