import type {
  StorylineData,
  StorylineEdgeData,
  StorylineNodeData,
} from "../lib/api";

export interface StorylineNode {
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
  isObsession: boolean;
  isActive: boolean;
}

export interface StorylineEdge {
  id: string;
  from: string;
  to: string;
  isObsession: boolean;
}

export interface StorylineGraph {
  nodes: StorylineNode[];
  edges: StorylineEdge[];
  width: number;
  height: number;
}

const ROUND_WIDTH = 156;
const ROUND_HEIGHT = 54;
const LEVEL_GAP = 86;
const CANVAS_PADDING = 48;
const SIBLING_GAP = 18;
const NODE_STEP = ROUND_WIDTH + SIBLING_GAP;

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

export function edgePath(from: StorylineNode, to: StorylineNode): string {
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

function isObsessionEdge(edge: StorylineEdgeData | undefined): boolean {
  return Boolean(
    edge?.actions.some(
      (action) =>
        action.action_type === "free_text" &&
        action.action.trim() &&
        action.action.trim() !== "continue",
    ),
  );
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

export function buildStorylineGraph(
  storyline: StorylineData | null,
): StorylineGraph {
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
  const sourceNodeById = new Map(
    sourceNodes.map((node) => [node.nodeId, node] as const),
  );
  const childrenByNodeId = new Map<string, StorylineNodeData[]>();

  for (const edge of graphEdges) {
    incomingEdges.set(edge.toNodeId, edge);
    const parent = sourceNodeById.get(edge.fromNodeId);
    const child = sourceNodeById.get(edge.toNodeId);
    if (!parent || !child) {
      continue;
    }

    const children = childrenByNodeId.get(parent.nodeId) ?? [];
    children.push(child);
    childrenByNodeId.set(parent.nodeId, children);
  }

  for (const children of childrenByNodeId.values()) {
    children.sort(sortStorylineNodes);
  }

  const levelKeys = [...new Set(sourceNodes.map((node) => node.round))].sort(
    (left, right) => left - right,
  );
  const levelIndexByRound = new Map(
    levelKeys.map((round, index) => [round, index] as const),
  );
  const roots = sourceNodes
    .filter((node) => !incomingEdges.has(node.nodeId))
    .sort(sortStorylineNodes);
  const orderedRoots = roots.length > 0 ? roots : sourceNodes;
  const nodeCenterX = new Map<string, number>();
  const visitedNodeIds = new Set<string>();
  let nextLeafIndex = 0;

  const layoutSubtree = (
    node: StorylineNodeData,
    visiting = new Set<string>(),
  ): number => {
    const existingCenter = nodeCenterX.get(node.nodeId);
    if (existingCenter != null) {
      return existingCenter;
    }
    if (visiting.has(node.nodeId)) {
      const fallbackCenter = nextLeafIndex * NODE_STEP + ROUND_WIDTH / 2;
      nextLeafIndex += 1;
      nodeCenterX.set(node.nodeId, fallbackCenter);
      return fallbackCenter;
    }

    visiting.add(node.nodeId);
    const children = (childrenByNodeId.get(node.nodeId) ?? []).filter((child) =>
      nodeIds.has(child.nodeId),
    );
    let center: number;
    if (children.length === 0) {
      center = nextLeafIndex * NODE_STEP + ROUND_WIDTH / 2;
      nextLeafIndex += 1;
    } else {
      const childCenters = children.map((child) =>
        layoutSubtree(child, visiting),
      );
      center = (childCenters[0] + childCenters[childCenters.length - 1]) / 2;
    }
    visiting.delete(node.nodeId);
    visitedNodeIds.add(node.nodeId);
    nodeCenterX.set(node.nodeId, center);
    return center;
  };

  for (const root of orderedRoots) {
    layoutSubtree(root);
  }
  for (const node of sourceNodes) {
    if (!visitedNodeIds.has(node.nodeId)) {
      layoutSubtree(node);
    }
  }

  const nodesByLevel = new Map<number, StorylineNodeData[]>();
  for (const node of sourceNodes) {
    const levelIndex = levelIndexByRound.get(node.round) ?? 0;
    const levelNodes = nodesByLevel.get(levelIndex) ?? [];
    levelNodes.push(node);
    nodesByLevel.set(levelIndex, levelNodes);
  }

  const leafCount = Math.max(nextLeafIndex, 1);
  const graphWidth =
    leafCount * ROUND_WIDTH +
    Math.max(0, leafCount - 1) * SIBLING_GAP +
    CANVAS_PADDING * 2;
  const nodes: StorylineNode[] = [];

  for (const [levelIndex, levelNodes] of nodesByLevel.entries()) {
    levelNodes.sort(sortStorylineNodes).forEach((node) => {
      const centerX = nodeCenterX.get(node.nodeId) ?? ROUND_WIDTH / 2;
      const incomingEdge = incomingEdges.get(node.nodeId);
      nodes.push({
        id: node.nodeId,
        kind: "round",
        round: node.round,
        x: CANVAS_PADDING + centerX - ROUND_WIDTH / 2,
        y: CANVAS_PADDING + levelIndex * LEVEL_GAP,
        width: ROUND_WIDTH,
        height: ROUND_HEIGHT,
        title: node.title || (node.round === 0 ? "根节点" : `第 ${node.round} 章`),
        summary: trimText(node.narrationText, "这一章仍在铺展。"),
        incomingChoice: incomingChoiceTitle(incomingEdge),
        isObsession: isObsessionEdge(incomingEdge),
        isActive: node.nodeId === storyline.activeNodeId,
      });
    });
  }

  const edges: StorylineEdge[] = graphEdges.map((edge) => ({
    id: `${edge.fromNodeId}-${edge.toNodeId}`,
    from: edge.fromNodeId,
    to: edge.toNodeId,
    isObsession: isObsessionEdge(edge),
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
