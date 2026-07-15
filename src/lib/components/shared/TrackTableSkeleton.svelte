<script lang="ts">
	import Skeleton from './Skeleton.svelte';

	let { rows = 8 }: { rows?: number } = $props();
</script>

<div class="rounded-xl border border-border/60 overflow-hidden">
	<table class="w-full">
		<thead>
			<tr class="border-b border-border bg-muted/30 text-xs text-muted-foreground uppercase tracking-wider">
				<th class="w-12 px-4 py-3"><Skeleton class="h-3 w-3 mx-auto" /></th>
				<th class="px-4 py-3 text-left"><Skeleton class="h-3 w-12" /></th>
				<th class="px-4 py-3 text-left"><Skeleton class="h-3 w-12" /></th>
				<th class="px-4 py-3 text-left hidden lg:table-cell"><Skeleton class="h-3 w-12" /></th>
				<th class="px-4 py-3 text-right w-20"><Skeleton class="h-3 w-3 ml-auto" /></th>
			</tr>
		</thead>
		<tbody>
			{#each Array(rows) as _, i}
				<tr class="border-b border-border/40">
					<td class="w-12 px-4 py-2.5 text-center">
						<Skeleton class="h-4 w-4 mx-auto" />
					</td>
					<td class="px-4 py-2.5">
						<div class="flex items-center gap-3">
							<Skeleton class="size-9 rounded shrink-0" />
							<Skeleton class="h-4 w-32" />
						</div>
					</td>
					<td class="px-4 py-2.5">
						<Skeleton class="h-4 w-24" />
					</td>
					<td class="px-4 py-2.5 hidden lg:table-cell">
						<Skeleton class="h-4 w-20" />
					</td>
					<td class="px-4 py-2.5 text-right">
						<Skeleton class="h-4 w-10 ml-auto" />
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>
