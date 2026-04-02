/**
 * Reusable debounced search hook for library pages.
 * Uses Svelte 5 runes for reactive state management.
 */
export function useSearch(onSearch: () => Promise<void>, debounceMs = 250) {
	let query = $state('');
	let debounceTimer: ReturnType<typeof setTimeout> | undefined;

	function handleSearch(e: Event) {
		query = (e.target as HTMLInputElement).value;
		clearTimeout(debounceTimer);
		debounceTimer = setTimeout(() => onSearch(), debounceMs);
	}

	function clearSearch() {
		query = '';
		clearTimeout(debounceTimer);
		onSearch();
	}

	function setQuery(value: string) {
		query = value;
	}

	return {
		get query() {
			return query;
		},
		set query(value: string) {
			query = value;
		},
		handleSearch,
		clearSearch,
		setQuery,
	};
}
