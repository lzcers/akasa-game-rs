import type { StoreApi } from 'zustand';
import type { LiveEngineEvent } from '../../lib/api';
import {
  getGameSessionStoryNode,
  getGameSession,
  openGameSessionStoryNodeStream,
} from '../../lib/api';
import { useGameInternalStore } from '../gameStore';
import { roundStateFromPersistedHistoryEntry } from './mappers';
import { reduceStreamEvent } from './streamReducer';
import { applySessionSnapshotToStores } from './stateSync';
import type { GameUIStoreState } from '../gameUIStore';

interface NodeStreamHandlers {
  onEngineEvent: (event: LiveEngineEvent) => void;
  onStreamConnected: () => void;
  onStreamError: () => void;
}

interface GameSessionNodeStreamRuntime {
  set: StoreApi<GameUIStoreState>['setState'];
  get: StoreApi<GameUIStoreState>['getState'];
}

let activeNodeStream: EventSource | null = null;
let activeNodeStreamSessionId: string | null = null;
let activeNodeStreamNodeId: string | null = null;
let activeNodeStreamGeneration = 0;
let lastNodeStreamEventId: string | null = null;
let endingSnapshotSyncTimer: number | null = null;
const STREAM_RECONNECTING_MESSAGE = '连接有些不稳定，正在为你续接这段记录...';

export function isStoryNodeStreamActiveForSession(sessionId: string): boolean {
  return activeNodeStreamSessionId === sessionId && activeNodeStream !== null;
}

function isCurrentNodeStream(sessionId: string, nodeId: string, generation: number): boolean {
  return activeNodeStreamSessionId === sessionId
    && activeNodeStreamNodeId === nodeId
    && activeNodeStreamGeneration === generation;
}

export function closeStoryNodeStream() {
  activeNodeStreamGeneration += 1;
  activeNodeStream?.close();
  activeNodeStream = null;
  activeNodeStreamSessionId = null;
  activeNodeStreamNodeId = null;
  lastNodeStreamEventId = null;
  if (endingSnapshotSyncTimer !== null) {
    window.clearTimeout(endingSnapshotSyncTimer);
    endingSnapshotSyncTimer = null;
  }
}

function scheduleSnapshotSync(sessionId: string, handler: (sessionId: string) => void) {
  if (endingSnapshotSyncTimer !== null) {
    window.clearTimeout(endingSnapshotSyncTimer);
  }
  endingSnapshotSyncTimer = window.setTimeout(() => {
    endingSnapshotSyncTimer = null;
    handler(sessionId);
  }, 120);
}

export function connectNodeStream(
  sessionId: string,
  nodeId: string,
  handlers: NodeStreamHandlers,
) {
  activeNodeStream?.close();
  const streamGeneration = activeNodeStreamGeneration + 1;
  activeNodeStreamGeneration = streamGeneration;
  activeNodeStreamSessionId = sessionId;
  activeNodeStreamNodeId = nodeId;
  activeNodeStream = openGameSessionStoryNodeStream(
    sessionId,
    nodeId,
    {
      onOpen: () => {
        if (!isCurrentNodeStream(sessionId, nodeId, streamGeneration)) {
          return;
        }
        handlers.onStreamConnected();
      },
      onEngineEvent: (event, lastEventId) => {
        if (!isCurrentNodeStream(sessionId, nodeId, streamGeneration)) {
          return;
        }
        handlers.onStreamConnected();
        lastNodeStreamEventId = lastEventId || lastNodeStreamEventId;
        handlers.onEngineEvent(event);
      },
      onError: () => {
        if (!isCurrentNodeStream(sessionId, nodeId, streamGeneration)) {
          return;
        }
        handlers.onStreamError();
      },
    },
    lastNodeStreamEventId,
  );
  return streamGeneration;
}

function finishCurrentNodeStream(sessionId: string, nodeId: string, generation: number) {
  if (!isCurrentNodeStream(sessionId, nodeId, generation)) {
    return;
  }
  activeNodeStream?.close();
  activeNodeStream = null;
  activeNodeStreamSessionId = null;
  activeNodeStreamNodeId = null;
  lastNodeStreamEventId = null;
}

