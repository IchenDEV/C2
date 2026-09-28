// @ts-nocheck
import { afterEach, expect, test } from "bun:test";

import {
  activateDom,
  button,
  click,
  dom,
  flush,
  mount,
} from "./domTestHarness";

activateDom();
const { I18nProvider } = await import("../src/i18n");
const { FileDockContent } = await import("../src/files/FileDockContent");

afterEach(() => dom.document.body.replaceChildren());

function renderContent(
  openFiles = [],
  activeFile = null,
  onActiveFile = () => {}
) {
  return (
    <I18nProvider>
      <FileDockContent
        cwd={null}
        openFiles={openFiles}
        activeFile={activeFile}
        reveal={null}
        onActiveFile={onActiveFile}
        onCloseFile={() => {}}
        onInsertFile={() => {}}
        onOpenFile={() => {}}
        onSendText={() => {}}
      />
    </I18nProvider>
  );
}

test("empty file browser has one toolbar without a redundant navigation row", async () => {
  const view = mount(renderContent());
  await flush();
  expect(view.container.querySelector("[data-file-tabs]")).toBeNull();
  expect(view.container.querySelectorAll("[data-file-toolbar]")).toHaveLength(
    1
  );
  expect(
    view.container.querySelector('input[aria-label="Search files"]')
  ).not.toBeNull();
  expect(button(view.container, "New file").disabled).toBe(true);
  expect(button(view.container, "New folder").disabled).toBe(true);
  expect(button(view.container, "Rescan the workspace")).toBeTruthy();
  view.unmount();
});

test("open documents retain tab selection and a path back to browsing", async () => {
  const selected = [];
  const view = mount(
    renderContent(["README.md"], "README.md", (path) => selected.push(path))
  );
  await flush();
  expect(view.container.querySelector("[data-file-tabs]")).not.toBeNull();
  expect(view.container.querySelector("[data-file-toolbar]")).toBeNull();
  click(button(view.container, "Browse workspace files"));
  await flush();
  expect(view.container.querySelector("[data-file-toolbar]")).not.toBeNull();
  click(button(view.container, "README.md"));
  await flush();
  expect(selected).toEqual(["README.md"]);
  expect(view.container.querySelector("[data-file-toolbar]")).toBeNull();
  view.rerender(renderContent());
  await flush();
  expect(view.container.querySelector("[data-file-tabs]")).toBeNull();
  expect(view.container.querySelector("[data-file-toolbar]")).not.toBeNull();
  view.unmount();
});
