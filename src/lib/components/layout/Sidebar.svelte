<script lang="ts">
	import { page } from '$app/state';
	import { Separator } from '$lib/components/ui/separator';
	import { Button } from '$lib/components/ui/button';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { Home, Search, Music, Disc, Users, ListMusic, Settings, PanelLeftClose, PanelLeft, LayoutDashboard } from 'lucide-svelte';
	import { ui } from '$lib/stores/ui.svelte';

	const navItems = [
		{ href: '/', label: 'Home', icon: Home },
		{ href: '/manager', label: 'Manager', icon: LayoutDashboard },
		{ href: '/search', label: 'Search', icon: Search },
	];

	const libraryItems = [
		{ href: '/library/songs', label: 'Songs', icon: Music },
		{ href: '/library/albums', label: 'Albums', icon: Disc },
		{ href: '/library/artists', label: 'Artists', icon: Users },
		{ href: '/library/playlists', label: 'Playlists', icon: ListMusic },
	];

	const bottomItems = [
		{ href: '/settings', label: 'Settings', icon: Settings },
	];

	function isActive(href: string): boolean {
		if (href === '/') return page.url.pathname === '/';
		return page.url.pathname.startsWith(href);
	}

	const collapsed = $derived(ui.sidebarCollapsed);
</script>

<aside class="flex h-full flex-col bg-sidebar text-sidebar-foreground border-r border-sidebar-border transition-all duration-200 {collapsed ? 'w-16' : 'w-60'}">
	<div class="flex h-14 items-center {collapsed ? 'justify-center px-2' : 'justify-between px-5'}">
		{#if !collapsed}
			<div class="flex items-center gap-2">
				<div class="flex size-7 items-center justify-center rounded-lg bg-primary text-primary-foreground text-sm font-bold">
					P
				</div>
				<span class="text-lg font-semibold tracking-tight">Playlist</span>
			</div>
		{/if}
		<Button
			variant="ghost"
			size="icon-sm"
			class="text-muted-foreground hover:text-foreground shrink-0"
			onclick={() => ui.toggleSidebar()}
		>
			{#if collapsed}
				<PanelLeft class="size-4" />
			{:else}
				<PanelLeftClose class="size-4" />
			{/if}
		</Button>
	</div>

	<nav class="flex flex-1 flex-col gap-1 {collapsed ? 'px-2' : 'px-3'} overflow-y-auto no-scrollbar">
		{#each navItems as item}
			{#if collapsed}
				<Tooltip.Root>
					<Tooltip.Trigger>
						<a
							href={item.href}
							class="flex items-center justify-center rounded-lg p-2 transition-colors
								{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground' : 'text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
						>
							<item.icon class="size-4 shrink-0" />
						</a>
					</Tooltip.Trigger>
					<Tooltip.Content side="right">
						{item.label}
					</Tooltip.Content>
				</Tooltip.Root>
			{:else}
				<a
					href={item.href}
					class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors
						{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground font-medium' : 'text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
				>
					<item.icon class="size-4 shrink-0" />
					{item.label}
				</a>
			{/if}
		{/each}

		<Separator class="my-2 bg-sidebar-border" />

		{#if !collapsed}
			<div class="px-3 py-1">
				<span class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Library</span>
			</div>
		{/if}

		{#each libraryItems as item}
			{#if collapsed}
				<Tooltip.Root>
					<Tooltip.Trigger>
						<a
							href={item.href}
							class="flex items-center justify-center rounded-lg p-2 transition-colors
								{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground' : 'text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
						>
							<item.icon class="size-4 shrink-0" />
						</a>
					</Tooltip.Trigger>
					<Tooltip.Content side="right">
						{item.label}
					</Tooltip.Content>
				</Tooltip.Root>
			{:else}
				<a
					href={item.href}
					class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors
						{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground font-medium' : 'text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
				>
					<item.icon class="size-4 shrink-0" />
					{item.label}
				</a>
			{/if}
		{/each}

		<div class="flex-1"></div>

		<Separator class="my-2 bg-sidebar-border" />

		{#each bottomItems as item}
			{#if collapsed}
				<Tooltip.Root>
					<Tooltip.Trigger>
						<a
							href={item.href}
							class="flex items-center justify-center rounded-lg p-2 transition-colors
								{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground' : 'text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
						>
							<item.icon class="size-4 shrink-0" />
						</a>
					</Tooltip.Trigger>
					<Tooltip.Content side="right">
						{item.label}
					</Tooltip.Content>
				</Tooltip.Root>
			{:else}
				<a
					href={item.href}
					class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors
						{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground font-medium' : 'text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
				>
					<item.icon class="size-4 shrink-0" />
					{item.label}
				</a>
			{/if}
		{/each}

		<div class="h-2"></div>
	</nav>
</aside>