function applyMaterializedNodeToStores(node: Awaited<ReturnType<typeof getGameSessionStoryNode>>) {
  if (!node.data) {
    return;
  }

  useGameInternalStore.setState((state) => ({
    turnIndex: Math.max(state.turnIndex, node.round),
    displayRound: node.round,
    roundStates: {
      ...state.roundStates,
      [node.round]: roundStateFromPersistedHistoryEntry(node.data),
    },
  }));
}

function applyStreamEventToStores(
  runtime: GameSessionNodeStreamRuntime,
  event: LiveEngineEvent,
) {
  const { internalStatePatch, uiStatePatch, shouldSyncSnapshot } = reduceStreamEvent({
    internalState: useGameInternalStore.getState(),
    uiState: runtime.get(),
    event: event.event,
  });

  if (internalStatePatch) {
    useGameInternalStore.setState(internalStatePatch);
  }

  if (uiStatePatch) {
    runtime.set(uiStatePatch);
  }

  return shouldSyncSnapshot;
}

async function syncActiveSessionSnapshot(
  runtime: GameSessionNodeStreamRuntime,
  sessionId: string,
  nodeId: string,
  streamGeneration: number,
) {
  if (!isCurrentNodeStream(sessionId, nodeId, streamGeneration)) {
    return;
  }

  try {
    const session = await getGameSession(sessionId);
    if (!isCurrentNodeStream(sessionId, nodeId, streamGeneration)) {
      return;
    }

    runtime.set({
      stateView: applySessionSnapshotToStores(session),
      generatedProfiles: session.generatedProfiles,
      isLoading: false,
      error: session.phase === 'failed' ? '故事推进失败，请稍后重试。' : null,
    });
  } catch (error) {
    if (!isCurrentNodeStream(sessionId, nodeId, streamGeneration)) {
      return;
    }
    runtime.set({
      isLoading: false,
      error: error instanceof Error ? error.message : '同步故事状态失败。',
    });
  }
}

export function materializeActiveStoryNode(
  sessionId: string,
  runtime: GameSessionNodeStreamRuntime,
) {
  void materializeActiveStoryNodeFromSession(sessionId, runtime);
}

export async function materializeActiveStoryNodeFromSession(
  sessionId: string,
  runtime: GameSessionNodeStreamRuntime,
) {
  const session = await getGameSession(sessionId);
  runtime.set({
    stateView: applySessionSnapshotToStores(session),
    generatedProfiles: session.generatedProfiles,
    isLoading: false,
    error: null,
  });
  await materializeStoryNode(sessionId, session.activeNodeId, runtime);
}

export async function materializeStoryNode(
  sessionId: string,
  nodeId: string,
  runtime: GameSessionNodeStreamRuntime,
) {
  const node = await getGameSessionStoryNode(sessionId, nodeId);
  if (node.status === 'complete' || node.status === 'ended') {
    closeStoryNodeStream();
    applyMaterializedNodeToStores(node);
    runtime.set({
      isLoading: false,
      error: null,
      skipRestoredNarrationAnimation: true,
    });
    return;
  }

  if (node.status === 'failed') {
    closeStoryNodeStream();
    runtime.set({
      isLoading: false,
      error: '故事推进失败，请稍后重试。',
    });
    return;
  }

  let streamGeneration = 0;
  streamGeneration = connectNodeStream(sessionId, node.nodeId, {
    onEngineEvent: (event) => {
      if (applyStreamEventToStores(runtime, event)) {
        scheduleSnapshotSync(sessionId, (nextSessionId) => {
          void syncActiveSessionSnapshot(runtime, nextSessionId, node.nodeId, streamGeneration);
        });
      }
      if (
        event.event.type === 'flow_turn_completed'
        || event.event.type === 'flow_turn_end'
        || event.event.type === 'flow_turn_error'
      ) {
        finishCurrentNodeStream(sessionId, node.nodeId, streamGeneration);
      }
    },
    onStreamConnected: () => {
      if (runtime.get().error === STREAM_RECONNECTING_MESSAGE) {
        runtime.set({ error: null });
      }
    },
    onStreamError: () => {
      runtime.set({
        error: STREAM_RECONNECTING_MESSAGE,
      });
    },
  });
}
