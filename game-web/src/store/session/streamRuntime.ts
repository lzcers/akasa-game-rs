import type { StoreApi } from 'zustand';
import type { LiveEngineEvent } from '../../lib/api';
import {
  getGameSession,
  openGameSessionStream,
} from '../../lib/api';
import { useGameInternalStore } from '../gameStore';
import { reduceStreamEvent } from './streamReducer';
import { applySessionSnapshotToStores } from './stateSync';
import type { GameUIStoreState } from '../gameUIStore';

interface SessionStreamHandlers {
  onEngineEvent: (event: LiveEngineEvent) => void;
  onStreamConnected: () => void;
  onStreamError: () => void;
}

interface GameSessionStreamRuntime {
  set: StoreApi<GameUIStoreState>['setState'];
  get: StoreApi<GameUIStoreState>['getState'];
}

let activeSessionStream: EventSource | null = null;
let activeStreamSessionId: string | null = null;
let activeStreamGeneration = 0;
let lastStreamEventId: string | null = null;
let endingSnapshotSyncTimer: number | null = null;
const STREAM_RECONNECTING_MESSAGE = '连接有些不稳定，正在为你续接这段记录...';

export function isSessionStreamActive(sessionId: string): boolean {
  return activeStreamSessionId === sessionId;
}

function isCurrentSessionStream(sessionId: string, generation: number): boolean {
  return activeStreamSessionId === sessionId && activeStreamGeneration === generation;
}

export function closeSessionStream() {
  activeStreamGeneration += 1;
  activeSessionStream?.close();
  activeSessionStream = null;
  activeStreamSessionId = null;
  lastStreamEventId = null;
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

export function connectSessionStream(sessionId: string, handlers: SessionStreamHandlers) {
  activeSessionStream?.close();
  const streamGeneration = activeStreamGeneration + 1;
  activeStreamGeneration = streamGeneration;
  activeStreamSessionId = sessionId;
  activeSessionStream = openGameSessionStream(
    sessionId,
    {
      onOpen: () => {
        if (!isCurrentSessionStream(sessionId, streamGeneration)) {
          return;
        }
        handlers.onStreamConnected();
      },
      onEngineEvent: (event, lastEventId) => {
        if (!isCurrentSessionStream(sessionId, streamGeneration)) {
          return;
        }
        handlers.onStreamConnected();
        lastStreamEventId = lastEventId || lastStreamEventId;
        handlers.onEngineEvent(event);
      },
      onError: () => {
        if (!isCurrentSessionStream(sessionId, streamGeneration)) {
          return;
        }
        handlers.onStreamError();
      },
    },
    lastStreamEventId,
  );
  return streamGeneration;
}

function applyStreamEventToStores(
  runtime: GameSessionStreamRuntime,
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
  runtime: GameSessionStreamRuntime,
  sessionId: string,
  streamGeneration: number,
) {
  if (!isCurrentSessionStream(sessionId, streamGeneration)) {
    return;
  }

  try {
    const session = await getGameSession(sessionId);
    if (!isCurrentSessionStream(sessionId, streamGeneration)) {
      return;
    }

    runtime.set({
      stateView: applySessionSnapshotToStores(session),
      generatedProfiles: session.generatedProfiles,
      isLoading: false,
      error: session.phase === 'failed' ? '故事推进失败，请稍后重试。' : null,
    });
  } catch (error) {
    if (!isCurrentSessionStream(sessionId, streamGeneration)) {
      return;
    }
    runtime.set({
      isLoading: false,
      error: error instanceof Error ? error.message : '同步故事状态失败。',
    });
  }
}

export function connectGameSessionStream(
  sessionId: string,
  runtime: GameSessionStreamRuntime,
) {
  let streamGeneration = 0;
  streamGeneration = connectSessionStream(sessionId, {
    onEngineEvent: (event) => {
      if (applyStreamEventToStores(runtime, event)) {
        scheduleSnapshotSync(sessionId, (nextSessionId) => {
          void syncActiveSessionSnapshot(runtime, nextSessionId, streamGeneration);
        });
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
