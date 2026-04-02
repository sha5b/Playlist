import { toast } from 'svelte-sonner';

/**
 * Log an error to the console with context.
 * Use for silent error handling where we don't want to disturb the user.
 */
export function logError(context: string, error: unknown): void {
	console.error(`[${context}]`, error);
}

/**
 * Show an error toast to the user.
 * Use when an explicit user action has failed.
 */
export function showErrorToast(message: string, error?: unknown): void {
	toast.error(message, {
		description: error ? String(error) : undefined,
	});
}

/**
 * Wrapper for async operations with error handling.
 * - silent: logs error but doesn't show toast (use for background operations)
 * - fallback: returns this value on error instead of undefined
 */
export async function withErrorHandling<T>(
	operation: () => Promise<T>,
	options: {
		context: string;
		silent?: boolean;
		fallback?: T;
		onError?: (error: unknown) => void;
	}
): Promise<T | undefined> {
	try {
		return await operation();
	} catch (error) {
		logError(options.context, error);

		if (options.onError) {
			options.onError(error);
		}

		if (!options.silent) {
			showErrorToast(`Failed: ${options.context}`, error);
		}

		return options.fallback;
	}
}
