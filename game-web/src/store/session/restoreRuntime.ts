import type { StoreApi } from 'zustand';
import {
  cloneGameSession,
  getGameSession,
  loadGameSessionFromArchive,
  selectGameSessionStorylineNode,
} from '../../lib/api';
import {
  getAnalyticsSourceSessionId,
  track,
} from '../../lib/analytics';
import { readStoredSaveArchive } from '../../lib/saveSlots';
import { useGameInternalStore } from '../gameStore';
import {
  getActiveCloneRequest,
  trackCloneRequest,
} from './cloneRequestRuntime';
import {
  isSessionStreamActive,
} from './streamRuntime';
import { loadCompleteSessionRounds } from './roundHistoryRuntime';
import {
  activateSessionSnapshot,
  beginSessionSwitch,
  failSessionSwitch,
} from './switchRuntime';
import type { GameUIStoreState } from '../gameUIStore';

export interface SessionRestoreRuntime {
  set: StoreApi<GameUIStoreState>['setState'];
  get: StoreApi<GameUIStoreState>['getState'];
  closeSessionStream: () => void;
  connectSessionStream: (sessionId: string) => void;
}

let restoringSessionId: string | null = null;

export function clearSessionRestoreState() {
  restoringSessionId = null;
}

export async function loadStoredGameSave(
  runtime: SessionRestoreRuntime,
  saveId: string,
): Promise<{ sessionId: string }> {
  const slotId = saveId.trim();
  if (!slotId) {
    throw new Error('未找到要读取的记录。');
  }

  beginSessionSwitch(runtime.set, runtime.closeSessionStream);

  try {
    const archive = readStoredSaveArchive(slotId);
    if (!archive) {
      throw new Error('没有找到这份记录，请确认它仍然存在。');
    }

    const loaded = await loadGameSessionFromArchive({
      compressedArchive: archive,
    });
    activateSessionSnapshot(runtime.set, loaded, runtime.connectSessionStream);
    await loadCompleteSessionRounds(loaded.sessionId);
    return {
      sessionId: loaded.sessionId,
    };
  } catch (error) {
    failSessionSwitch(runtime.set, runtime.closeSessionStream, error, '读取记录失败。');
    throw error;
  }
}

export async function restoreExistingGameSession(
  runtime: SessionRestoreRuntime,
  sessionId: string,
): Promise<void> {
  const targetSessionId = sessionId.trim();
  if (!targetSessionId) {
    throw new Error('未找到要恢复的记录编号。');
  }

  const currentSessionId = useGameInternalStore.getState().sessionId;
  if (currentSessionId === targetSessionId && runtime.get().stateView) {
    if (!isSessionStreamActive(targetSessionId)) {
      runtime.closeSessionStream();
      runtime.connectSessionStream(targetSessionId);
    }
    return;
  }

  if (restoringSessionId === targetSessionId) {
    return;
  }

  beginSessionSwitch(runtime.set, runtime.closeSessionStream, { invalidateStartup: true });
  restoringSessionId = targetSessionId;

  try {
    const loaded = await getGameSession(targetSessionId);
    if (restoringSessionId !== targetSessionId) {
      return;
    }
    activateSessionSnapshot(runtime.set, loaded, runtime.connectSessionStream);
    await loadCompleteSessionRounds(loaded.sessionId);
    restoringSessionId = null;
  } catch (error) {
    if (restoringSessionId !== targetSessionId) {
      return;
    }

    failSessionSwitch(runtime.set, runtime.closeSessionStream, error, '这段记录已经暂时无法续上。');
    throw error;
  }
}

export async function cloneSharedGameSession(
  runtime: SessionRestoreRuntime,
  sourceSessionId: string,
  sourceRound: number | null = null,
): Promise<{ sessionId: string; isEnding: boolean }> {
  const targetSessionId = sourceSessionId.trim();
  if (!targetSessionId) {
    throw new Error('未找到要复制的记录编号。');
  }

  const activeRequest = getActiveCloneRequest(targetSessionId, sourceRound);
  if (activeRequest) {
    return activeRequest;
  }

  const clonePromise = (async () => {
    beginSessionSwitch(runtime.set, runtime.closeSessionStream, { invalidateStartup: true });
    restoringSessionId = null;

    try {
      const cloned = await cloneGameSession(targetSessionId, sourceRound);
      track('share_clone_session_created', {
        sourceSessionId: targetSessionId,
        clonedSessionId: cloned.sessionId,
        sourceSessionIdFromAttribution: getAnalyticsSourceSessionId(),
        sourceRound: cloned.worldState.round,
        sourceEndingType: cloned.worldState.endingType ?? null,
        isEnding: cloned.worldState.isEnding,
      });

      activateSessionSnapshot(runtime.set, cloned, runtime.connectSessionStream);
      await loadCompleteSessionRounds(cloned.sessionId);
      return {
        sessionId: cloned.sessionId,
        isEnding: cloned.worldState.isEnding,
      };
    } catch (error) {
      failSessionSwitch(runtime.set, runtime.closeSessionStream, error, '这段记录暂时无法复制。');
      throw error;
    }
  })();

  return trackCloneRequest(targetSessionId, sourceRound, clonePromise);
}

export async function selectStorylineNodeForSession(
  runtime: SessionRestoreRuntime,
  sessionId: string,
  nodeId: string,
): Promise<{ sessionId: string; isEnding: boolean }> {
  const targetSessionId = sessionId.trim();
  const targetNodeId = nodeId.trim();
  if (!targetSessionId || !targetNodeId) {
    throw new Error('未找到要切换的故事节点。');
  }

  runtime.set({
    isLoading: true,
    error: null,
  });
  runtime.closeSessionStream();

  try {
    const selected = await selectGameSessionStorylineNode(targetSessionId, {
      nodeId: targetNodeId,
    });
    activateSessionSnapshot(runtime.set, selected, runtime.connectSessionStream, {
      replaceTimeline: true,
    });
    await loadCompleteSessionRounds(selected.sessionId, {
      replaceTimeline: true,
    });
    return {
      sessionId: selected.sessionId,
      isEnding: selected.worldState.isEnding || selected.flowEnd,
    };
  } catch (error) {
    if (useGameInternalStore.getState().sessionId === targetSessionId) {
      runtime.connectSessionStream(targetSessionId);
    }
    runtime.set({
      isLoading: false,
      error: error instanceof Error ? error.message : '切换故事线失败。',
    });
    throw error;
  }
}
