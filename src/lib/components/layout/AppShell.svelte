<script lang="ts">
	import Titlebar from './Titlebar.svelte';
	import Sidebar from './Sidebar.svelte';
	import NowPlayingBar from './NowPlayingBar.svelte';
	import QueuePanel from './QueuePanel.svelte';
	import DebugConsole from './DebugConsole.svelte';
	import SearchPalette from './SearchPalette.svelte';
	import ShortcutsDialog from './ShortcutsDialog.svelte';
	import { player } from '$lib/stores/player.svelte';
	import { depsStore } from '$lib/stores/deps.svelte';
	import { CircleX, RefreshCw, FolderDown } from 'lucide-svelte';
	import { Button } from '$lib/components/ui/button';
	import { libraryStore } from '$lib/stores/library.svelte';
	import { downloadStore } from '$lib/stores/downloads.svelte';
	import { metadataScanStore } from '$lib/stores/metadataScan.svelte';
	import { mvDownloadStore } from '$lib/stores/mvDownloads.svelte';
	import { listen } from '@tauri-apps/api/event';
	import { getCurrentWebview } from '@tauri-apps/api/webview';
	import { importPaths } from '$lib/api/library';
	import { toast } from 'svelte-sonner';

	let { children } = $props();

	let debugOpen = $state(false);
	let searchOpen = $state(false);
	let shortcutsOpen = $state(false);
	// Last non-zero volume, restored when unmuting via the M shortcut.
	let preMuteVolume = 0.75;

	$effect(() => {
		player.init();
		depsStore.init();
		libraryStore.init();
		downloadStore.init();
		metadataScanStore.init();
		mvDownloadStore.init();
		return () => {
			player.destroy();
			libraryStore.destroy();
			downloadStore.destroy();
			metadataScanStore.destroy();
			mvDownloadStore.destroy();
		};
	});

	// Folder watch auto-imports: notify the user when new tracks appear.
	// (Library stores refresh via the separate 'library-updated' event.)
	$effect(() => {
		let unlisten: (() => void) | undefined;
		listen<{ imported: number }>('watch-import', (event) => {
			const n = event.payload.imported;
			toast.success(`${n} track${n === 1 ? '' : 's'} imported`, {
				description: 'New music found in a watched folder',
			});
		}).then((fn) => (unlisten = fn));
		return () => unlisten?.();
	});

	// --- OS file drag-and-drop import ---
	// Only OS drags carry `paths` (in the 'enter'/'drop' payloads); in-app
	// HTML5 drag-and-drop (tracks/albums onto the queue) never reaches this
	// handler with paths, so it is left alone.
	let dropActive = $state(false);
	let dropImporting = $state(false);

	async function handleFileDrop(paths: string[]) {
		if (dropImporting) return;
		dropImporting = true;
		try {
			const imported = await importPaths(paths);
			if (imported > 0) {
				toast.success(`Imported ${imported} track${imported !== 1 ? 's' : ''}`);
			} else {
				toast.info('No new tracks found', {
					description: 'Dropped items were not audio files or are already in the library'
				});
			}
		} catch (e) {
			toast.error('Import failed', { description: String(e) });
		} finally {
			dropImporting = false;
		}
	}

	$effect(() => {
		let unlistenDragDrop: (() => void) | undefined;
		getCurrentWebview()
			.onDragDropEvent((event) => {
				const payload = event.payload;
				if (payload.type === 'enter') {
					// Only react to OS file drags (paths present)
					if (payload.paths.length > 0) dropActive = true;
				} else if (payload.type === 'leave') {
					dropActive = false;
				} else if (payload.type === 'drop') {
					dropActive = false;
					if (payload.paths.length > 0) void handleFileDrop(payload.paths);
				}
			})
			.then((fn) => (unlistenDragDrop = fn))
			.catch((e) => console.error('Failed to listen for drag-drop events:', e));
		return () => unlistenDragDrop?.();
	});

	function isEditableTarget(target: EventTarget | null): boolean {
		const el = target as HTMLElement | null;
		if (!el || !el.tagName) return false;
		const tag = el.tagName;
		return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable;
	}

	function seekBy(deltaSeconds: number) {
		if (!player.currentTrack) return;
		const durationSec = player.durationMs / 1000;
		let target = player.positionMs / 1000 + deltaSeconds;
		target = Math.max(0, durationSec > 0 ? Math.min(durationSec, target) : target);
		player.seek(target);
	}

	function adjustVolume(delta: number) {
		const next = Math.min(1, Math.max(0, player.volume + delta));
		player.setVolume(next);
	}

	function toggleMute() {
		if (player.volume > 0) {
			preMuteVolume = player.volume;
			player.setVolume(0);
		} else {
			player.setVolume(preMuteVolume > 0 ? preMuteVolume : 0.75);
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		const mod = e.ctrlKey || e.metaKey;

		// Global shortcuts (work everywhere, including inputs)
		if (mod && e.shiftKey && e.key === 'D') {
			e.preventDefault();
			debugOpen = !debugOpen;
			return;
		}
		if (mod && (e.key === 'k' || e.key === 'K')) {
			e.preventDefault();
			searchOpen = !searchOpen;
			return;
		}
		if (mod && e.key === 'ArrowLeft') {
			e.preventDefault();
			player.prev();
			return;
		}
		if (mod && e.key === 'ArrowRight') {
			e.preventDefault();
			player.next();
			return;
		}
		if (mod && e.key === 'ArrowUp') {
			e.preventDefault();
			adjustVolume(0.05);
			return;
		}
		if (mod && e.key === 'ArrowDown') {
			e.preventDefault();
			adjustVolume(-0.05);
			return;
		}

		// The rest must not fire while typing or while an overlay owns the keyboard
		if (isEditableTarget(e.target)) return;
		if (searchOpen || shortcutsOpen) return;
		if (mod || e.altKey) return;

		switch (e.key) {
			case ' ':
				e.preventDefault();
				player.togglePlayPause();
				break;
			case 'ArrowLeft':
				e.preventDefault();
				seekBy(-10);
				break;
			case 'ArrowRight':
				e.preventDefault();
				seekBy(10);
				break;
			case 'm':
			case 'M':
				e.preventDefault();
				toggleMute();
				break;
			case '?':
				e.preventDefault();
				shortcutsOpen = !shortcutsOpen;
				break;
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="flex h-dvh w-dvw flex-col">
	<Titlebar onSearchClick={() => (searchOpen = true)} />
	<div class="flex flex-1 min-h-0">
		<Sidebar />
		<main class="flex-1 flex flex-col min-h-0 overflow-hidden p-6">
			{#if depsStore.setupError}
				<div class="bg-destructive/10 border border-destructive/30 text-destructive text-sm rounded-md p-3 mb-4 flex items-center gap-2">
					<CircleX class="size-4 shrink-0" />
					<span class="flex-1">Download tools failed to initialize: {depsStore.setupError}</span>
					<Button variant="outline" size="sm" onclick={() => depsStore.init(true)}>
						<RefreshCw class="size-3" />
						Retry
					</Button>
				</div>
			{/if}
			{@render children()}
		</main>
		<QueuePanel />
	</div>
	<NowPlayingBar />
	<DebugConsole bind:open={debugOpen} />
	<SearchPalette bind:open={searchOpen} />
	<ShortcutsDialog bind:open={shortcutsOpen} />

	{#if dropActive}
		<div class="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm flex items-center justify-center pointer-events-none">
			<div class="rounded-2xl border-2 border-dashed border-primary/60 bg-muted/30 px-12 py-10 flex flex-col items-center gap-3">
				<FolderDown class="size-10 text-primary" />
				<p class="text-lg font-medium">Drop to import</p>
				<p class="text-sm text-muted-foreground">Audio files and folders will be added to your library</p>
			</div>
		</div>
	{/if}
</div>
