<script lang="ts">
	import * as Dialog from '$lib/components/ui/dialog';

	let { open = $bindable(false) }: { open?: boolean } = $props();

	interface Shortcut {
		keys: string[];
		description: string;
	}

	const groups: { title: string; shortcuts: Shortcut[] }[] = [
		{
			title: 'Playback',
			shortcuts: [
				{ keys: ['Space'], description: 'Play / pause' },
				{ keys: ['←', '→'], description: 'Seek back / forward 10 s' },
				{ keys: ['Ctrl', '←'], description: 'Previous track' },
				{ keys: ['Ctrl', '→'], description: 'Next track' },
				{ keys: ['Ctrl', '↑'], description: 'Volume up 5%' },
				{ keys: ['Ctrl', '↓'], description: 'Volume down 5%' },
				{ keys: ['M'], description: 'Mute / unmute' },
			],
		},
		{
			title: 'App',
			shortcuts: [
				{ keys: ['Ctrl', 'K'], description: 'Search library' },
				{ keys: ['Ctrl', 'Shift', 'D'], description: 'Toggle debug console' },
				{ keys: ['?'], description: 'Show this cheat sheet' },
				{ keys: ['Esc'], description: 'Close dialogs' },
			],
		},
	];
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Keyboard shortcuts</Dialog.Title>
			<Dialog.Description>
				Playback shortcuts are ignored while typing in a text field.
			</Dialog.Description>
		</Dialog.Header>

		<div class="flex flex-col gap-4">
			{#each groups as group (group.title)}
				<div>
					<h3 class="mb-1.5 text-[10px] font-semibold tracking-wider text-muted-foreground uppercase">
						{group.title}
					</h3>
					<div class="flex flex-col divide-y divide-border/50 rounded-lg border border-border">
						{#each group.shortcuts as shortcut (shortcut.description)}
							<div class="flex items-center justify-between gap-4 px-3 py-1.5">
								<span class="text-sm text-foreground">{shortcut.description}</span>
								<span class="flex items-center gap-1">
									{#each shortcut.keys as key, i (i)}
										<kbd class="rounded border border-border bg-muted px-1.5 py-0.5 text-[11px] font-medium text-muted-foreground">
											{key}
										</kbd>
									{/each}
								</span>
							</div>
						{/each}
					</div>
				</div>
			{/each}
		</div>
	</Dialog.Content>
</Dialog.Root>
