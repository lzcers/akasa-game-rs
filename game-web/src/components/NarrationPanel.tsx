import React, { useCallback, useEffect, useRef, useState } from "react";
import { Clock3, MousePointer2, Sparkles } from "lucide-react";
import Typewriter from "./Typewriter";
import { STREAM_PLACEHOLDER_TEXT } from "../store/session/taskContent";
import type { NarrationRoundEntry } from "./gameplayTypes";

interface NarrationHistoryItemProps {
  entry: NarrationRoundEntry;
  isCurrentRound: boolean;
  animateCurrentRound: boolean;
  isFinished: boolean;
  onComplete?: () => void;
}

interface NarrationPanelProps {
  narrationHistory: NarrationRoundEntry[];
  currentRound: number;
  isAwaitingNarration: boolean;
  skipRestoredNarrationAnimation: boolean;
  broadcastMessages: string[];
  onTypewriterComplete: () => void;
}

const NarrationHistoryItem: React.FC<NarrationHistoryItemProps> = React.memo(
  ({ entry, isCurrentRound, animateCurrentRound, isFinished, onComplete }) => {
    const selectedChoiceAction = entry.selectedChoiceAction?.trim();
    const shouldShowSelectedChoiceAction = Boolean(
      selectedChoiceAction &&
      selectedChoiceAction !== entry.selectedChoiceText?.trim(),
    );

    return (
      <div className="space-y-2">
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
          <div className="inline-flex max-w-full items-start gap-1.5 rounded-[0.85rem] border border-amber-300/25 bg-amber-100/8 px-2.5 py-1.5 text-[0.82rem] font-medium leading-5 text-amber-100/90 sm:text-[0.92rem]">
            <div className="min-w-0 space-y-1">
              <span className="flex">
                <MousePointer2 className="mr-1 mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-200/90" />
                {entry.selectedChoiceText}
              </span>
              {shouldShowSelectedChoiceAction ? (
                <span className="block text-[0.76rem] leading-5 text-amber-100/72 sm:text-[0.84rem]">
                  {selectedChoiceAction}
                </span>
              ) : null}
            </div>
          </div>
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
  onTypewriterComplete,
}) => {
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const scrollTrackRef = useRef<HTMLDivElement | null>(null);
  const scrollbarDragRef = useRef<{
    pointerId: number;
    clientY: number;
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
    if (!skipRestoredNarrationAnimation || !scrollContainerRef.current) {
      return;
    }

    const frameId = window.requestAnimationFrame(() => {
      if (!scrollContainerRef.current) {
        return;
      }
      scrollContainerRef.current.scrollTop =
        scrollContainerRef.current.scrollHeight;
    });

    return () => window.cancelAnimationFrame(frameId);
  }, [currentRound, narrationHistory, skipRestoredNarrationAnimation]);

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
            <div className="h-full space-y-5 text-[1rem] font-semibold leading-[1.82] text-[#f6eddc] sm:text-[1rem] md:text-[1.2rem]">
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
                    onComplete={onTypewriterComplete}
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
            />
          </div>
        </div>
      </section>
    </>
  );
};

export default NarrationPanel;
