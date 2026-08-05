// Scroll position restoration for custom scroll containers.
//
// SvelteKit's built-in scroll restoration only handles `window` scrolling.
// This app scrolls inside per-page overflow containers (under AppShell's
// overflow-hidden <main>), so saved positions are managed manually here.
//
// Usage — attach to the scrollable element:
//   <div use:scrollRestore>            // keyed by current pathname
//   <div use:scrollRestore={'my-key'}> // explicit key
//
// The action saves scrollTop as the user scrolls (keyed per route, in memory
// for the app session) and restores it when the element mounts again. Because
// list pages render asynchronously (data arrives from stores/IPC after
// mount), restoration waits until the container is tall enough to hold the
// saved position, via MutationObserver + ResizeObserver, and gives up after a
// grace period or as soon as the user interacts.

const positions = new Map<string, number>();
const MAX_ENTRIES = 100;
const RESTORE_TIMEOUT_MS = 5000;

function save(key: string, value: number) {
	// Re-insert so the map's insertion order approximates LRU when trimming.
	positions.delete(key);
	positions.set(key, value);
	if (positions.size > MAX_ENTRIES) {
		const oldest = positions.keys().next().value;
		if (oldest !== undefined) positions.delete(oldest);
	}
}

export function scrollRestore(node: HTMLElement, key?: string) {
	const k = key ?? window.location.pathname;
	const target = positions.get(k) ?? 0;

	let mo: MutationObserver | undefined;
	let ro: ResizeObserver | undefined;
	let timeout: ReturnType<typeof setTimeout> | undefined;

	function stopPending() {
		mo?.disconnect();
		mo = undefined;
		ro?.disconnect();
		ro = undefined;
		if (timeout !== undefined) {
			clearTimeout(timeout);
			timeout = undefined;
		}
		node.removeEventListener('wheel', stopPending);
		node.removeEventListener('touchstart', stopPending);
		node.removeEventListener('pointerdown', stopPending);
		node.removeEventListener('keydown', stopPending);
	}

	function tryRestore(): boolean {
		if (node.scrollHeight - node.clientHeight >= target) {
			node.scrollTop = target;
			return true;
		}
		return false;
	}

	if (target > 0 && !tryRestore()) {
		// Content isn't tall enough yet (data still loading) — retry whenever
		// the subtree or size changes, until it fits or we give up. User
		// interaction cancels the pending restore so we never yank the view
		// out from under them.
		mo = new MutationObserver(() => {
			if (tryRestore()) stopPending();
		});
		mo.observe(node, { childList: true, subtree: true });
		ro = new ResizeObserver(() => {
			if (tryRestore()) stopPending();
		});
		ro.observe(node);
		node.addEventListener('wheel', stopPending, { passive: true });
		node.addEventListener('touchstart', stopPending, { passive: true });
		node.addEventListener('pointerdown', stopPending);
		node.addEventListener('keydown', stopPending);
		timeout = setTimeout(stopPending, RESTORE_TIMEOUT_MS);
	}

	const onScroll = () => save(k, node.scrollTop);
	node.addEventListener('scroll', onScroll, { passive: true });

	return {
		destroy() {
			stopPending();
			node.removeEventListener('scroll', onScroll);
			// Safety net alongside the scroll listener; skip if already
			// detached (scrollTop would read 0 and clobber the saved value).
			if (node.isConnected) save(k, node.scrollTop);
		},
	};
}
