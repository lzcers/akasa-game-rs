import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { ArrowLeft, GitBranch, LocateFixed, MousePointer2 } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { ScreenShell, StoryFrame } from "../components/AkashicUI";
import { appRoutes, routeWithSession } from "../lib/appRoutes";
import {
  getGameSessionStoryline,
  type StorylineData,
  type StorylineEdgeData,
  type StorylineNodeData,
} from "../lib/api";
import { cn } from "../lib/utils";
import { useGameInternalStore } from "../store/gameStore";
import { useGameUIStore } from "../store/gameUIStore";

interface StorylineNode {
  id: string;
  kind: "round";
  round: number;
  x: number;
  y: number;
  width: number;
  height: number;
  title: string;
  summary: string;
  incomingChoice: string | null;
}

interface StorylineEdge {
  id: string;
  from: string;
  to: string;
}

interface StorylineGraph {
  nodes: StorylineNode[];
  edges: StorylineEdge[];
  width: number;
  height: number;
}

const ROUND_WIDTH = 164;
const ROUND_HEIGHT = 58;
const LEVEL_GAP = 108;
const CANVAS_PADDING = 70;
const SIBLING_GAP = 34;

function trimText(
  value: string | null | undefined,
  fallback = "尚未显影",
): string {
  const trimmed = value?.replace(/\s+/g, " ").trim();
  return trimmed || fallback;
}

function nodeCenter(node: StorylineNode) {
  return {
    x: node.x + node.width / 2,
    y: node.y + node.height / 2,
  };
}

function edgePath(from: StorylineNode, to: StorylineNode): string {
  const start = nodeCenter(from);
  const end = nodeCenter(to);
  const controlOffset = Math.max(32, Math.abs(end.y - start.y) * 0.26);

  return [
    `M ${start.x} ${from.y + from.height}`,
    `C ${start.x} ${from.y + from.height + controlOffset}`,
    `${end.x} ${to.y - controlOffset}`,
    `${end.x} ${to.y}`,
  ].join(" ");
}

function incomingChoiceTitle(edge: StorylineEdgeData | undefined): string | null {
  if (!edge) {
    return null;
  }

  const firstAction = edge.actions[0];
  if (!firstAction) {
    return "继续回响";
  }

  return trimText(firstAction.title || firstAction.action, "继续回响");
}

function sortStorylineNodes(
  left: StorylineNodeData,
  right: StorylineNodeData,
): number {
  return (
    left.round - right.round ||
    left.sequenceIndex - right.sequenceIndex ||
    left.nodeId.localeCompare(right.nodeId)
  );
}

function buildStorylineGraph(storyline: StorylineData | null): StorylineGraph {
  if (!storyline) {
    return {
      nodes: [],
      edges: [],
      width: ROUND_WIDTH + CANVAS_PADDING * 2,
      height: 480,
    };
  }

  const sourceNodes = storyline.nodes
    .filter((node) => node.nodeId !== storyline.rootNodeId)
    .sort(sortStorylineNodes);
  const nodeIds = new Set(sourceNodes.map((node) => node.nodeId));
  const incomingEdges = new Map<string, StorylineEdgeData>();
  const graphEdges = storyline.edges.filter(
    (edge) => nodeIds.has(edge.fromNodeId) && nodeIds.has(edge.toNodeId),
  );

  for (const edge of graphEdges) {
    incomingEdges.set(edge.toNodeId, edge);
  }

  const levelKeys = [...new Set(sourceNodes.map((node) => node.round))].sort(
    (left, right) => left - right,
  );
  const levelIndexByRound = new Map(
    levelKeys.map((round, index) => [round, index] as const),
  );
  const nodesByLevel = new Map<number, StorylineNodeData[]>();
  for (const node of sourceNodes) {
    const levelIndex = levelIndexByRound.get(node.round) ?? 0;
    const levelNodes = nodesByLevel.get(levelIndex) ?? [];
    levelNodes.push(node);
    nodesByLevel.set(levelIndex, levelNodes);
  }

  const widestLevelCount = Math.max(
    1,
    ...[...nodesByLevel.values()].map((levelNodes) => levelNodes.length),
  );
  const graphWidth =
    widestLevelCount * ROUND_WIDTH +
    Math.max(0, widestLevelCount - 1) * SIBLING_GAP +
    CANVAS_PADDING * 2;
  const nodes: StorylineNode[] = [];

  for (const [levelIndex, levelNodes] of nodesByLevel.entries()) {
    const levelWidth =
      levelNodes.length * ROUND_WIDTH +
      Math.max(0, levelNodes.length - 1) * SIBLING_GAP;
    const startX = (graphWidth - levelWidth) / 2;

    levelNodes.sort(sortStorylineNodes).forEach((node, index) => {
      nodes.push({
        id: node.nodeId,
        kind: "round",
        round: node.round,
        x: startX + index * (ROUND_WIDTH + SIBLING_GAP),
        y: CANVAS_PADDING + levelIndex * LEVEL_GAP,
        width: ROUND_WIDTH,
        height: ROUND_HEIGHT,
        title: node.title || (node.round === 0 ? "根节点" : `第 ${node.round} 章`),
        summary: trimText(node.narrationText, "这一章仍在铺展。"),
        incomingChoice: incomingChoiceTitle(incomingEdges.get(node.nodeId)),
      });
    });
  }

  const edges: StorylineEdge[] = graphEdges.map((edge) => ({
    id: `${edge.fromNodeId}-${edge.toNodeId}`,
    from: edge.fromNodeId,
    to: edge.toNodeId,
  }));

  const deepestContentBottom = nodes.reduce(
    (max, node) => Math.max(max, node.y + node.height),
    CANVAS_PADDING + ROUND_HEIGHT,
  );

  return {
    nodes,
    edges,
    width: graphWidth,
    height: Math.max(480, deepestContentBottom + CANVAS_PADDING),
  };
}

