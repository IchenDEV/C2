import type { ProviderInfo } from "../bridge";

const DEFAULT_RETRY_DELAYS_MS = [0, 250, 750] as const;
// Host discovery bounds version probes at 6s and model queries at 8s. Let it settle
// before retrying, otherwise a slow CLI spawns overlapping discovery requests.
const DEFAULT_ATTEMPT_TIMEOUT_MS = 16_000;

async function timeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  return await new Promise<T>((resolve, reject) => {
    const timer = globalThis.setTimeout(
      () =>
        reject(new Error(`Provider detection timed out after ${timeoutMs}ms`)),
      timeoutMs
    );
    promise.then(
      (value) => {
        globalThis.clearTimeout(timer);
        resolve(value);
      },
      (error: unknown) => {
        globalThis.clearTimeout(timer);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    );
  });
}

async function pause(delayMs: number): Promise<void> {
  return delayMs > 0
    ? await new Promise((resolve) => globalThis.setTimeout(resolve, delayMs))
    : await Promise.resolve();
}

/**
 * Desktop RPC can race the native bridge during first paint. Bound every attempt and retry the
 * host provider discovery so one lost startup request cannot leave the picker empty forever.
 */
export async function loadProviderRegistry(
  load: () => Promise<ProviderInfo[]>,
  retryDelaysMs: readonly number[] = DEFAULT_RETRY_DELAYS_MS,
  attemptTimeoutMs = DEFAULT_ATTEMPT_TIMEOUT_MS
): Promise<ProviderInfo[]> {
  let lastError: unknown = new Error("Provider detection did not run");

  for (const delayMs of retryDelaysMs) {
    await pause(delayMs);
    try {
      const providers = await timeout(load(), attemptTimeoutMs);
      if (providers.length === 0)
        throw new Error("Provider detection returned an empty registry");
      return providers;
    } catch (error) {
      lastError = error;
    }
  }

  throw lastError instanceof Error ? lastError : new Error(String(lastError));
}
