<script lang="ts">
	import { Input } from '$lib/components/ui/input';
</script>

<div class="space-y-6">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Search</h1>
		<p class="text-muted-foreground mt-1">Find tracks, albums, and artists</p>
	</div>
	<Input placeholder="Search your library..." class="max-w-md" />
	<div class="flex items-center justify-center h-48 rounded-lg border border-dashed border-border">
		<p class="text-muted-foreground text-sm">Start typing to search your library</p>
	</div>
</div>
