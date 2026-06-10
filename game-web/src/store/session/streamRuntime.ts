import type { StoreApi } from 'zustand';
import type { TaskView } from '../../lib/api';
import {
  getGameSession,
  openGameSessionStream,
} from '../../lib/api';
import { useGameInternalStore } from '../gameStore';
import { reduceStreamTask } from './streamReducer';
import { applySessionSnapshotToStores } from './stateSync';
import { applyTaskUpdate } from './taskUpdates';
import type { GameUIStoreState } from '../gameUIStore';

interface SessionStreamHandlers {
  onTaskUpdated: (task: TaskView, boundRound?: number | null) => void;
  onStreamConnected: () => void;
  onStreamError: () => void;
  onSnapshotSyncRequested: (sessionId: string) => void;
}

interface GameSessionStreamRuntime {
  set: StoreApi<GameUIStoreState>['setState'];
  get: StoreApi<GameUIStoreState>['getState'];
}

let activeSessionStream: EventSource | null = null;
let activeStreamSessionId: string | null = null;
let lastStreamEventId: string | null = null;
let activeStreamTasks = new Map<string, TaskView>();
let activeStreamTaskRounds = new Map<string, number>();
let endingSnapshotSyncTimer: number | null = null;
const STREAM_RECONNECTING_MESSAGE = '连接有些不稳定，正在为你续接这段记录...';

export function isSessionStreamActive(sessionId: string): boolean {
  return activeStreamSessionId === sessionId;
}

export function closeSessionStream() {
  activeSessionStream?.close();
  activeSessionStream = null;
  activeStreamSessionId = null;
  lastStreamEventId = null;
  activeStreamTasks = new Map();
  activeStreamTaskRounds = new Map();
  if (endingSnapshotSyncTimer !== null) {
    window.clearTimeout(endingSnapshotSyncTimer);
    endingSnapshotSyncTimer = null;
  }
}

function scheduleSnapshotSync(sessionId: string, handler: SessionStreamHandlers['onSnapshotSyncRequested']) {
  if (endingSnapshotSyncTimer !== null) {
    window.clearTimeout(endingSnapshotSyncTimer);
  }
  endingSnapshotSyncTimer = window.setTimeout(() => {
    endingSnapshotSyncTimer = null;
    handler(sessionId);
  }, 120);
}

export function connectSessionStream(sessionId: string, handlers: SessionStreamHandlers) {
  activeStreamSessionId = sessionId;
  activeSessionStream = openGameSessionStream(
    sessionId,
    {
      onOpen: () => {
        if (!isSessionStreamActive(sessionId)) {
          return;
        }
        handlers.onStreamConnected();
      },
      onTaskUpdated: (event, lastEventId) => {
        if (!isSessionStreamActive(sessionId)) {
          return;
        }
        handlers.onStreamConnected();
        lastStreamEventId = lastEventId || lastStreamEventId;
        if (event.kind === 'narration' || event.kind === 'protagonist_action') {
          activeStreamTaskRounds.set(event.entity, Math.max(event.round, 1));
        }
        const nextTask = applyTaskUpdate(activeStreamTasks, event);
        handlers.onTaskUpdated(nextTask, activeStreamTaskRounds.get(event.entity));
        if (event.kind === 'narration' && event.status === 'done') {
          scheduleSnapshotSync(sessionId, handlers.onSnapshotSyncRequested);
        }
        if (event.status === 'error') {
          scheduleSnapshotSync(sessionId, handlers.onSnapshotSyncRequested);
        }
      },
      onError: () => {
        if (!isSessionStreamActive(sessionId)) {
          return;
        }
        handlers.onStreamError();
      },
    },
    lastStreamEventId,
  );
}

function applyStreamTaskToStores(
  runtime: GameSessionStreamRuntime,
  task: TaskView,
  boundRound?: number | null,
) {
  const { internalStatePatch, uiStatePatch } = reduceStreamTask({
    internalState: useGameInternalStore.getState(),
    uiState: runtime.get(),
    task,
    boundRound,
  });

  if (internalStatePatch) {
    useGameInternalStore.setState(internalStatePatch);
  }

  if (uiStatePatch) {
    runtime.set(uiStatePatch);
  }
}

async function syncActiveSessionSnapshot(
  runtime: GameSessionStreamRuntime,
  sessionId: string,
) {
  if (!isSessionStreamActive(sessionId)) {
    return;
  }

  try {
    const session = await getGameSession(sessionId);
    if (!isSessionStreamActive(sessionId)) {
      return;
    }

    runtime.set({
      stateView: applySessionSnapshotToStores(session),
      generatedProfiles: session.generatedProfiles,
      isLoading: false,
      error: session.phase === 'failed' ? '故事推进失败，请稍后重试。' : null,
    });
  } catch (error) {
    if (!isSessionStreamActive(sessionId)) {
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
  connectSessionStream(sessionId, {
    onTaskUpdated: (task, boundRound) => {
      applyStreamTaskToStores(runtime, task, boundRound);
    },
    onSnapshotSyncRequested: (nextSessionId) => {
      void syncActiveSessionSnapshot(runtime, nextSessionId);
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
