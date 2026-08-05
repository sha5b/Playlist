// UI state store — persists sidebar collapsed state to localStorage

let sidebarCollapsed: boolean = $state(false);

if (typeof window !== 'undefined') {
	const saved = localStorage.getItem('sidebar-collapsed');
	if (saved !== null) sidebarCollapsed = saved === 'true';
}

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
};
