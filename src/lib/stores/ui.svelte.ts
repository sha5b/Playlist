// UI state store — persists sidebar collapsed state to localStorage

let sidebarCollapsed: boolean = $state(false);

if (typeof window !== 'undefined') {
	const saved = localStorage.getItem('sidebar-collapsed');
	if (saved !== null) sidebarCollapsed = saved === 'true';
}

// In-memory scroll position map for preserving scroll across navigations
const scrollPositions = new Map<string, number>();

export const ui = {
	get sidebarCollapsed() {
		return sidebarCollapsed;
	},

	toggleSidebar() {
		sidebarCollapsed = !sidebarCollapsed;
		if (typeof window !== 'undefined') {
			localStorage.setItem('sidebar-collapsed', String(sidebarCollapsed));
		}
	},

	saveScroll(route: string, position: number) {
		scrollPositions.set(route, position);
	},

	getScroll(route: string): number {
		return scrollPositions.get(route) ?? 0;
	},
};
