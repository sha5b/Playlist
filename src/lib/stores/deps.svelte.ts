import { listen } from '@tauri-apps/api/event';
import { checkDeps, ensureDeps } from '$lib/api/downloads';
import type { DepsStatus, SetupProgress } from '$lib/types';

let status = $state<DepsStatus | null>(null);
let checking = $state(true);
let setupInProgress = $state(false);
let setupMessage = $state('Checking dependencies...');
let setupProgress = $state(0);
let setupError = $state<string | null>(null);
let initialized = false;

async function init(force = false) {
	if (initialized && !force) return;
	initialized = true;

	checking = true;
	setupError = null;

	try {
		status = await checkDeps();

		if (!status.ytdlp_available || !status.ffmpeg_available) {
			setupInProgress = true;
			setupMessage = 'Setting up download tools...';
			setupProgress = 0;

			const unlisten = await listen<SetupProgress>('setup-progress', (event) => {
				setupMessage = event.payload.message;
				setupProgress = event.payload.progress;
			});

			try {
				await ensureDeps();
				status = await checkDeps();
			} catch (e) {
				setupError = String(e);
			} finally {
				unlisten();
				setupInProgress = false;
			}
		}
	} catch (e) {
		setupError = String(e);
	} finally {
		checking = false;
	}
}

export const depsStore = {
	get status() {
		return status;
	},
	get ready() {
		return status?.ytdlp_available && status?.ffmpeg_available;
	},
	get checking() {
		return checking;
	},
	get setupInProgress() {
		return setupInProgress;
	},
	get setupMessage() {
		return setupMessage;
	},
	get setupProgress() {
		return setupProgress;
	},
	get setupError() {
		return setupError;
	},
	init,
};
