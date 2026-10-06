// Shared sidecar entry: load the pinned SDK lazily and serve the protocol. A missing SDK is
// reported at `initialize`, before any session or turn exists, so the host classifies it as a
// startup failure and never as a lost turn.

import { readFileSync } from "node:fs";
import { SidecarError } from "./ipc.mjs";
import { serve } from "./runtime.mjs";

/**
 * @param {object} spec
 * @param {string} spec.backend          backend name sent by the host
 * @param {string} spec.packageName      npm package the adapter wraps (also the pin key)
 * @param {URL}    spec.packageJson      the adapter directory's package.json
 * @param {(sdk: any, host: object, ctx: {env: object, sdkVersion: string}) => object} spec.makeAdapter
 */
export async function launch(spec) {
  const pinned = JSON.parse(readFileSync(spec.packageJson, "utf8")).dependencies?.[spec.packageName] ?? "unknown";
  // Tests substitute a fake SDK module; production resolves the pinned package.
  const moduleName = process.env.CODETWO_SIDECAR_SDK_MODULE || spec.packageName;
  let sdk;
  let loadError;
  try {
    sdk = await import(moduleName);
  } catch (error) {
    loadError = error;
  }
  serve({
    backend: spec.backend,
    createAdapter: (host) => {
      if (!sdk) {
        return {
          async initialize() {
            throw new SidecarError(
              `${spec.packageName} could not be loaded (${loadError?.message}). Run \`bun install\` in script/provider-sidecars/${spec.backend}.`,
              { phase: "pre_dispatch" },
            );
          },
          async shutdown() {},
        };
      }
      return spec.makeAdapter(sdk, host, { env: process.env, sdkVersion: pinned });
    },
  });
}
