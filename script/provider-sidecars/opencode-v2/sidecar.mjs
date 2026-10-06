#!/usr/bin/env node
import { launch } from "../common/entry.mjs";
import { createOpenCodeV2Adapter } from "./adapter.mjs";

await launch({
  backend: "opencode-v2",
  packageName: "@opencode/sdk",
  packageJson: new URL("./package.json", import.meta.url),
  makeAdapter: (sdk, host, ctx) => createOpenCodeV2Adapter({ sdk, host, ...ctx }),
});
