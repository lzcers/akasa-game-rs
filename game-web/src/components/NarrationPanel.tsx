import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { Clock3, Sparkles } from "lucide-react";
import Typewriter from "./Typewriter";
import SelectedChoiceDisplay from "./SelectedChoiceDisplay";
import { STREAM_PLACEHOLDER_TEXT } from "../store/session/taskContent";
import type { NarrationRoundEntry } from "./gameplayTypes";

interface NarrationHistoryItemProps {
  entry: NarrationRoundEntry;
  isCurrentRound: boolean;
  animateCurrentRound: boolean;
  isFinished: boolean;
  activeBacktrackRound: number | null;
  onComplete?: () => void;
  onBacktrack?: (round: number) => void;
}

interface NarrationPanelProps {
  narrationHistory: NarrationRoundEntry[];
  currentRound: number;
  isAwaitingNarration: boolean;
  skipRestoredNarrationAnimation: boolean;
  broadcastMessages: string[];
  scrollToBottomKey?: string | null;
  onTypewriterComplete: () => void;
  activeBacktrackRound?: number | null;
  onBacktrackRound?: (round: number) => void;
}

const NarrationHistoryItem: React.FC<NarrationHistoryItemProps> = React.memo(
  ({
    entry,
    isCurrentRound,
    animateCurrentRound,
    isFinished,
    activeBacktrackRound,
    onComplete,
    onBacktrack,
  }) => {
    return (
      <div className="space-y-2" data-narration-round={entry.round}>
        <p className="text-sm font-medium text-[#d8c7aa]">
          第 {entry.round} 章：{entry.title || ""}
        </p>
        {entry.isAwaitingNarration && !entry.narrationText ? (
          <p className="inline-flex items-center gap-1.5 text-sm font-medium text-[#8f98ab]">
            <Clock3 className="h-3.5 w-3.5 animate-spin" />
            {STREAM_PLACEHOLDER_TEXT}
          </p>
        ) : (
          <Typewriter
            text={entry.narrationText}
            animate={animateCurrentRound}
            isFinished={isFinished}
            onComplete={isCurrentRound ? onComplete : undefined}
          />
        )}
        {entry.selectedChoiceText ? (
          <SelectedChoiceDisplay
            selectedChoiceText={entry.selectedChoiceText}
            selectedChoiceAction={entry.selectedChoiceAction}
            canBacktrack={
              entry.choices.length > 0 || entry.branchExplorations.length > 0
            }
            isBacktrackActive={activeBacktrackRound === entry.round}
            onBacktrack={onBacktrack ? () => onBacktrack(entry.round) : undefined}
          />
        ) : null}
      </div>
    );
  },
);

NarrationHistoryItem.displayName = "NarrationHistoryItem";

