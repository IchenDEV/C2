import { useSyncExternalStore } from "react";

import { environmentRegistry } from "../coreTransport";
import type { EnvironmentsSnapshot } from "../remoteEnvironments";

/** Remote C2 environments paired with this desktop, re-rendering on any change. */
export function useEnvironments(): EnvironmentsSnapshot {
  return useSyncExternalStore(
    environmentRegistry.subscribe,
    environmentRegistry.snapshot
  );
}
