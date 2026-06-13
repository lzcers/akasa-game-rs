import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useShallow } from "zustand/react/shallow";
import {
  ArrowLeft,
  GitBranch,
  LocateFixed,
  MousePointer2,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { useNavigate } from "react-router-dom";
import { ScreenShell, StoryFrame } from "../components/AkashicUI";
import { appRoutes, routeWithSession } from "../lib/appRoutes";
import { getGameSessionStoryline, type StorylineData } from "../lib/api";
import { cn } from "../lib/utils";
import { useGameInternalStore } from "../store/gameStore";
import { useGameUIStore } from "../store/gameUIStore";
import {
  buildStorylineGraph,
  edgePath,
  type StorylineNode,
} from "./storylineGraph";

interface PointerPoint {
  clientX: number;
  clientY: number;
}

const MIN_ZOOM = 0.2;
const MAX_ZOOM = 1.8;
const ZOOM_STEP = 0.2;

interface StorylinePageProps {
  isOverlay?: boolean;
  onClose?: () => void;
}

function clampZoom(value: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Number(value.toFixed(2))));
}

function pointerDistance(left: PointerPoint, right: PointerPoint): number {
  return Math.hypot(left.clientX - right.clientX, left.clientY - right.clientY);
}

function pointerCenter(left: PointerPoint, right: PointerPoint): PointerPoint {
  return {
    clientX: (left.clientX + right.clientX) / 2,
    clientY: (left.clientY + right.clientY) / 2,
  };
}