const StorylinePage: React.FC = () => {
  const navigate = useNavigate();
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    originX: number;
    originY: number;
  } | null>(null);
  const sessionId = useGameInternalStore((state) => state.sessionId);
  const selectStorylineNode = useGameUIStore(
    (state) => state.selectStorylineNode,
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

  const focusGraph = useCallback(() => {
    const viewport = viewportRef.current;
    if (!viewport) {
      setView({ x: 0, y: 0 });
      return;
    }

    setView({
      x: (viewport.clientWidth - graph.width) / 2,
      y: Math.max(12, (viewport.clientHeight - graph.height) / 2),
    });
  }, [graph.height, graph.width]);

  useEffect(() => {
    focusGraph();
  }, [focusGraph]);

  const openNode = useCallback(
    async (node: StorylineNode) => {
      if (!sessionId) {
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
      } catch (error) {
        setFeedback(error instanceof Error ? error.message : "切换故事线失败。");
      } finally {
        setSelectingNodeId(null);
      }
    },
    [navigate, selectStorylineNode, sessionId],
  );

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement).closest("button")) {
      return;
    }

    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: view.x,
      originY: view.y,
    };
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
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
    if (dragRef.current?.pointerId === event.pointerId) {
      dragRef.current = null;
    }
  };

  return (
    <ScreenShell className="h-full min-h-0 max-w-none items-stretch overflow-hidden px-0 py-0 sm:px-0 sm:py-0 md:px-0 md:py-0">
      <StoryFrame className="relative flex h-full max-w-none flex-col overflow-hidden rounded-none px-2.5 py-2.5 sm:px-3 sm:py-3">
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
                onClick={() => {
                  if (sessionId) {
                    navigate(routeWithSession(appRoutes.gameplay, sessionId));
                  }
                }}
                className="inline-flex h-9 items-center justify-center gap-1.5 rounded-full border border-[rgba(116,103,80,0.58)] bg-[rgba(48,49,59,0.9)] px-3 text-xs font-semibold text-[#f5ecdc] transition-colors hover:bg-[rgba(66,69,81,0.96)]"
              >
                <ArrowLeft className="h-3.5 w-3.5" />
                返回
              </button>
            </div>
          </header>

          {feedback || isLoadingStoryline ? (
            <div className="rounded-[0.95rem] border border-[#d6c3a0]/20 bg-[#121927]/82 px-3 py-2 text-xs leading-5 text-[#d9cbb1]">
              {isLoadingStoryline ? "正在读取完整故事线..." : feedback}
            </div>
          ) : null}

          <div className="relative min-h-0 flex-1 overflow-hidden rounded-[1.1rem] border border-[rgba(116,103,80,0.42)] bg-[rgba(5,10,20,0.68)]">
            <div className="absolute left-2 top-2 z-20 flex items-center gap-1 rounded-full border border-[rgba(116,103,80,0.38)] bg-[rgba(7,13,24,0.88)] p-1 shadow-[0_10px_24px_rgba(0,0,0,0.28)] backdrop-blur-md">
              <button
                type="button"
                onClick={focusGraph}
                className="inline-flex h-8 w-8 items-center justify-center rounded-full text-[#f3ead8] transition-colors hover:bg-[rgba(188,169,124,0.14)]"
                aria-label="重置故事线视图"
                title="重置视图"
              >
                <LocateFixed className="h-3.5 w-3.5" />
              </button>
            </div>

            <div
              ref={viewportRef}
              className="h-full w-full cursor-grab touch-none overflow-hidden active:cursor-grabbing"
              onPointerDown={handlePointerDown}
              onPointerMove={handlePointerMove}
              onPointerUp={handlePointerUp}
              onPointerCancel={handlePointerUp}
            >
              <div
                className="relative origin-top-left"
                style={{
                  width: graph.width,
                  height: graph.height,
                  transform: `translate(${view.x}px, ${view.y}px)`,
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
                        stroke="#d8c18f"
                        strokeWidth={1.6}
                        strokeOpacity={0.5}
                        markerEnd="url(#storyline-arrow)"
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
                    disabled={selectingNodeId !== null}
                    className={cn(
                      "absolute flex flex-col items-center justify-center overflow-hidden rounded-[0.6rem] border border-[#d8c18f]/42 bg-[linear-gradient(180deg,rgba(18,30,51,0.96),rgba(10,17,31,0.94))] px-2 py-1.5 text-center shadow-[0_6px_14px_rgba(0,0,0,0.2)] transition-transform hover:-translate-y-0.5 focus:outline-none focus:ring-2 focus:ring-[#d8c18f]/45",
                      selectingNodeId === node.id &&
                        "border-[#8fa4ca]/70 text-[#8fa4ca]",
                      selectingNodeId !== null && "cursor-wait hover:translate-y-0",
                    )}
                    style={{
                      left: node.x,
                      top: node.y,
                      width: node.width,
                      height: node.height,
                    }}
                    title={`切换到第 ${node.round} 章`}
                  >
                    <span className="line-clamp-1 max-w-full text-[0.74rem] font-semibold leading-4 text-[#f6eddc]">
                      {node.title}
                    </span>
                    {node.incomingChoice ? (
                      <span className="mt-0.5 flex max-w-full items-center justify-center gap-1 text-[0.62rem] font-medium leading-3 text-[#bca984]">
                        <MousePointer2 className="h-2.5 w-2.5 shrink-0 text-[#8fa4ca]" />
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
};

export default StorylinePage;
