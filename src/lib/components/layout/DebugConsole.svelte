<script lang="ts">
	import { onMount } from 'svelte';
	import { attachLogger, LogLevel } from '@tauri-apps/plugin-log';
	import { X, Terminal, Trash2, ArrowDown } from 'lucide-svelte';
	import { Button } from '$lib/components/ui/button';

	let { open = $bindable(false) }: { open: boolean } = $props();

	interface LogEntry {
		level: string;
		message: string;
		timestamp: string;
	}

	let logs: LogEntry[] = $state([]);
	let scrollContainer: HTMLDivElement | undefined = $state();
	let autoScroll = $state(true);
	let filter = $state('all');

	const MAX_LOGS = 500;

	const levelNames: Record<number, string> = {
		[LogLevel.Trace]: 'TRACE',
		[LogLevel.Debug]: 'DEBUG',
		[LogLevel.Info]: 'INFO',
		[LogLevel.Warn]: 'WARN',
		[LogLevel.Error]: 'ERROR',
	};

	const levelColors: Record<string, string> = {
		TRACE: 'text-zinc-500',
		DEBUG: 'text-zinc-400',
		INFO: 'text-blue-400',
		WARN: 'text-yellow-400',
		ERROR: 'text-red-400',
	};

	function addLog(level: string, message: string) {
		const now = new Date();
		const timestamp = now.toLocaleTimeString('en-US', { hour12: false, fractionalSecondDigits: 3 });
		logs.push({ level, message, timestamp });
		if (logs.length > MAX_LOGS) {
			logs = logs.slice(-MAX_LOGS);
		}
		if (autoScroll) {
			requestAnimationFrame(() => {
				scrollContainer?.scrollTo({ top: scrollContainer.scrollHeight });
			});
		}
	}

	let filteredLogs = $derived(
		filter === 'all' ? logs : logs.filter((l) => l.level === filter)
	);

	onMount(() => {
		let detach: (() => void) | undefined;
		attachLogger(({ level, message }) => {
			addLog(levelNames[level] ?? 'INFO', message);
		}).then((d) => {
			detach = d;
		});
		return () => detach?.();
	});

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			open = false;
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
	<div class="fixed inset-x-0 bottom-[72px] z-50 flex flex-col bg-zinc-950/95 backdrop-blur border-t border-zinc-700 shadow-2xl"
		style="height: 280px;"
	>
		<!-- Header -->
		<div class="flex items-center justify-between px-3 py-1.5 border-b border-zinc-800 shrink-0">
			<div class="flex items-center gap-2">
				<Terminal class="size-4 text-zinc-400" />
				<span class="text-xs font-medium text-zinc-300">Debug Console</span>
				<span class="text-xs text-zinc-500">{logs.length} entries</span>
			</div>
			<div class="flex items-center gap-1">
				<!-- Filter buttons -->
				{#each ['all', 'INFO', 'WARN', 'ERROR', 'DEBUG'] as level}
					<button
						class="px-2 py-0.5 text-[10px] font-mono rounded transition-colors
							{filter === level
								? 'bg-zinc-700 text-zinc-200'
								: 'text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800'}"
						onclick={() => (filter = level)}
					>
						{level}
					</button>
				{/each}
				<div class="w-px h-4 bg-zinc-700 mx-1"></div>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-6 text-zinc-500 hover:text-zinc-300"
					onclick={() => (autoScroll = !autoScroll)}
					title={autoScroll ? 'Auto-scroll ON' : 'Auto-scroll OFF'}
				>
					<ArrowDown class="size-3.5 {autoScroll ? 'text-blue-400' : ''}" />
				</Button>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-6 text-zinc-500 hover:text-zinc-300"
					onclick={() => (logs = [])}
					title="Clear logs"
				>
					<Trash2 class="size-3.5" />
				</Button>
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-6 text-zinc-500 hover:text-zinc-300"
					onclick={() => (open = false)}
					title="Close (Esc)"
				>
					<X class="size-3.5" />
				</Button>
			</div>
		</div>

		<!-- Log entries -->
		<div
			bind:this={scrollContainer}
			class="flex-1 overflow-y-auto font-mono text-[11px] leading-5 px-3 py-1"
		>
			{#if filteredLogs.length === 0}
				<div class="flex items-center justify-center h-full text-zinc-600 text-xs">
					{logs.length === 0 ? 'Waiting for log events...' : 'No matching logs'}
				</div>
			{:else}
				{#each filteredLogs as log}
					<div class="flex gap-2 hover:bg-zinc-900/50 px-1 rounded">
						<span class="text-zinc-600 shrink-0 select-none">{log.timestamp}</span>
						<span class="{levelColors[log.level] ?? 'text-zinc-400'} shrink-0 w-12 text-right select-none">
							{log.level}
						</span>
						<span class="text-zinc-300 break-all">{log.message}</span>
					</div>
				{/each}
			{/if}
		</div>
	</div>
{/if}