const StorylinePage: React.FC<StorylinePageProps> = ({
  isOverlay = false,
  onClose,
}) => {
  const navigate = useNavigate();
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    originX: number;
    originY: number;
  } | null>(null);
  const activePointersRef = useRef(new Map<number, PointerPoint>());
  const previousPinchDistanceRef = useRef<number | null>(null);
  const { sessionId, displayRound, roundStates } = useGameInternalStore(
    useShallow((state) => ({
      sessionId: state.sessionId,
      displayRound: state.displayRound,
      roundStates: state.roundStates,
    })),
  );
  const { activeTurnId, isGameLoading, selectStorylineNode } = useGameUIStore(
    useShallow((state) => ({
      activeTurnId: state.stateView?.activeTurnId,
      isGameLoading: state.isLoading,
      selectStorylineNode: state.selectStorylineNode,
    })),
  );
  const [storyline, setStoryline] = useState<StorylineData | null>(null);
  const visibleStoryline = storyline?.sessionId === sessionId ? storyline : null;
  const graph = useMemo(
    () => buildStorylineGraph(visibleStoryline),
    [visibleStoryline],
  );
  const [isLoadingStoryline, setIsLoadingStoryline] = useState(false);
  const [selectingNodeId, setSelectingNodeId] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<string | null>(null);
  const [view, setView] = useState({ x: 0, y: 0 });
  const [zoom, setZoom] = useState(1);
  const zoomRef = useRef(1);
  const activeRound = displayRound || activeTurnId || 1;
  const activeRoundState = roundStates[activeRound];
  const isStoryGenerationLocked =
    isGameLoading ||
    Boolean(activeRoundState?.isAwaitingNarration) ||
    activeRoundState?.narrationStatus === "running";
  const isNodeSelectionDisabled =
    selectingNodeId !== null || isStoryGenerationLocked;
  const statusMessage = isLoadingStoryline
    ? "正在读取完整故事线..."
    : feedback ??
      (isStoryGenerationLocked ? "故事生成中，暂不能切换节点。" : null);

  const nodeById = useMemo(() => {
    const map = new Map<string, StorylineNode>();
    for (const node of graph.nodes) {
      map.set(node.id, node);
    }
    return map;
  }, [graph.nodes]);

  const visibleRounds = useMemo(
    () => graph.nodes.filter((node) => node.kind === "round").length,
    [graph.nodes],
  );

  useEffect(() => {
    if (!sessionId) {
      return;
    }

    let cancelled = false;

    const loadStoryline = async () => {
      setIsLoadingStoryline(true);
      setFeedback(null);
      try {
        const nextStoryline = await getGameSessionStoryline(sessionId);
        if (!cancelled) {
          setStoryline(nextStoryline);
        }
      } catch (error) {
        if (cancelled) {
          return;
        }
        setStoryline(null);
        setFeedback(
          error instanceof Error ? error.message : "读取故事线失败。",
        );
      } finally {
        if (!cancelled) {
          setIsLoadingStoryline(false);
        }
      }
    };

    void loadStoryline();

    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  const focusGraph = useCallback((nextZoom = 1) => {
    const viewport = viewportRef.current;
    const targetZoom = clampZoom(nextZoom);
    if (!viewport) {
      setView({ x: 0, y: 0 });
      setZoom(targetZoom);
      zoomRef.current = targetZoom;
      return;
    }

    setView({
      x: (viewport.clientWidth - graph.width * targetZoom) / 2,
      y: Math.max(12, (viewport.clientHeight - graph.height * targetZoom) / 2),
    });
    setZoom(targetZoom);
    zoomRef.current = targetZoom;
  }, [graph.height, graph.width]);

  useEffect(() => {
    focusGraph();
  }, [focusGraph]);

  const updateZoom = useCallback((nextZoom: number, anchor?: PointerPoint) => {
    const viewport = viewportRef.current;
    setZoom((previousZoom) => {
      const targetZoom = clampZoom(nextZoom);
      if (targetZoom === previousZoom) {
        return previousZoom;
      }
      zoomRef.current = targetZoom;

      if (!viewport) {
        return targetZoom;
      }

      const viewportRect = viewport.getBoundingClientRect();
      const viewportCenterX = anchor
        ? anchor.clientX - viewportRect.left
        : viewport.clientWidth / 2;
      const viewportCenterY = anchor
        ? anchor.clientY - viewportRect.top
        : viewport.clientHeight / 2;
      setView((previousView) => ({
        x: viewportCenterX - ((viewportCenterX - previousView.x) / previousZoom) * targetZoom,
        y: viewportCenterY - ((viewportCenterY - previousView.y) / previousZoom) * targetZoom,
      }));

      return targetZoom;
    });
  }, []);

  const handleWheel = useCallback((event: React.WheelEvent<HTMLDivElement>) => {
    event.preventDefault();
    const zoomDelta = event.deltaY > 0 ? -ZOOM_STEP : ZOOM_STEP;
    updateZoom(zoomRef.current + zoomDelta, {
      clientX: event.clientX,
      clientY: event.clientY,
    });
  }, [updateZoom]);

  useEffect(() => {
    if (!isOverlay || !onClose) {
      return undefined;
    }

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };

    window.addEventListener("keydown", handleEscape);
    return () => window.removeEventListener("keydown", handleEscape);
  }, [isOverlay, onClose]);

  const closeStoryline = useCallback(() => {
    if (onClose) {
      onClose();
      return;
    }

    if (sessionId) {
      navigate(routeWithSession(appRoutes.gameplay, sessionId));
    }
  }, [navigate, onClose, sessionId]);

  const openNode = useCallback(
    async (node: StorylineNode) => {
      if (!sessionId) {
        return;
      }
      if (isNodeSelectionDisabled) {
        if (isStoryGenerationLocked) {
          setFeedback("故事生成中，暂不能切换节点。");
        }
        return;
      }

      setSelectingNodeId(node.id);
      setFeedback("正在切换故事线...");
      try {
        const selected = await selectStorylineNode(sessionId, node.id);
        navigate(
          routeWithSession(
            selected.isEnding ? appRoutes.ending : appRoutes.gameplay,
            selected.sessionId,
          ),
          {
            state: {
              scrollNarrationToBottomKey: `${selected.sessionId}:${node.id}:${Date.now()}`,
            },
          },
        );
        onClose?.();
      } catch (error) {
        setFeedback(error instanceof Error ? error.message : "切换故事线失败。");
      } finally {
        setSelectingNodeId(null);
      }
    },
    [
      isNodeSelectionDisabled,
      isStoryGenerationLocked,
      navigate,
      onClose,
      selectStorylineNode,
      sessionId,
    ],
  );

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement).closest("button")) {
      return;
    }

    event.currentTarget.setPointerCapture(event.pointerId);
    activePointersRef.current.set(event.pointerId, {
      clientX: event.clientX,
      clientY: event.clientY,
    });

    if (activePointersRef.current.size >= 2) {
      const [firstPointer, secondPointer] = [...activePointersRef.current.values()];
      previousPinchDistanceRef.current = pointerDistance(
        firstPointer,
        secondPointer,
      );
      dragRef.current = null;
      return;
    }

    previousPinchDistanceRef.current = null;
    dragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: view.x,
      originY: view.y,
    };
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    if (activePointersRef.current.has(event.pointerId)) {
      activePointersRef.current.set(event.pointerId, {
        clientX: event.clientX,
        clientY: event.clientY,
      });
    }

    if (activePointersRef.current.size >= 2) {
      const [firstPointer, secondPointer] = [...activePointersRef.current.values()];
      const nextPinchDistance = pointerDistance(firstPointer, secondPointer);
      const previousPinchDistance = previousPinchDistanceRef.current;
      if (previousPinchDistance && nextPinchDistance > 0) {
        updateZoom(zoomRef.current * (nextPinchDistance / previousPinchDistance), pointerCenter(
          firstPointer,
          secondPointer,
        ));
      }
      previousPinchDistanceRef.current = nextPinchDistance;
      return;
    }

    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) {
      return;
    }

    setView((prev) => ({
      ...prev,
      x: drag.originX + event.clientX - drag.startX,
      y: drag.originY + event.clientY - drag.startY,
    }));
  };

  const handlePointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    activePointersRef.current.delete(event.pointerId);
    previousPinchDistanceRef.current = null;

    if (dragRef.current?.pointerId === event.pointerId) {
      dragRef.current = null;
    }

    if (activePointersRef.current.size === 1) {
      const [remainingPointerId, remainingPointer] = [
        ...activePointersRef.current.entries(),
      ][0];
      dragRef.current = {
        pointerId: remainingPointerId,
        startX: remainingPointer.clientX,
        startY: remainingPointer.clientY,
        originX: view.x,
        originY: view.y,
      };
    }
  };

  const content = (
    <ScreenShell
      className={cn(
        "h-full min-h-0 max-w-none items-stretch overflow-hidden px-0 py-0 sm:px-0 sm:py-0 md:px-0 md:py-0",
        isOverlay && "w-full",
      )}
    >
      <StoryFrame
        className={cn(
          "relative flex h-full max-w-none flex-col overflow-hidden px-2.5 py-2.5 sm:px-3 sm:py-3",
          isOverlay
            ? "rounded-3xl border border-[rgba(116,103,80,0.5)] bg-[rgba(8,14,26,0.95)] shadow-[0_24px_80px_rgba(1,8,20,0.6)]"
            : "rounded-none",
        )}
      >
        <div className="pointer-events-none absolute inset-0 bg-linear-to-b from-transparent via-[#08111d]/20 to-[#08111d]" />
        <div className="relative z-10 flex min-h-0 flex-1 flex-col gap-2">
          <header className="relative flex items-center justify-between gap-2 rounded-[1.1rem] border border-[rgba(116,103,80,0.4)] bg-[rgba(8,14,26,0.78)] px-3 py-2.5 backdrop-blur-md">
            <div className="min-w-0">
              <div className="flex items-center gap-2 text-xs font-semibold tracking-[0.18em] text-[#bca984]">
                <GitBranch className="h-3.5 w-3.5" />
                故事线
              </div>
            </div>
            <div className="pointer-events-none absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2">
              <div className="flex items-center gap-1.5 px-2.5 py-1 text-[0.72rem] text-[#d9cbb1] sm:text-xs">
                <span>{visibleRounds} 个已生成节点</span>
              </div>
            </div>
            <div className="flex shrink-0 items-center justify-end gap-2">
              <button
                type="button"
                onClick={closeStoryline}
                className={cn(
                  "inline-flex h-9 items-center justify-center rounded-full border border-[rgba(116,103,80,0.58)] bg-[rgba(48,49,59,0.9)] text-xs font-semibold text-[#f5ecdc] transition-colors hover:bg-[rgba(66,69,81,0.96)]",
                  isOverlay ? "w-9" : "gap-1.5 px-3",
                )}
                aria-label={isOverlay ? "关闭故事线" : "返回游玩页"}
                title={isOverlay ? "关闭故事线" : "返回游玩页"}
              >
                {isOverlay ? (
                  <X className="h-3.5 w-3.5" />
                ) : (
                  <>
                    <ArrowLeft className="h-3.5 w-3.5" />
                    返回
                  </>
                )}
              </button>
            </div>
          </header>

          {statusMessage ? (
            <div className="rounded-[0.95rem] border border-[#d6c3a0]/20 bg-[#121927]/82 px-3 py-2 text-xs leading-5 text-[#d9cbb1]">
              {statusMessage}
            </div>
          ) : null}

          <div className="relative min-h-0 flex-1 overflow-hidden rounded-[1.1rem] border border-[rgba(116,103,80,0.42)] bg-[rgba(5,10,20,0.68)]">
            <div className="absolute left-2 top-2 z-20 flex items-center gap-1 rounded-full border border-[rgba(116,103,80,0.38)] bg-[rgba(7,13,24,0.88)] p-1 shadow-[0_10px_24px_rgba(0,0,0,0.28)] backdrop-blur-md">
              <button
                type="button"
                onClick={() => focusGraph()}
                className="inline-flex h-8 w-8 items-center justify-center rounded-full text-[#f3ead8] transition-colors hover:bg-[rgba(188,169,124,0.14)]"
                aria-label="重置故事线视图"
                title="重置视图"
              >
                <LocateFixed className="h-3.5 w-3.5" />
              </button>
              <span className="h-5 w-px bg-[rgba(116,103,80,0.36)]" />
              <button
                type="button"
                onClick={() => updateZoom(zoom - ZOOM_STEP)}
                disabled={zoom <= MIN_ZOOM}
                className="inline-flex h-8 w-8 items-center justify-center rounded-full text-[#f3ead8] transition-colors hover:bg-[rgba(188,169,124,0.14)] disabled:cursor-not-allowed disabled:opacity-45"
                aria-label="缩小故事线"
                title="缩小"
              >
                <ZoomOut className="h-3.5 w-3.5" />
              </button>
              <span
                className="min-w-10 text-center font-mono text-[0.68rem] text-[#d9cbb1]"
                aria-label={`当前缩放 ${Math.round(zoom * 100)}%`}
              >
                {Math.round(zoom * 100)}%
              </span>
              <button
                type="button"
                onClick={() => updateZoom(zoom + ZOOM_STEP)}
                disabled={zoom >= MAX_ZOOM}
                className="inline-flex h-8 w-8 items-center justify-center rounded-full text-[#f3ead8] transition-colors hover:bg-[rgba(188,169,124,0.14)] disabled:cursor-not-allowed disabled:opacity-45"
                aria-label="放大故事线"
                title="放大"
              >
                <ZoomIn className="h-3.5 w-3.5" />
              </button>
            </div>

            <div
              ref={viewportRef}
              className="h-full w-full cursor-grab touch-none overflow-hidden active:cursor-grabbing"
              onPointerDown={handlePointerDown}
              onPointerMove={handlePointerMove}
              onPointerUp={handlePointerUp}
              onPointerCancel={handlePointerUp}
              onWheel={handleWheel}
            >
              <div
                className="relative origin-top-left"
                style={{
                  width: graph.width,
                  height: graph.height,
                  transform: `matrix(${zoom}, 0, 0, ${zoom}, ${view.x}, ${view.y})`,
                }}
              >
                <div
                  className="pointer-events-none absolute inset-0 opacity-70"
                  style={{
                    backgroundImage:
                      "linear-gradient(rgba(148,163,184,0.07) 1px, transparent 1px), linear-gradient(90deg, rgba(148,163,184,0.07) 1px, transparent 1px)",
                    backgroundSize: "44px 44px",
                  }}
                />
                <svg
                  className="pointer-events-none absolute inset-0"
                  width={graph.width}
                  height={graph.height}
                  viewBox={`0 0 ${graph.width} ${graph.height}`}
                  aria-hidden="true"
                >
                  <defs>
                    <marker
                      id="storyline-arrow"
                      markerWidth="5"
                      markerHeight="5"
                      refX="4"
                      refY="2.5"
                      orient="auto"
                    >
                      <path
                        d="M 0 0 L 5 2.5 L 0 5 z"
                        fill="#d8c18f"
                        opacity="0.52"
                      />
                    </marker>
                    <marker
                      id="storyline-obsession-arrow"
                      markerWidth="5"
                      markerHeight="5"
                      refX="4"
                      refY="2.5"
                      orient="auto"
                    >
                      <path
                        d="M 0 0 L 5 2.5 L 0 5 z"
                        fill="#f87171"
                        opacity="0.74"
                      />
                    </marker>
                  </defs>
                  {graph.edges.map((edge) => {
                    const from = nodeById.get(edge.from);
                    const to = nodeById.get(edge.to);
                    if (!from || !to) {
                      return null;
                    }

                    return (
                      <path
                        key={edge.id}
                        d={edgePath(from, to)}
                        fill="none"
                        stroke={edge.isObsession ? "#f87171" : "#d8c18f"}
                        strokeWidth={edge.isObsession ? 2 : 1.6}
                        strokeOpacity={edge.isObsession ? 0.72 : 0.5}
                        markerEnd={
                          edge.isObsession
                            ? "url(#storyline-obsession-arrow)"
                            : "url(#storyline-arrow)"
                        }
                      />
                    );
                  })}
                </svg>

                {graph.nodes.map((node) => (
                  <button
                    key={node.id}
                    type="button"
                    onClick={() => {
                      void openNode(node);
                    }}
                    disabled={isNodeSelectionDisabled}
                    className={cn(
                      "absolute flex flex-col items-center justify-center overflow-hidden rounded-[0.6rem] border border-[#d8c18f]/42 bg-[linear-gradient(180deg,rgba(18,30,51,0.96),rgba(10,17,31,0.94))] px-2 py-1.5 text-center shadow-[0_6px_14px_rgba(0,0,0,0.2)] transition-transform hover:-translate-y-0.5 focus:outline-none focus:ring-2 focus:ring-[#d8c18f]/45",
                      node.isObsession &&
                        "border-red-300/70 bg-[linear-gradient(180deg,rgba(54,25,39,0.98),rgba(28,18,32,0.95))] shadow-[0_0_0_1px_rgba(248,113,113,0.2),0_8px_18px_rgba(127,29,29,0.2)] focus:ring-red-300/45",
                      node.isActive &&
                        "border-[#8fa4ca]/85 bg-[linear-gradient(180deg,rgba(34,50,82,0.98),rgba(13,23,42,0.96))] shadow-[0_0_0_1px_rgba(143,164,202,0.35),0_10px_20px_rgba(0,0,0,0.28)]",
                      node.isActive &&
                        node.isObsession &&
                        "border-red-200/85 bg-[linear-gradient(180deg,rgba(72,30,48,0.98),rgba(35,20,36,0.96))] shadow-[0_0_0_1px_rgba(248,113,113,0.28),0_10px_22px_rgba(127,29,29,0.25)]",
                      selectingNodeId === node.id &&
                        "border-[#8fa4ca]/70 text-[#8fa4ca]",
                      selectingNodeId !== null && "cursor-wait hover:translate-y-0",
                      isStoryGenerationLocked &&
                        "cursor-not-allowed opacity-60 hover:translate-y-0",
                    )}
                    style={{
                      left: node.x,
                      top: node.y,
                      width: node.width,
                      height: node.height,
                    }}
                    title={
                      isStoryGenerationLocked
                        ? "故事生成中，暂不能切换节点"
                        : node.isActive
                        ? `当前故事线：第 ${node.round} 章`
                        : node.isObsession
                          ? `切换到第 ${node.round} 章（执念分支）`
                        : `切换到第 ${node.round} 章`
                    }
                  >
                    <span className="line-clamp-1 max-w-full text-[0.74rem] font-semibold leading-4 text-[#f6eddc]">
                      {node.title}
                    </span>
                    {node.incomingChoice ? (
                      <span
                        className={cn(
                          "mt-0.5 flex max-w-full items-center justify-center gap-1 text-[0.62rem] font-medium leading-3 text-[#bca984]",
                          node.isObsession && "text-red-100/90",
                        )}
                      >
                        <MousePointer2
                          className={cn(
                            "h-2.5 w-2.5 shrink-0 text-[#8fa4ca]",
                            node.isObsession && "text-red-300",
                          )}
                        />
                        <span className="min-w-0 truncate">
                          {node.incomingChoice}
                        </span>
                      </span>
                    ) : null}
                  </button>
                ))}

                {graph.nodes.length === 0 ? (
                  <div className="absolute left-[160px] top-[120px] w-[min(26rem,calc(100vw-3rem))] rounded-[1rem] border border-[rgba(116,103,80,0.42)] bg-[rgba(8,14,26,0.9)] px-4 py-4 text-sm leading-6 text-[#d9cbb1]">
                    这段记录还没有可展示的章节。先回到游玩页，让第一章显影出来。
                  </div>
                ) : null}
              </div>
            </div>
          </div>
        </div>
      </StoryFrame>
    </ScreenShell>
  );

  if (!isOverlay) {
    return content;
  }

  return (
    <div className="fixed inset-0 z-[60] flex items-end justify-center bg-[rgba(5,8,15,0.72)] px-3 py-4 backdrop-blur-sm sm:items-center sm:px-6">
      <div
        className="absolute inset-0"
        onClick={closeStoryline}
        aria-hidden="true"
      />
      <div className="relative z-10 flex h-[min(88svh,48rem)] min-h-[28rem] w-full max-w-5xl flex-col">
        {content}
      </div>
    </div>
  );
};

export default StorylinePage;
