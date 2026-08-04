<script lang="ts">
	import { page } from '$app/state';
	import { Separator } from '$lib/components/ui/separator';
	import { Button } from '$lib/components/ui/button';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { Home, Music, Disc, Users, ListMusic, Settings, PanelLeftClose, PanelLeft, LayoutDashboard, Download, AudioLines, Sparkles, Smartphone } from 'lucide-svelte';
	import { ui } from '$lib/stores/ui.svelte';
	import { downloadStore } from '$lib/stores/downloads.svelte';
	import { metadataScanStore } from '$lib/stores/metadataScan.svelte';

	const navItems = [
		{ href: '/', label: 'Home', icon: Home },
		{ href: '/playing', label: 'Playing', icon: AudioLines },
		{ href: '/manager', label: 'Manager', icon: LayoutDashboard },
		{ href: '/devices', label: 'Devices', icon: Smartphone },
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
	<div class="flex h-10 items-center {collapsed ? 'justify-center px-2' : 'justify-end px-3'}">
		<Button
			variant="ghost"
			size="icon-sm"
			class="text-muted-foreground hover:text-foreground shrink-0"
			onclick={() => ui.toggleSidebar()}
			aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
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
					class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors border-l-2
						{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground font-medium border-primary' : 'border-transparent text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
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
					class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors border-l-2
						{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground font-medium border-primary' : 'border-transparent text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
				>
					<item.icon class="size-4 shrink-0" />
					{item.label}
				</a>
			{/if}
		{/each}

		<div class="flex-1"></div>

		{#if downloadStore.activeCount > 0}
			{#if collapsed}
				<Tooltip.Root>
					<Tooltip.Trigger>
						<a
							href="/manager?tab=downloads"
							class="flex items-center justify-center rounded-lg p-2 transition-colors text-primary"
						>
							<div class="relative">
								<Download class="size-4 shrink-0 animate-pulse" />
								<span class="absolute -top-1.5 -right-1.5 flex size-3.5 items-center justify-center rounded-full bg-primary text-[9px] font-bold text-primary-foreground">
									{downloadStore.activeCount}
								</span>
							</div>
						</a>
					</Tooltip.Trigger>
					<Tooltip.Content side="right">
						{downloadStore.activeCount} downloading
					</Tooltip.Content>
				</Tooltip.Root>
			{:else}
				<a
					href="/manager?tab=downloads"
					class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors text-primary hover:bg-sidebar-accent/50"
				>
					<Download class="size-4 shrink-0 animate-pulse" />
					<span class="flex-1">{downloadStore.activeCount} downloading</span>
				</a>
			{/if}
		{/if}

		{#if metadataScanStore.active}
			{@const isScan = metadataScanStore.scanning}
			{@const scanProg = metadataScanStore.progress}
			{@const label = isScan
				? (scanProg ? `Enriching ${scanProg.current}/${scanProg.total}` : 'Enriching…')
				: `${metadataScanStore.autoPhase === 'albums' ? 'Albums' : 'Tracks'} ${metadataScanStore.autoCurrent}/${metadataScanStore.autoTotal}`}
			{@const pct = isScan
				? (scanProg && scanProg.total > 0 ? Math.round((scanProg.current / scanProg.total) * 100) : 0)
				: (metadataScanStore.autoTotal > 0 ? Math.round((metadataScanStore.autoCurrent / metadataScanStore.autoTotal) * 100) : 0)}
			{@const subtitle = isScan ? scanProg?.track_title : metadataScanStore.autoTitle}
			{#if collapsed}
				<Tooltip.Root>
					<Tooltip.Trigger>
						<a
							href="/metadata"
							class="flex items-center justify-center rounded-lg p-2 transition-colors text-warning"
						>
							<Sparkles class="size-4 shrink-0 animate-pulse" />
						</a>
					</Tooltip.Trigger>
					<Tooltip.Content side="right">
						{label}
					</Tooltip.Content>
				</Tooltip.Root>
			{:else}
				<a
					href="/metadata"
					class="flex flex-col gap-1 rounded-lg px-3 py-2 text-sm transition-colors text-warning hover:bg-sidebar-accent/50"
				>
					<div class="flex items-center gap-3">
						<Sparkles class="size-4 shrink-0 animate-pulse" />
						<span class="flex-1 truncate">{label}</span>
					</div>
					{#if pct > 0 || subtitle}
						<div class="ml-7 flex flex-col gap-0.5">
							<div class="h-1 w-full rounded-full bg-warning/20">
								<div
									class="h-1 rounded-full bg-warning transition-all"
									style="width: {pct}%"
								></div>
							</div>
							{#if subtitle}
								<span class="text-[10px] text-warning/70 truncate">{subtitle}</span>
							{/if}
						</div>
					{/if}
				</a>
			{/if}
		{/if}

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
					class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors border-l-2
						{isActive(item.href) ? 'bg-sidebar-accent text-sidebar-accent-foreground font-medium border-primary' : 'border-transparent text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground'}"
				>
					<item.icon class="size-4 shrink-0" />
					{item.label}
				</a>
			{/if}
		{/each}

		<div class="h-2"></div>
	</nav>
</aside>
