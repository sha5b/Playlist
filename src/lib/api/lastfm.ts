import { invoke } from '@tauri-apps/api/core';
import type { LastfmAuth, LastfmStatus } from '$lib/types';

/** Request an auth token and open the Last.fm authorization page in the browser. */
export async function startLastfmAuth(): Promise<LastfmAuth> {
	return invoke('lastfm_start_auth');
}

/** After the user authorized in the browser, exchange the token for a session. */
export async function finishLastfmAuth(token: string): Promise<LastfmStatus> {
	return invoke('lastfm_finish_auth', { token });
}

export async function getLastfmStatus(): Promise<LastfmStatus> {
	return invoke('lastfm_get_status');
}

export async function disconnectLastfm(): Promise<LastfmStatus> {
	return invoke('lastfm_disconnect');
}

export async function setLastfmScrobbling(enabled: boolean): Promise<LastfmStatus> {
	return invoke('lastfm_set_scrobbling', { enabled });
}
