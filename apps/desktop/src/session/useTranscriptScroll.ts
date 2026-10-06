import { useLayoutEffect, useRef, useState } from "react";
import type {
  KeyboardEventHandler,
  MutableRefObject,
  PointerEventHandler,
  UIEventHandler,
} from "react";

import { isTranscriptNearEnd, scrollTopAfterPrepend } from "./transcriptScroll";
import type { TranscriptScrollAnchor } from "./transcriptScroll";
import type { Turn } from "./turns";

export interface TranscriptScrollController {
  viewportRef: MutableRefObject<HTMLElement | null>;
  showJumpToLatest: boolean;
  onScroll: UIEventHandler<HTMLElement>;
  onPointerDownCapture: PointerEventHandler<HTMLElement>;
  onKeyDownCapture: KeyboardEventHandler<HTMLElement>;
  jumpToLatest: () => void;
  capturePrependAnchor: () => TranscriptScrollAnchor | null;
  prepareForPrepend: (anchor: TranscriptScrollAnchor | null) => void;
}

function prefersReducedMotion(): boolean {
  return (
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false
  );
}

/**
 * Keep a live transcript at the edge only while the reader is already following it.
 * User scrolling or interacting with the transcript pauses following; prepended history restores
 * the exact content position instead of treating the older page as new output.
 */
export function useTranscriptScroll(
  sessionId: string | null,
  turns: readonly Turn[],
  /** False while the pane is display:none; its offset is saved and restored on return. */
  active = true
): TranscriptScrollController {
  const viewportRef = useRef<HTMLElement | null>(null);
  const followingRef = useRef(true);
  const pendingPrependRef = useRef<TranscriptScrollAnchor | null>(null);
  const savedTopRef = useRef(0);
  const [showJumpToLatest, setShowJumpToLatest] = useState(false);

  const syncJumpVisibility = (element: HTMLElement | null) => {
    const visible = element ? !isTranscriptNearEnd(element) : false;
    setShowJumpToLatest((current) => (current === visible ? current : visible));
  };

  useLayoutEffect(() => {
    followingRef.current = true;
    pendingPrependRef.current = null;
    setShowJumpToLatest(false);
  }, [sessionId]);

  useLayoutEffect(() => {
    // A hidden pane loses its offset; put a reader who scrolled up back where they were.
    const element = viewportRef.current;
    if (!active || !element) return;
    if (followingRef.current) element.scrollTop = element.scrollHeight;
    else element.scrollTop = savedTopRef.current;
  }, [active]);

  useLayoutEffect(() => {
    if (!active) return;
    const anchor = pendingPrependRef.current;
    if (anchor) {
      pendingPrependRef.current = null;
      const element = anchor.element.isConnected
        ? anchor.element
        : viewportRef.current;
      if (element) {
        element.scrollTop = scrollTopAfterPrepend(anchor, element.scrollHeight);
        followingRef.current = isTranscriptNearEnd(element);
        syncJumpVisibility(element);
      }
      return;
    }

    const element = viewportRef.current;
    if (!element) return;
    if (followingRef.current) {
      // Streaming can update many times a second. Assigning scrollTop avoids stacking smooth-scroll
      // animations and batches the read/write inside the post-render layout phase.
      element.scrollTop = element.scrollHeight;
      setShowJumpToLatest(false);
    } else {
      syncJumpVisibility(element);
    }
  }, [syncJumpVisibility, turns, active]);

  const onScroll: UIEventHandler<HTMLElement> = (event) => {
    if (!active) return;
    savedTopRef.current = event.currentTarget.scrollTop;
    const following = isTranscriptNearEnd(event.currentTarget);
    followingRef.current = following;
    setShowJumpToLatest((current) =>
      current === !following ? current : !following
    );
  };

  const pauseFollowing = () => {
    followingRef.current = false;
  };

  const onKeyDownCapture: KeyboardEventHandler<HTMLElement> = (event) => {
    if (["ArrowUp", "Home", "PageUp"].includes(event.key)) {
      followingRef.current = false;
    }
  };

  const jumpToLatest = () => {
    followingRef.current = true;
    setShowJumpToLatest(false);
    viewportRef.current?.scrollTo({
      behavior: prefersReducedMotion() ? "auto" : "smooth",
      top: viewportRef.current.scrollHeight,
    });
  };

  const capturePrependAnchor = (): TranscriptScrollAnchor | null => {
    const element = viewportRef.current;
    if (!element) return null;
    return {
      element,
      scrollHeight: element.scrollHeight,
      scrollTop: element.scrollTop,
    };
  };

  const prepareForPrepend = (anchor: TranscriptScrollAnchor | null) => {
    pendingPrependRef.current = anchor;
  };

  return {
    viewportRef,
    showJumpToLatest,
    onScroll,
    onPointerDownCapture: pauseFollowing,
    onKeyDownCapture,
    jumpToLatest,
    capturePrependAnchor,
    prepareForPrepend,
  };
}
