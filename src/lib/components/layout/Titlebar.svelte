<script lang="ts">
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { Minus, Square, X, Copy } from 'lucide-svelte';

	let isMaximized = $state(false);
	const appWindow = getCurrentWindow();
	let unlistenResize: (() => void) | null = null;

	async function updateMaximizedState() {
		isMaximized = await appWindow.isMaximized();
	}

	$effect(() => {
		updateMaximizedState();
		appWindow.onResized(() => {
			updateMaximizedState();
		}).then(fn => { unlistenResize = fn; });
		return () => {
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
		await appWindow.hide();
	}
</script>

<div class="flex h-9 items-center bg-sidebar text-sidebar-foreground border-b border-sidebar-border select-none shrink-0">
	<!-- Drag region fills the entire titlebar -->
	<div class="flex-1 flex items-center h-full pl-3 gap-2" data-tauri-drag-region>
		<div class="flex size-5 items-center justify-center rounded bg-primary text-primary-foreground text-[10px] font-bold pointer-events-none">
			P
		</div>
		<span class="text-xs font-medium tracking-tight text-muted-foreground pointer-events-none">Playlist</span>
	</div>

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
			aria-label="Maximize"
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