const NarrationPanel: React.FC<NarrationPanelProps> = ({
  narrationHistory,
  currentRound,
  isAwaitingNarration,
  skipRestoredNarrationAnimation,
  broadcastMessages,
  scrollToBottomKey = null,
  onTypewriterComplete,
  activeBacktrackRound = null,
  onBacktrackRound,
}) => {
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const narrationContentRef = useRef<HTMLDivElement | null>(null);
  const scrollTrackRef = useRef<HTMLDivElement | null>(null);
  const scrollbarDragRef = useRef<{
    pointerId: number;
    clientY: number;
    scrollTop: number;
  } | null>(null);
  const scrollAnchorRef = useRef<{
    round: number;
    offsetTop: number;
    scrollTop: number;
  } | null>(null);
  const [scrollThumb, setScrollThumb] = useState({
    top: 0,
    height: 32,
    visible: false,
  });
  const [isScrollbarDragging, setIsScrollbarDragging] = useState(false);
  const [broadcastCursor, setBroadcastCursor] = useState({ key: "", index: 0 });
  const [isBroadcastDragging, setIsBroadcastDragging] = useState(false);
  const broadcastSwipeStartRef = useRef<{
    pointerId: number;
    clientX: number;
  } | null>(null);
  const suppressBroadcastClickRef = useRef(false);
  const broadcastKey = broadcastMessages.join("||");
  const broadcastIndex =
    broadcastCursor.key === broadcastKey
      ? Math.min(
          broadcastCursor.index,
          Math.max(broadcastMessages.length - 1, 0),
        )
      : 0;
  const activeBroadcastMessage =
    broadcastMessages[broadcastIndex] ?? broadcastMessages[0] ?? "";
  const broadcastCountLabel =
    broadcastMessages.length > 0
      ? `${Math.min(broadcastIndex + 1, broadcastMessages.length)}/${broadcastMessages.length}`
      : "0/0";

  const updateNarrationScrollbar = useCallback(() => {
    const scrollElement = scrollContainerRef.current;
    const trackElement = scrollTrackRef.current;
    if (!scrollElement || !trackElement) {
      return;
    }

    const { clientHeight, scrollHeight, scrollTop } = scrollElement;
    const trackHeight = trackElement.clientHeight;
    const visible = scrollHeight > clientHeight + 1 && trackHeight > 0;
    if (!visible) {
      setScrollThumb((prev) =>
        prev.visible ? { ...prev, visible: false } : prev,
      );
      return;
    }

    const height = Math.max(
      32,
      Math.min(trackHeight, (clientHeight / scrollHeight) * trackHeight),
    );
    const maxThumbTop = Math.max(trackHeight - height, 0);
    const maxScrollTop = Math.max(scrollHeight - clientHeight, 1);
    const top = (scrollTop / maxScrollTop) * maxThumbTop;

    setScrollThumb((prev) => {
      if (
        prev.visible &&
        Math.abs(prev.top - top) < 0.5 &&
        Math.abs(prev.height - height) < 0.5
      ) {
        return prev;
      }

      return { top, height, visible: true };
    });
  }, []);

  useEffect(() => {
    const frameId = window.requestAnimationFrame(updateNarrationScrollbar);
    window.addEventListener("resize", updateNarrationScrollbar);

    return () => {
      window.cancelAnimationFrame(frameId);
      window.removeEventListener("resize", updateNarrationScrollbar);
    };
  }, [
    currentRound,
    isAwaitingNarration,
    narrationHistory,
    updateNarrationScrollbar,
  ]);

  useEffect(() => {
    const contentElement = narrationContentRef.current;
    if (!contentElement || typeof ResizeObserver === "undefined") {
      return undefined;
    }

    let frameId: number | null = null;
    const resizeObserver = new ResizeObserver(() => {
      if (frameId !== null) {
        window.cancelAnimationFrame(frameId);
      }

      frameId = window.requestAnimationFrame(() => {
        frameId = null;
        updateNarrationScrollbar();
      });
    });
    resizeObserver.observe(contentElement);

    return () => {
      resizeObserver.disconnect();
      if (frameId !== null) {
        window.cancelAnimationFrame(frameId);
      }
    };
  }, [updateNarrationScrollbar]);

  const captureNarrationScrollAnchor = useCallback((round: number) => {
    const scrollElement = scrollContainerRef.current;
    if (!scrollElement) {
      return;
    }

    const anchorElement = scrollElement.querySelector<HTMLElement>(
      `[data-narration-round="${round}"]`,
    );
    scrollAnchorRef.current = {
      round,
      offsetTop: anchorElement
        ? anchorElement.offsetTop - scrollElement.scrollTop
        : 0,
      scrollTop: scrollElement.scrollTop,
    };
  }, []);

  useLayoutEffect(() => {
    const scrollElement = scrollContainerRef.current;
    const anchor = scrollAnchorRef.current;
    if (!scrollElement || !anchor) {
      return;
    }

    const anchorElement = scrollElement.querySelector<HTMLElement>(
      `[data-narration-round="${anchor.round}"]`,
    );
    if (anchorElement) {
      scrollElement.scrollTop = Math.max(
        0,
        anchorElement.offsetTop - anchor.offsetTop,
      );
    } else {
      scrollElement.scrollTop = anchor.scrollTop;
    }

    updateNarrationScrollbar();
    if (activeBacktrackRound === null) {
      scrollAnchorRef.current = null;
    }
  }, [
    activeBacktrackRound,
    currentRound,
    narrationHistory,
    updateNarrationScrollbar,
  ]);

  useLayoutEffect(() => {
    if (!scrollToBottomKey) {
      return;
    }

    const scrollElement = scrollContainerRef.current;
    if (!scrollElement) {
      return;
    }

    scrollElement.scrollTop = scrollElement.scrollHeight;
    updateNarrationScrollbar();
  }, [scrollToBottomKey, updateNarrationScrollbar]);

  const scrollByThumbDelta = useCallback(
    (deltaY: number) => {
      const scrollElement = scrollContainerRef.current;
      const trackElement = scrollTrackRef.current;
      const dragStart = scrollbarDragRef.current;
      if (!scrollElement || !trackElement || !dragStart) {
        return;
      }

      const maxScrollTop = Math.max(
        scrollElement.scrollHeight - scrollElement.clientHeight,
        0,
      );
      const maxThumbTop = Math.max(
        trackElement.clientHeight - scrollThumb.height,
        1,
      );
      scrollElement.scrollTop =
        dragStart.scrollTop + (deltaY / maxThumbTop) * maxScrollTop;
      updateNarrationScrollbar();
    },
    [scrollThumb.height, updateNarrationScrollbar],
  );

  const handleScrollbarThumbPointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (event.pointerType === "mouse" && event.button !== 0) {
        return;
      }

      const scrollElement = scrollContainerRef.current;
      if (!scrollElement) {
        return;
      }

      event.preventDefault();
      event.stopPropagation();
      event.currentTarget.setPointerCapture(event.pointerId);
      scrollbarDragRef.current = {
        pointerId: event.pointerId,
        clientY: event.clientY,
        scrollTop: scrollElement.scrollTop,
      };
      setIsScrollbarDragging(true);
    },
    [],
  );

  useEffect(() => {
    const handlePointerMove = (event: PointerEvent) => {
      const dragStart = scrollbarDragRef.current;
      if (!dragStart || dragStart.pointerId !== event.pointerId) {
        return;
      }

      event.preventDefault();
      event.stopPropagation();
      scrollByThumbDelta(event.clientY - dragStart.clientY);
    };

    const releasePointer = (event: PointerEvent) => {
      const dragStart = scrollbarDragRef.current;
      if (!dragStart || dragStart.pointerId !== event.pointerId) {
        return;
      }

      scrollbarDragRef.current = null;
      setIsScrollbarDragging(false);
    };

    window.addEventListener("pointermove", handlePointerMove, {
      passive: false,
    });
    window.addEventListener("pointerup", releasePointer);
    window.addEventListener("pointercancel", releasePointer);

    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", releasePointer);
      window.removeEventListener("pointercancel", releasePointer);
    };
  }, [scrollByThumbDelta]);

  const handleScrollbarTrackPointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (event.target !== event.currentTarget) {
        return;
      }

      const scrollElement = scrollContainerRef.current;
      const trackElement = scrollTrackRef.current;
      if (!scrollElement || !trackElement) {
        return;
      }

      event.preventDefault();
      event.stopPropagation();
      const trackRect = trackElement.getBoundingClientRect();
      const nextThumbTop =
        event.clientY - trackRect.top - scrollThumb.height / 2;
      const maxThumbTop = Math.max(
        trackElement.clientHeight - scrollThumb.height,
        1,
      );
      const maxScrollTop = Math.max(
        scrollElement.scrollHeight - scrollElement.clientHeight,
        0,
      );
      scrollElement.scrollTop =
        (Math.min(Math.max(nextThumbTop, 0), maxThumbTop) / maxThumbTop) *
        maxScrollTop;
      updateNarrationScrollbar();
    },
    [scrollThumb.height, updateNarrationScrollbar],
  );

  const handleScrollbarLostPointerCapture = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      const dragStart = scrollbarDragRef.current;
      if (!dragStart || dragStart.pointerId !== event.pointerId) {
        return;
      }

      scrollbarDragRef.current = null;
      setIsScrollbarDragging(false);
    },
    [],
  );

  const handleBacktrackRound = useCallback(
    (round: number) => {
      captureNarrationScrollAnchor(round);
      onBacktrackRound?.(round);
    },
    [captureNarrationScrollAnchor, onBacktrackRound],
  );

  const moveBroadcastIndex = useCallback(
    (direction: "prev" | "next") => {
      if (broadcastMessages.length <= 1) {
        return;
      }

      setBroadcastCursor((prev) => {
        const currentIndex = prev.key === broadcastKey ? prev.index : 0;
        const nextIndex =
          direction === "next"
            ? (currentIndex + 1) % broadcastMessages.length
            : (currentIndex - 1 + broadcastMessages.length) %
              broadcastMessages.length;
        return {
          key: broadcastKey,
          index: nextIndex,
        };
      });
    },
    [broadcastKey, broadcastMessages.length],
  );

  const releaseBroadcastPointer = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      broadcastSwipeStartRef.current = null;
      setIsBroadcastDragging(false);

      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
    },
    [],
  );

  const handleBroadcastPointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (event.pointerType === "mouse" && event.button !== 0) {
        return;
      }

      broadcastSwipeStartRef.current = {
        pointerId: event.pointerId,
        clientX: event.clientX,
      };
      setIsBroadcastDragging(true);
      event.currentTarget.setPointerCapture(event.pointerId);
    },
    [],
  );

  const handleBroadcastPointerEnd = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      const start = broadcastSwipeStartRef.current;
      if (!start || start.pointerId !== event.pointerId) {
        return;
      }

      const deltaX = event.clientX - start.clientX;
      releaseBroadcastPointer(event);

      if (Math.abs(deltaX) < 36) {
        return;
      }

      suppressBroadcastClickRef.current = true;
      moveBroadcastIndex(deltaX < 0 ? "next" : "prev");
    },
    [moveBroadcastIndex, releaseBroadcastPointer],
  );

  const handleBroadcastPointerCancel = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      const start = broadcastSwipeStartRef.current;
      if (!start || start.pointerId !== event.pointerId) {
        return;
      }

      releaseBroadcastPointer(event);
    },
    [releaseBroadcastPointer],
  );

  const handleBroadcastClick = useCallback(() => {
    if (suppressBroadcastClickRef.current) {
      suppressBroadcastClickRef.current = false;
      return;
    }

    moveBroadcastIndex("next");
  }, [moveBroadcastIndex]);

  const handleBroadcastKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (event.key === "ArrowLeft") {
        event.preventDefault();
        moveBroadcastIndex("prev");
        return;
      }

      if (
        event.key === "ArrowRight" ||
        event.key === "Enter" ||
        event.key === " "
      ) {
        event.preventDefault();
        moveBroadcastIndex("next");
      }
    },
    [moveBroadcastIndex],
  );

  return (
    <>
      {activeBroadcastMessage ? (
        <div
          className={`akashic-pill flex min-h-10 w-full max-w-full mb-2 shrink-0 items-start border-amber-300/50 bg-[#1d1820]/95 px-2.5 py-1 text-[0.72rem] text-amber-100 select-none touch-pan-y sm:min-h-11 sm:text-xs ${isBroadcastDragging ? "cursor-grabbing" : "cursor-grab"}`}
          role={broadcastMessages.length > 1 ? "button" : undefined}
          tabIndex={broadcastMessages.length > 1 ? 0 : undefined}
          aria-label={broadcastMessages.length > 1 ? "切换天命回响" : undefined}
          onPointerDown={handleBroadcastPointerDown}
          onPointerUp={handleBroadcastPointerEnd}
          onPointerCancel={handleBroadcastPointerCancel}
          onLostPointerCapture={releaseBroadcastPointer}
          onClick={handleBroadcastClick}
          onKeyDown={handleBroadcastKeyDown}
        >
          <Sparkles className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-200" />
          <span className="line-clamp-2 min-w-0 flex-1 leading-4">
            {activeBroadcastMessage}
          </span>
          <span className="shrink-0 rounded-full border border-amber-300/25 bg-black/15 px-1.5 py-0.5 text-[0.65rem] leading-none text-amber-100/80 sm:text-[0.7rem]">
            {broadcastCountLabel}
          </span>
        </div>
      ) : null}
      <section className="akashic-panel flex min-h-0 flex-1 flex-col p-2">
        <div className="relative flex min-h-0 flex-1 flex-col rounded-2xl bg-[#040912]/90 sm:rounded-[1.2rem] sm:pl-4 md:rounded-[1.3rem] md:pl-5">
          <div
            id="narration-scroll"
            ref={scrollContainerRef}
            className="scrollbar-none min-h-0 flex-1 touch-pan-y overscroll-contain overflow-y-auto"
            onScroll={updateNarrationScrollbar}
          >
            <div
              ref={narrationContentRef}
              className="h-full space-y-5 text-[1rem] font-semibold leading-[1.82] text-[#f6eddc] sm:text-[1rem] md:text-[1.2rem]"
            >
              {narrationHistory.map((entry) => {
                return (
                  <NarrationHistoryItem
                    key={entry.round}
                    entry={entry}
                    isCurrentRound={entry.round === currentRound}
                    animateCurrentRound={
                      entry.round === currentRound &&
                      !skipRestoredNarrationAnimation
                    }
                    isFinished={
                      entry.round !== currentRound ||
                      entry.narrationStatus === "done"
                    }
                    activeBacktrackRound={activeBacktrackRound}
                    onComplete={onTypewriterComplete}
                    onBacktrack={handleBacktrackRound}
                  />
                );
              })}
              {!narrationHistory.length && isAwaitingNarration ? (
                <p className="inline-flex items-center gap-1.5 text-sm font-medium text-[#8f98ab]">
                  <Clock3 className="h-3.5 w-3.5 animate-spin" />
                  {STREAM_PLACEHOLDER_TEXT}
                </p>
              ) : null}
            </div>
          </div>
          <div
            ref={scrollTrackRef}
            className={`absolute bottom-2 -right-4 top-2 w-5 touch-none select-none transition-opacity ${scrollThumb.visible ? "opacity-100" : "pointer-events-none opacity-0"}`}
            onPointerDown={handleScrollbarTrackPointerDown}
            aria-hidden={!scrollThumb.visible}
          >
            <div
              className={`absolute left-1/2 w-[3px] -translate-x-1/2 touch-none select-none rounded-full bg-[#d8c18f]/55 transition-colors hover:bg-[#e4d1a9]/80 ${isScrollbarDragging ? "cursor-grabbing bg-[#f0dfc2]/90" : "cursor-grab"}`}
              style={{
                height: `${scrollThumb.height}px`,
                transform: `translate(-50%, ${scrollThumb.top}px)`,
              }}
              role="scrollbar"
              aria-controls="narration-scroll"
              aria-orientation="vertical"
              aria-valuemin={0}
              aria-valuemax={100}
              onPointerDown={handleScrollbarThumbPointerDown}
              onLostPointerCapture={handleScrollbarLostPointerCapture}
            />
          </div>
        </div>
      </section>
    </>
  );
};

export default NarrationPanel;
