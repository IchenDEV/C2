// @ts-nocheck
import { GlobalWindow } from "happy-dom";
import { act as reactAct } from "react";
import type { ReactElement } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";

/**
 * The repository's CanvasEditor tests install their own happy-dom window at
 * module evaluation time.  This harness deliberately does not touch globals
 * while it is imported, so those tests keep ownership of their Testing
 * Library document when the suites are run together.
 */
export const dom = new GlobalWindow({ url: "http://localhost/" });

if (typeof dom.Element.prototype.getAnimations !== "function") {
  Object.defineProperty(dom.Element.prototype, "getAnimations", {
    configurable: true,
    writable: true,
    value: () => [],
  });
}

export const canvas2dStub = {
  filter: "none",
  canvas: {},
  drawImage: () => {},
  getImageData: () => ({ data: [] }),
  putImageData: () => {},
  createImageData: () => ({ data: [] }),
  setTransform: () => {},
  resetTransform: () => {},
  save: () => {},
  restore: () => {},
  beginPath: () => {},
  closePath: () => {},
  moveTo: () => {},
  lineTo: () => {},
  stroke: () => {},
  fill: () => {},
  arc: () => {},
  rect: () => {},
  fillRect: () => {},
  strokeRect: () => {},
  clearRect: () => {},
  measureText: () => ({ width: 0 }),
  transform: () => {},
  scale: () => {},
  translate: () => {},
  rotate: () => {},
};

dom.HTMLCanvasElement.prototype.getContext = function () {
  return canvas2dStub as any;
};

const DOM_KEYS = [
  "window",
  "document",
  "DocumentFragment",
  "DOMRect",
  "Range",
  "Selection",
  "Node",
  "NodeFilter",
  "ShadowRoot",
  "Element",
  "SVGElement",
  "navigator",
  "localStorage",
  "HTMLElement",
  "HTMLButtonElement",
  "HTMLFormElement",
  "HTMLInputElement",
  "HTMLTextAreaElement",
  "HTMLImageElement",
  "HTMLCanvasElement",
  "MutationObserver",
  "KeyboardEvent",
  "Event",
  "CustomEvent",
  "FocusEvent",
  "MouseEvent",
  "PointerEvent",
] as const;

let previousGlobals: Record<string, unknown> | null = null;

function installDom(): void {
  for (const key of DOM_KEYS) {
    (globalThis as Record<string, unknown>)[key] = (
      dom as unknown as Record<string, unknown>
    )[key];
  }
  dom.HTMLCanvasElement.prototype.getContext = function () {
    return canvas2dStub as any;
  };
  if (dom.window?.HTMLCanvasElement?.prototype) {
    dom.window.HTMLCanvasElement.prototype.getContext = function () {
      return canvas2dStub as any;
    };
  }
  globalThis.getComputedStyle = dom.getComputedStyle.bind(dom);
  // Preserve frame pacing so floating-position effects cannot spin on zero-delay timers.
  globalThis.requestAnimationFrame = dom.requestAnimationFrame.bind(dom);
  globalThis.cancelAnimationFrame = dom.cancelAnimationFrame.bind(dom);
  globalThis.btoa = dom.btoa.bind(dom);
  globalThis.devicePixelRatio = 1;
}

export function activateDom(): void {
  previousGlobals ??= Object.fromEntries(
    [
      ...DOM_KEYS,
      "getComputedStyle",
      "requestAnimationFrame",
      "cancelAnimationFrame",
      "btoa",
      "devicePixelRatio",
    ].map((key) => [key, (globalThis as Record<string, unknown>)[key]])
  );
  installDom();
}

export function restoreDom(): void {
  // All Canvas component suites share this one window.  Restoring an
  // undefined/foreign document between files makes Radix portals and the
  // source CanvasEditor test bind to different DOM owners in one Bun run.
}

export interface Mounted {
  container: HTMLElement;
  rerender: (element: ReactElement) => void;
  unmount: () => void;
}

export function mount(element: ReactElement): Mounted {
  const container = dom.document.createElement("div") as unknown as HTMLElement;
  dom.document.body.append(container);
  let root: Root | null = null;
  reactAct(() => {
    root = createRoot(container);
    flushSync(() => root?.render(element));
  });
  return {
    container,
    rerender: (element: ReactElement) => {
      if (!root) return;
      reactAct(() => {
        flushSync(() => root?.render(element));
      });
    },
    unmount: () => {
      if (!root) return;
      flushSync(() => root?.unmount());
      container.remove();
      root = null;
    },
  };
}

export async function flush(): Promise<void> {
  await reactAct(async () => {
    /* empty */
  });
}

export function click(element: Element): void {
  element.dispatchEvent(
    new dom.window.MouseEvent("click", { bubbles: true, cancelable: true })
  );
}

export function button(container: ParentNode, name: string): HTMLButtonElement {
  const candidates = [...container.querySelectorAll("button")];
  const result = candidates.find((candidate) => {
    const label =
      candidate.getAttribute("aria-label") ?? candidate.textContent ?? "";
    return label.replaceAll(/\s+/gu, " ").trim() === name;
  });
  if (!result) throw new Error(`button not found: ${name}`);
  return result;
}

export function maybeButton(
  container: ParentNode,
  name: string
): HTMLButtonElement | null {
  try {
    return button(container, name);
  } catch {
    return null;
  }
}

export function image(container: ParentNode, alt: string): HTMLImageElement {
  const result = [...container.querySelectorAll("img")].find(
    (candidate) => candidate.getAttribute("alt") === alt
  );
  if (!result) throw new Error(`image not found: ${alt}`);
  return result;
}

export function text(container: ParentNode, value: string): Element {
  const result = [...container.querySelectorAll("*")].find((candidate) =>
    candidate.textContent?.includes(value)
  );
  if (!result) throw new Error(`text not found: ${value}`);
  return result;
}

export async function waitFor(
  predicate: () => void,
  timeoutMs = 500
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastError: unknown;
  while (Date.now() < deadline) {
    try {
      predicate();
      return;
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, 5));
      await flush();
    }
  }
  throw lastError ?? new Error("condition did not become true");
}
