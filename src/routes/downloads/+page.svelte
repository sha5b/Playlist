<script lang="ts">
	import { Input } from '$lib/components/ui/input';
	import { Button } from '$lib/components/ui/button';
</script>

<div class="space-y-6">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Downloads</h1>
		<p class="text-muted-foreground mt-1">Download music from online platforms</p>
	</div>
	<div class="flex gap-2 max-w-xl">
		<Input placeholder="Paste a URL (YouTube, Spotify, SoundCloud...)" class="flex-1" />
		<Button disabled>Download</Button>
	</div>
	<div class="flex items-center justify-center h-48 rounded-lg border border-dashed border-border">
		<p class="text-muted-foreground text-sm">No downloads yet</p>
	</div>
</div>
