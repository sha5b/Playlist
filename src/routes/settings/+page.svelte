<script lang="ts">
	import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Separator } from '$lib/components/ui/separator';
</script>

<div class="space-y-6">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">Settings</h1>
		<p class="text-muted-foreground mt-1">Configure your preferences</p>
	</div>

	<Card>
		<CardHeader>
			<CardTitle>General</CardTitle>
			<CardDescription>Application settings</CardDescription>
		</CardHeader>
		<CardContent>
			<p class="text-sm text-muted-foreground">Settings will be available in a future update.</p>
		</CardContent>
	</Card>
</div>
