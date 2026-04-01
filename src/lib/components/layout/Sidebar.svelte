<script lang="ts">
	import { page } from '$app/state';
	import { Separator } from '$lib/components/ui/separator';

	const navItems = [
		{ href: '/', label: 'Home', icon: 'home' },
		{ href: '/search', label: 'Search', icon: 'search' },
	];

	const libraryItems = [
		{ href: '/library/songs', label: 'Songs', icon: 'music' },
		{ href: '/library/albums', label: 'Albums', icon: 'disc' },
		{ href: '/library/artists', label: 'Artists', icon: 'users' },
		{ href: '/library/playlists', label: 'Playlists', icon: 'list-music' },
	];

	const bottomItems = [
		{ href: '/downloads', label: 'Downloads', icon: 'download' },
		{ href: '/settings', label: 'Settings', icon: 'settings' },
	];

	function isActive(href: string): boolean {
		if (href === '/') return page.url.pathname === '/';
		return page.url.pathname.startsWith(href);
	}
</script>

<aside class="flex h-full w-60 flex-col bg-sidebar text-sidebar-foreground border-r border-sidebar-border">
	<div class="flex h-14 items-center gap-2 px-5">
		<div class="flex size-7 items-center justify-center rounded-lg bg-primary text-primary-foreground text-sm font-bold">
			P
		</div>
		<span class="text-lg font-semibold tracking-tight">Playlist</span>
	</div>

	<nav class="flex flex-1 flex-col gap-1 px-3 overflow-y-auto no-scrollbar">
		{#each navItems as item}
			<a
				href={item.href}
				class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors
					{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground font-medium' : 'text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
			>
				<span class="size-4 shrink-0">
					{#if item.icon === 'home'}
						<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>
					{:else if item.icon === 'search'}
						<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
					{/if}
				</span>
				{item.label}
			</a>
		{/each}

		<Separator class="my-2 bg-sidebar-border" />

		<div class="px-3 py-1">
			<span class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Library</span>
		</div>

		{#each libraryItems as item}
			<a
				href={item.href}
				class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors
					{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground font-medium' : 'text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
			>
				<span class="size-4 shrink-0">
					{#if item.icon === 'music'}
						<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>
					{:else if item.icon === 'disc'}
						<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="2"/></svg>
					{:else if item.icon === 'users'}
						<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></svg>
					{:else if item.icon === 'list-music'}
						<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><path d="M21 15V6"/><path d="M18.5 18a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z"/><path d="M12 12H3"/><path d="M16 6H3"/><path d="M12 18H3"/></svg>
					{/if}
				</span>
				{item.label}
			</a>
		{/each}

		<div class="flex-1"></div>

		<Separator class="my-2 bg-sidebar-border" />

		{#each bottomItems as item}
			<a
				href={item.href}
				class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors
					{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground font-medium' : 'text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
			>
				<span class="size-4 shrink-0">
					{#if item.icon === 'download'}
						<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/></svg>
					{:else if item.icon === 'settings'}
						<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/></svg>
					{/if}
				</span>
				{item.label}
			</a>
		{/each}

		<div class="h-2"></div>
	</nav>
</aside>
