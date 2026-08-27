<script lang="ts">
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { Minus, Square, X, Copy, Search } from 'lucide-svelte';

	let { onSearchClick }: { onSearchClick?: () => void } = $props();

	let isMaximized = $state(false);
	const appWindow = getCurrentWindow();
	let unlistenResize: (() => void) | null = null;

	async function updateMaximizedState() {
		isMaximized = await appWindow.isMaximized();
	}

	$effect(() => {
		updateMaximizedState();
		let disposed = false;
		appWindow.onResized(() => {
			updateMaximizedState();
		}).then(fn => {
			// If cleanup already ran, unlisten immediately — otherwise the
			// listener (and its Tauri event registration) would leak.
			if (disposed) fn();
			else unlistenResize = fn;
		});
		return () => {
			disposed = true;
			unlistenResize?.();
			unlistenResize = null;
		};
	});

	async function minimize() {
		await appWindow.minimize();
	}

	async function toggleMaximize() {
		await appWindow.toggleMaximize();
	}

	async function close() {
		await appWindow.close();
	}
</script>

<div class="flex h-9 items-center bg-sidebar text-sidebar-foreground border-b border-sidebar-border select-none shrink-0">
	<!-- Drag region fills the entire titlebar -->
	<div class="flex-1 flex items-center h-full pl-3 gap-2" data-tauri-drag-region>
		<svg class="size-5 pointer-events-none rounded" viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
			<rect width="100" height="100" rx="22" fill="#f0efec"/>
			<rect x="30" y="25" width="56" height="34" rx="7" fill="#1a1a1a" opacity="0.09"/>
			<rect x="22" y="33" width="56" height="34" rx="7" fill="#1a1a1a" opacity="0.28"/>
			<rect x="14" y="41" width="56" height="34" rx="7" fill="#1a1a1a"/>
			<rect x="20" y="50" width="23" height="3" rx="1.5" fill="#f0efec"/>
			<rect x="20" y="56" width="17" height="3" rx="1.5" fill="#f0efec" opacity="0.55"/>
			<rect x="20" y="62" width="10" height="3" rx="1.5" fill="#f0efec" opacity="0.28"/>
			<path d="M53 51 L65 58 L53 65 Z" fill="#f0efec"/>
		</svg>
		<span class="text-xs font-medium tracking-tight text-muted-foreground pointer-events-none">Playlist</span>
	</div>

	{#if onSearchClick}
		<button
			onclick={onSearchClick}
			class="mr-1 flex h-6 items-center gap-2 rounded-md border border-sidebar-border bg-sidebar-accent/40 px-2.5 text-xs text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-foreground"
			aria-label="Search library"
		>
			<Search class="size-3" />
			<span>Search</span>
			<kbd class="rounded border border-sidebar-border bg-sidebar px-1 text-[9px] font-medium">Ctrl K</kbd>
		</button>
	{/if}

	<!-- Window controls -->
	<div class="flex items-center h-full">
		<button
			onclick={minimize}
			class="inline-flex items-center justify-center w-11 h-full text-muted-foreground hover:bg-sidebar-accent hover:text-foreground transition-colors"
			aria-label="Minimize"
		>
			<Minus class="size-3.5" />
		</button>
		<button
			onclick={toggleMaximize}
			class="inline-flex items-center justify-center w-11 h-full text-muted-foreground hover:bg-sidebar-accent hover:text-foreground transition-colors"
			aria-label={isMaximized ? 'Restore' : 'Maximize'}
		>
			{#if isMaximized}
				<Copy class="size-3" />
			{:else}
				<Square class="size-3" />
			{/if}
		</button>
		<button
			onclick={close}
			class="inline-flex items-center justify-center w-11 h-full text-muted-foreground hover:bg-red-600 hover:text-white transition-colors"
			aria-label="Close"
		>
			<X class="size-3.5" />
		</button>
	</div>
</div>
