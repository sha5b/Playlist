<script lang="ts">
	import { getStatsOverview, getStatsTop } from '$lib/api/stats';
	import type { StatsOverview, StatsPeriod, StatsTop, DayPlays } from '$lib/types';
	import { player } from '$lib/stores/player.svelte';
	import * as Tabs from '$lib/components/ui/tabs';
	import { Button } from '$lib/components/ui/button';
	import CoverArt from '$lib/components/shared/CoverArt.svelte';
	import { formatDurationLong } from '$lib/utils/format';
	import { Play, Music, Clock, Users, Disc, BarChart3 } from 'lucide-svelte';

	let overview = $state<StatsOverview | null>(null);
	let top = $state<StatsTop | null>(null);
	let period = $state<StatsPeriod>('week');
	let loading = $state(true);
	let hoveredDay = $state<DayPlays | null>(null);

	const periods: { value: StatsPeriod; label: string }[] = [
		{ value: 'week', label: 'Week' },
		{ value: 'month', label: 'Month' },
		{ value: 'year', label: 'Year' },
		{ value: 'all', label: 'All Time' },
	];

	async function loadOverview() {
		try {
			overview = await getStatsOverview();
		} catch (e) {
			console.error('Failed to load stats overview:', e);
		} finally {
			loading = false;
		}
	}

	async function loadTop(p: StatsPeriod) {
		try {
			top = await getStatsTop(p, 10);
		} catch (e) {
			console.error('Failed to load top stats:', e);
		}
	}

	$effect(() => {
		loadOverview();
	});

	$effect(() => {
		loadTop(period);
	});

	function formatDayLabel(day: string): string {
		// Keys are UTC calendar dates (Rust's date(played_at)) — parse and
		// format in UTC so labels never shift by a day near midnight.
		const d = new Date(day + 'T00:00:00Z');
		return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', timeZone: 'UTC' });
	}

	// Fill the last 90 days so gaps (days without plays) render as empty slots.
	const chartDays = $derived.by<DayPlays[]>(() => {
		if (!overview) return [];
		const byDay = new Map(overview.plays_per_day.map((d) => [d.day, d.count]));
		const days: DayPlays[] = [];
		const today = new Date();
		for (let i = 89; i >= 0; i--) {
			const d = new Date(today);
			d.setDate(today.getDate() - i);
			const key = d.toISOString().slice(0, 10);
			days.push({ day: key, count: byDay.get(key) ?? 0 });
		}
		return days;
	});

	const maxDayCount = $derived(Math.max(1, ...chartDays.map((d) => d.count)));
	const hasAnyPlays = $derived((overview?.total_plays ?? 0) > 0);
</script>

