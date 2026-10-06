#!/usr/bin/env node
import { launch } from "../common/entry.mjs";
import { createCursorAdapter } from "./adapter.mjs";

await launch({
  backend: "cursor-sdk",
  packageName: "@cursor/sdk",
  packageJson: new URL("./package.json", import.meta.url),
  makeAdapter: (sdk, host, ctx) => createCursorAdapter({ sdk, host, ...ctx }),
});
