<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Plus, Trash2, Sparkles } from 'lucide-svelte';
	import { createSmartPlaylist, updateSmartPlaylist, previewSmartPlaylist } from '$lib/api/library';
	import type { Playlist, SmartRule, SmartRuleField, SmartRuleOp, SmartRules } from '$lib/types';
	import { toast } from 'svelte-sonner';

	let {
		open = $bindable(false),
		playlist = null,
		onsaved,
	}: {
		open?: boolean;
		/** When set, the dialog edits this smart playlist instead of creating one. */
		playlist?: Playlist | null;
		onsaved?: (playlist: Playlist) => void;
	} = $props();

	type FieldKind = 'text' | 'number' | 'date';

	const FIELDS: { value: SmartRuleField; label: string; kind: FieldKind }[] = [
		{ value: 'title', label: 'Title', kind: 'text' },
		{ value: 'artist', label: 'Artist', kind: 'text' },
		{ value: 'album', label: 'Album', kind: 'text' },
		{ value: 'genre', label: 'Genre', kind: 'text' },
		{ value: 'format', label: 'Format', kind: 'text' },
		{ value: 'year', label: 'Year', kind: 'number' },
		{ value: 'duration_ms', label: 'Duration (ms)', kind: 'number' },
		{ value: 'play_count', label: 'Play count', kind: 'number' },
		{ value: 'last_played_at', label: 'Last played', kind: 'date' },
		{ value: 'created_at', label: 'Date added', kind: 'date' },
	];

	const OP_LABELS: Record<SmartRuleOp, string> = {
		contains: 'contains',
		equals: 'is',
		not_equals: 'is not',
		gt: 'greater than',
		lt: 'less than',
		in_last_days: 'in the last N days',
		not_in_last_days: 'not in the last N days',
		is_null: 'is empty',
	};

	const OPS_BY_KIND: Record<FieldKind, SmartRuleOp[]> = {
		text: ['contains', 'equals', 'not_equals', 'is_null'],
		number: ['equals', 'not_equals', 'gt', 'lt', 'is_null'],
		date: ['in_last_days', 'not_in_last_days', 'is_null'],
	};

	interface RuleRow {
		field: SmartRuleField;
		op: SmartRuleOp;
		value: string;
	}

	let name = $state('');
	let description = $state('');
	let match = $state<'all' | 'any'>('all');
	let rows = $state<RuleRow[]>([{ field: 'genre', op: 'contains', value: '' }]);
	let sortField = $state<'' | SmartRuleField>('');
	let sortDir = $state<'asc' | 'desc'>('desc');
	let limit = $state('');
	let saving = $state(false);
	let previewCount = $state<number | null>(null);

	const isEdit = $derived(!!playlist);

	function fieldKind(field: SmartRuleField): FieldKind {
		return FIELDS.find((f) => f.value === field)?.kind ?? 'text';
	}

	function opsFor(field: SmartRuleField): SmartRuleOp[] {
		return OPS_BY_KIND[fieldKind(field)];
	}

	function onFieldChange(row: RuleRow) {
		// Keep the op valid for the newly selected field's kind.
		if (!opsFor(row.field).includes(row.op)) {
			row.op = opsFor(row.field)[0];
			row.value = '';
		}
	}

	function needsValue(op: SmartRuleOp): boolean {
		return op !== 'is_null';
	}

	function numericValue(row: RuleRow): boolean {
		return (
			fieldKind(row.field) === 'number' ||
			row.op === 'in_last_days' ||
			row.op === 'not_in_last_days'
		);
	}

	function addRow() {
		rows.push({ field: 'genre', op: 'contains', value: '' });
	}

	function removeRow(index: number) {
		rows.splice(index, 1);
	}

	/** Build the SmartRules payload; returns null when a required value is missing. */
	function buildRules(): SmartRules | null {
		const rules: SmartRule[] = [];
		for (const row of rows) {
			if (needsValue(row.op)) {
				const raw = row.value.trim();
				if (!raw) return null;
				if (numericValue(row)) {
					const n = Number(raw);
					if (!Number.isFinite(n)) return null;
					rules.push({ field: row.field, op: row.op, value: n });
				} else {
					rules.push({ field: row.field, op: row.op, value: raw });
				}
			} else {
				rules.push({ field: row.field, op: row.op });
			}
		}
		const limitNum = limit.trim() ? Number(limit.trim()) : null;
		if (limitNum !== null && (!Number.isFinite(limitNum) || limitNum < 1)) return null;
		return {
			match,
			rules,
			sort: sortField ? { field: sortField, dir: sortDir } : null,
			limit: limitNum,
		};
	}

	const rulesPayload = $derived.by(() => {
		// Touch every reactive input so the preview effect re-runs on any change.
		match;
		limit;
		sortField;
		sortDir;
		rows.forEach((r) => {
			r.field;
			r.op;
			r.value;
		});
		return buildRules();
	});

	// Live preview count (debounced).
	$effect(() => {
		const payload = rulesPayload;
		if (!open || !payload) {
			previewCount = null;
			return;
		}
		const timer = setTimeout(async () => {
			try {
				previewCount = await previewSmartPlaylist(payload);
			} catch {
				previewCount = null;
			}
		}, 350);
		return () => clearTimeout(timer);
	});

	// Reset the form each time the dialog opens (prefill in edit mode).
	$effect(() => {
		if (!open) return;
		if (playlist) {
			name = playlist.name;
			description = playlist.description ?? '';
			try {
				const parsed: SmartRules = JSON.parse(playlist.rules ?? '{}');
				match = parsed.match === 'any' ? 'any' : 'all';
				rows = (parsed.rules ?? []).map((r) => ({
					field: r.field,
					op: r.op,
					value: r.value != null ? String(r.value) : '',
				}));
				if (rows.length === 0) rows = [{ field: 'genre', op: 'contains', value: '' }];
				sortField = parsed.sort?.field ?? '';
				sortDir = parsed.sort?.dir === 'asc' ? 'asc' : 'desc';
				limit = parsed.limit != null ? String(parsed.limit) : '';
			} catch {
				rows = [{ field: 'genre', op: 'contains', value: '' }];
			}
		} else {
			name = '';
			description = '';
			match = 'all';
			rows = [{ field: 'genre', op: 'contains', value: '' }];
			sortField = '';
			sortDir = 'desc';
			limit = '';
		}
	});

	async function handleSave() {
		const payload = buildRules();
		if (!payload || !name.trim()) return;
		saving = true;
		try {
			let saved: Playlist;
			if (playlist) {
				saved = await updateSmartPlaylist(playlist.id, {
					name: name.trim(),
					description: description.trim() || undefined,
					rules: payload,
				});
				toast.success('Smart playlist updated');
			} else {
				saved = await createSmartPlaylist(name.trim(), payload, description.trim() || undefined);
				toast.success('Smart playlist created');
			}
			open = false;
			onsaved?.(saved);
		} catch (e) {
			toast.error(playlist ? 'Failed to update smart playlist' : 'Failed to create smart playlist');
			console.error('Smart playlist save failed:', e);
		} finally {
			saving = false;
		}
	}

	const selectClass =
		'border-input dark:bg-input/30 h-8 rounded-lg border bg-transparent px-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-3 disabled:opacity-50';
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-2xl">
		<Dialog.Header>
			<Dialog.Title class="flex items-center gap-2">
				<Sparkles class="size-4 text-primary" />
				{isEdit ? 'Edit Smart Playlist' : 'New Smart Playlist'}
			</Dialog.Title>
			<Dialog.Description>
				Tracks matching these rules are included automatically and stay up to date.
			</Dialog.Description>
		</Dialog.Header>

		<form onsubmit={(e) => { e.preventDefault(); handleSave(); }} class="space-y-4">
			<div class="space-y-2">
				<Input placeholder="Playlist name" bind:value={name} />
				<Input placeholder="Description (optional)" bind:value={description} />
			</div>

			<!-- Match mode -->
			<div class="flex items-center gap-2 text-sm">
				<span class="text-muted-foreground">Match</span>
				<select bind:value={match} class={selectClass}>
					<option value="all">all rules</option>
					<option value="any">any rule</option>
				</select>
			</div>

			<!-- Rule rows -->
			<div class="space-y-2">
				{#each rows as row, i}
					<div class="flex items-center gap-2">
						<select
							bind:value={row.field}
							onchange={() => onFieldChange(row)}
							class="{selectClass} w-36 shrink-0"
						>
							{#each FIELDS as f}
								<option value={f.value}>{f.label}</option>
							{/each}
						</select>
						<select bind:value={row.op} class="{selectClass} w-44 shrink-0">
							{#each opsFor(row.field) as op}
								<option value={op}>{OP_LABELS[op]}</option>
							{/each}
						</select>
						{#if needsValue(row.op)}
							<Input
								placeholder={numericValue(row) ? 'Number' : 'Value'}
								type={numericValue(row) ? 'number' : 'text'}
								bind:value={row.value}
								class="flex-1"
							/>
						{:else}
							<div class="flex-1"></div>
						{/if}
						<Button
							variant="ghost"
							size="icon"
							type="button"
							class="text-muted-foreground hover:text-destructive shrink-0"
							disabled={rows.length <= 1}
							onclick={() => removeRow(i)}
						>
							<Trash2 class="size-4" />
						</Button>
					</div>
				{/each}
				<Button variant="outline" size="sm" type="button" onclick={addRow} class="gap-1.5">
					<Plus class="size-3.5" />
					Add rule
				</Button>
			</div>

			<!-- Sort + limit -->
			<div class="flex items-center gap-2 text-sm flex-wrap">
				<span class="text-muted-foreground">Sort by</span>
				<select bind:value={sortField} class={selectClass}>
					<option value="">Date added (default)</option>
					{#each FIELDS as f}
						<option value={f.value}>{f.label}</option>
					{/each}
				</select>
				<select bind:value={sortDir} class={selectClass}>
					<option value="desc">Descending</option>
					<option value="asc">Ascending</option>
				</select>
				<span class="text-muted-foreground ml-2">Limit</span>
				<Input placeholder="No limit" type="number" min="1" bind:value={limit} class="w-28" />
			</div>

			<!-- Live preview -->
			<p class="text-xs text-muted-foreground">
				{#if rulesPayload === null}
					Fill in all rule values to preview.
				{:else if previewCount !== null}
					Currently matches {previewCount} track{previewCount !== 1 ? 's' : ''}.
				{:else}
					Counting matches…
				{/if}
			</p>

			<Dialog.Footer>
				<Dialog.Close>
					{#snippet child({ props })}
						<Button variant="outline" type="button" {...props}>Cancel</Button>
					{/snippet}
				</Dialog.Close>
				<Button type="submit" disabled={!name.trim() || rulesPayload === null || saving}>
					{saving ? 'Saving...' : isEdit ? 'Save rules' : 'Create'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