<div class="flex-1 min-h-0 overflow-y-auto overflow-x-hidden space-y-6 px-1">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Stats</h1>
		<p class="text-muted-foreground mt-1">Your listening history</p>
	</div>

	{#if loading}
		<p class="text-sm text-muted-foreground">Loading…</p>
	{:else if !hasAnyPlays}
		<div class="flex flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-border py-16 text-center">
			<BarChart3 class="size-8 text-muted-foreground/50" />
			<div>
				<p class="text-sm font-medium">No listening history yet</p>
				<p class="text-xs text-muted-foreground mt-1">Play some music — every counted play lands here.</p>
			</div>
		</div>
	{:else if overview}
		<!-- Overview tiles -->
		<div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
			<div class="rounded-lg border border-border bg-card p-4">
				<div class="flex items-center gap-2 text-muted-foreground">
					<Play class="size-3.5" />
					<span class="text-xs font-medium uppercase tracking-wider">Total Plays</span>
				</div>
				<p class="mt-2 text-2xl font-bold tabular-nums">{overview.total_plays.toLocaleString()}</p>
			</div>
			<div class="rounded-lg border border-border bg-card p-4">
				<div class="flex items-center gap-2 text-muted-foreground">
					<Clock class="size-3.5" />
					<span class="text-xs font-medium uppercase tracking-wider">Listening Time</span>
				</div>
				<p class="mt-2 text-2xl font-bold tabular-nums">{formatDurationLong(overview.total_listening_ms)}</p>
			</div>
			<div class="rounded-lg border border-border bg-card p-4">
				<div class="flex items-center gap-2 text-muted-foreground">
					<Users class="size-3.5" />
					<span class="text-xs font-medium uppercase tracking-wider">Artists</span>
				</div>
				<p class="mt-2 text-2xl font-bold tabular-nums">{overview.distinct_artists.toLocaleString()}</p>
			</div>
			<div class="rounded-lg border border-border bg-card p-4">
				<div class="flex items-center gap-2 text-muted-foreground">
					<Disc class="size-3.5" />
					<span class="text-xs font-medium uppercase tracking-wider">Albums</span>
				</div>
				<p class="mt-2 text-2xl font-bold tabular-nums">{overview.distinct_albums.toLocaleString()}</p>
			</div>
		</div>

		<!-- Plays per day (last 90 days) -->
		<section class="rounded-lg border border-border bg-card p-4">
			<div class="flex items-baseline justify-between">
				<h2 class="text-sm font-semibold">Plays per day</h2>
				<span class="text-xs text-muted-foreground tabular-nums">
					{#if hoveredDay}
						{formatDayLabel(hoveredDay.day)} — {hoveredDay.count} {hoveredDay.count === 1 ? 'play' : 'plays'}
					{:else}
						Last 90 days
					{/if}
				</span>
			</div>
			<div
				class="mt-3 flex h-32 items-end gap-px"
				role="img"
				aria-label="Bar chart of plays per day over the last 90 days"
				onmouseleave={() => (hoveredDay = null)}
			>
				{#each chartDays as d (d.day)}
					<div
						class="group relative flex h-full flex-1 items-end"
						role="presentation"
						onmouseenter={() => (hoveredDay = d)}
					>
						<div
							class="w-full rounded-t-[3px] transition-colors {d.count > 0
								? 'bg-primary/70 group-hover:bg-primary'
								: 'bg-muted/40 group-hover:bg-muted'}"
							style="height: {d.count > 0 ? Math.max(4, (d.count / maxDayCount) * 100) : 2}%"
						></div>
					</div>
				{/each}
			</div>
			<div class="mt-2 flex justify-between text-[10px] text-muted-foreground">
				<span>{chartDays.length ? formatDayLabel(chartDays[0].day) : ''}</span>
				<span>Today</span>
			</div>
		</section>

		<!-- Top lists -->
		<section class="space-y-4">
			<Tabs.Root value={period} onValueChange={(v) => (period = v as StatsPeriod)}>
				<div class="flex items-center justify-between">
					<h2 class="text-lg font-semibold">Most Played</h2>
					<Tabs.List>
						{#each periods as p}
							<Tabs.Trigger value={p.value}>{p.label}</Tabs.Trigger>
						{/each}
					</Tabs.List>
				</div>
			</Tabs.Root>

			{#if top}
				<div class="grid gap-6 lg:grid-cols-3">
					<!-- Top tracks -->
					<div class="space-y-2">
						<h3 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Tracks</h3>
						{#if top.tracks.length === 0}
							<p class="text-sm text-muted-foreground">No plays in this period.</p>
						{/if}
						{#each top.tracks as track, i (track.id)}
							<div class="group flex items-center gap-3 rounded-lg p-2 hover:bg-muted/50 transition-colors">
								<span class="w-5 text-right text-sm tabular-nums text-muted-foreground">{i + 1}</span>
								<div class="relative shrink-0">
									<CoverArt src={track.cover_art_path} alt={track.title} class="size-10 rounded object-cover" icon={Music} iconClass="size-4 text-muted-foreground/40" />
									<Button
										variant="default"
										size="icon-sm"
										class="absolute inset-0 m-auto size-7 opacity-0 group-hover:opacity-100 transition-opacity"
										aria-label="Play {track.title}"
										onclick={() => player.playTrack(track.id)}
									>
										<Play class="size-3.5" />
									</Button>
								</div>
								<div class="min-w-0 flex-1">
									<a href="/library/songs/{track.id}" class="block truncate text-sm font-medium hover:underline">{track.title}</a>
									{#if track.artist_name}
										{#if track.artist_id != null}
											<a href="/library/artists/{track.artist_id}" class="block truncate text-xs text-muted-foreground hover:underline">{track.artist_name}</a>
										{:else}
											<span class="block truncate text-xs text-muted-foreground">{track.artist_name}</span>
										{/if}
									{/if}
								</div>
								<span class="shrink-0 text-xs tabular-nums text-muted-foreground">{track.play_count} plays</span>
							</div>
						{/each}
					</div>

					<!-- Top artists -->
					<div class="space-y-2">
						<h3 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Artists</h3>
						{#if top.artists.length === 0}
							<p class="text-sm text-muted-foreground">No plays in this period.</p>
						{/if}
						{#each top.artists as artist, i (artist.id)}
							<a href="/library/artists/{artist.id}" class="flex items-center gap-3 rounded-lg p-2 hover:bg-muted/50 transition-colors">
								<span class="w-5 text-right text-sm tabular-nums text-muted-foreground">{i + 1}</span>
								<CoverArt src={artist.image_path} alt={artist.name} class="size-10 rounded-full object-cover" icon={Users} iconClass="size-4 text-muted-foreground/40" />
								<span class="min-w-0 flex-1 truncate text-sm font-medium">{artist.name}</span>
								<span class="shrink-0 text-xs tabular-nums text-muted-foreground">{artist.play_count} plays</span>
							</a>
						{/each}
					</div>

					<!-- Top albums -->
					<div class="space-y-2">
						<h3 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">Albums</h3>
						{#if top.albums.length === 0}
							<p class="text-sm text-muted-foreground">No plays in this period.</p>
						{/if}
						{#each top.albums as album, i (album.id)}
							<a href="/library/albums/{album.id}" class="flex items-center gap-3 rounded-lg p-2 hover:bg-muted/50 transition-colors">
								<span class="w-5 text-right text-sm tabular-nums text-muted-foreground">{i + 1}</span>
								<CoverArt src={album.cover_art_path} alt={album.title} class="size-10 rounded object-cover" icon={Disc} iconClass="size-4 text-muted-foreground/40" />
								<div class="min-w-0 flex-1">
									<span class="block truncate text-sm font-medium">{album.title}</span>
									{#if album.artist_name}
										<span class="block truncate text-xs text-muted-foreground">{album.artist_name}</span>
									{/if}
								</div>
								<span class="shrink-0 text-xs tabular-nums text-muted-foreground">{album.play_count} plays</span>
							</a>
						{/each}
					</div>
				</div>
			{/if}
		</section>
	{/if}
</div>
