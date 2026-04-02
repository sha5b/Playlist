import type { Download } from '$lib/types';

export interface DownloadGroup {
	albumId: number | null;
	albumTitle: string | null;
	downloads: Download[];
}

/**
 * Groups downloads by target album ID.
 * Albums are grouped first, followed by ungrouped downloads.
 */
export function groupDownloadsByAlbum(
	downloads: Download[],
	albumNames: Record<number, string>
): DownloadGroup[] {
	const groups: DownloadGroup[] = [];
	const albumMap = new Map<number, Download[]>();
	const ungrouped: Download[] = [];

	for (const dl of downloads) {
		if (dl.target_album_id) {
			let list = albumMap.get(dl.target_album_id);
			if (!list) {
				list = [];
				albumMap.set(dl.target_album_id, list);
			}
			list.push(dl);
		} else {
			ungrouped.push(dl);
		}
	}

	for (const [albumId, dls] of albumMap) {
		const firstWithTitle = dls.find((d) => d.title);
		groups.push({
			albumId,
			albumTitle: albumNames[albumId] ?? (firstWithTitle?.artist ? `Album #${albumId}` : `Album #${albumId}`),
			downloads: dls,
		});
	}

	if (ungrouped.length > 0) {
		groups.push({ albumId: null, albumTitle: null, downloads: ungrouped });
	}

	return groups;
}
