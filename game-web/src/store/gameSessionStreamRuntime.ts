import type { TaskView } from '../lib/api';
import { openGameSessionStream } from '../lib/api';
import { applyTaskUpdate } from './gameStoreHelpers';

interface SessionStreamHandlers {
  onTaskUpdated: (task: TaskView, boundRound?: number | null) => void;
  onStreamError: () => void;
  onSnapshotSyncRequested: (sessionId: string) => void;
}

let activeSessionStream: EventSource | null = null;
let activeStreamSessionId: string | null = null;
let lastStreamEventId: string | null = null;
let activeStreamTasks = new Map<string, TaskView>();
let activeStreamTaskRounds = new Map<string, number>();
let endingSnapshotSyncTimer: number | null = null;

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
      onTaskUpdated: (event, lastEventId) => {
        if (!isSessionStreamActive(sessionId)) {
          return;
        }
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
