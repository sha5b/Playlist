<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Slider } from '$lib/components/ui/slider';
	import { Separator } from '$lib/components/ui/separator';
</script>

<footer class="flex h-20 items-center border-t border-border bg-card px-4 gap-4">
	<!-- Left: Track info -->
	<div class="flex items-center gap-3 w-[280px] min-w-0">
		<div class="size-12 shrink-0 rounded-md bg-muted flex items-center justify-center">
			<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" class="size-6 text-muted-foreground">
				<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>
			</svg>
		</div>
		<div class="min-w-0">
			<p class="text-sm font-medium truncate text-foreground">No track playing</p>
			<p class="text-xs text-muted-foreground truncate">--</p>
		</div>
	</div>

	<!-- Center: Controls + Progress -->
	<div class="flex flex-1 flex-col items-center gap-1 max-w-[600px] mx-auto">
		<div class="flex items-center gap-2">
			<Button variant="ghost" size="icon-sm" class="text-muted-foreground hover:text-foreground" disabled>
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><path d="M2 7 L7 7 L7 2"/><path d="M16 3h2a2 2 0 0 1 2 2v2"/><path d="M2 17 L7 17 L7 22"/><path d="M16 21h2a2 2 0 0 0 2-2v-2"/></svg>
			</Button>
			<Button variant="ghost" size="icon-sm" class="text-muted-foreground hover:text-foreground" disabled>
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><polygon points="19 20 9 12 19 4 19 20"/><line x1="5" x2="5" y1="19" y2="5"/></svg>
			</Button>
			<Button variant="default" size="icon" class="rounded-full" disabled>
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" class="size-5"><polygon points="6 3 20 12 6 21 6 3"/></svg>
			</Button>
			<Button variant="ghost" size="icon-sm" class="text-muted-foreground hover:text-foreground" disabled>
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><polygon points="5 4 15 12 5 20 5 4"/><line x1="19" x2="19" y1="5" y2="19"/></svg>
			</Button>
			<Button variant="ghost" size="icon-sm" class="text-muted-foreground hover:text-foreground" disabled>
				<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><polyline points="17 1 21 5 17 9"/><path d="M3 11V9a4 4 0 0 1 4-4h14"/><polyline points="7 23 3 19 7 15"/><path d="M21 13v2a4 4 0 0 1-4 4H3"/></svg>
			</Button>
		</div>
		<div class="flex w-full items-center gap-2 px-2">
			<span class="text-[11px] text-muted-foreground w-10 text-right tabular-nums">0:00</span>
			<Slider value={[0]} max={100} step={1} class="flex-1" disabled />
			<span class="text-[11px] text-muted-foreground w-10 tabular-nums">0:00</span>
		</div>
	</div>

	<!-- Right: Volume + Queue -->
	<div class="flex items-center gap-2 w-[200px] justify-end">
		<Button variant="ghost" size="icon-sm" class="text-muted-foreground hover:text-foreground" disabled>
			<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M15.54 8.46a5 5 0 0 1 0 7.07"/><path d="M19.07 4.93a10 10 0 0 1 0 14.14"/></svg>
		</Button>
		<Slider value={[75]} max={100} step={1} class="w-24" disabled />
		<Button variant="ghost" size="icon-sm" class="text-muted-foreground hover:text-foreground" disabled>
			<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="size-4"><path d="M21 15V6"/><path d="M18.5 18a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z"/><path d="M12 12H3"/><path d="M16 6H3"/><path d="M12 18H3"/></svg>
		</Button>
	</div>
</footer>
