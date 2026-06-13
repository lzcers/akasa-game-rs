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
  isStoryNodeStreamActiveForSession,
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
  closeStoryNodeStream: () => void;
  materializeStoryNode: (sessionId: string, nodeId?: string) => void;
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

  beginSessionSwitch(runtime.set, runtime.closeStoryNodeStream);

  try {
    const archive = readStoredSaveArchive(slotId);
    if (!archive) {
      throw new Error('没有找到这份记录，请确认它仍然存在。');
    }

    const loaded = await loadGameSessionFromArchive({
      compressedArchive: archive,
    });
    activateSessionSnapshot(runtime.set, loaded, runtime.materializeStoryNode);
    await loadCompleteSessionRounds(loaded.sessionId);
    return {
      sessionId: loaded.sessionId,
    };
  } catch (error) {
    failSessionSwitch(runtime.set, runtime.closeStoryNodeStream, error, '读取记录失败。');
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
    if (!isStoryNodeStreamActiveForSession(targetSessionId)) {
      runtime.closeStoryNodeStream();
      runtime.materializeStoryNode(targetSessionId);
    }
    return;
  }

  if (restoringSessionId === targetSessionId) {
    return;
  }

  beginSessionSwitch(runtime.set, runtime.closeStoryNodeStream, { invalidateStartup: true });
  restoringSessionId = targetSessionId;

  try {
    const loaded = await getGameSession(targetSessionId);
    if (restoringSessionId !== targetSessionId) {
      return;
    }
    activateSessionSnapshot(runtime.set, loaded, runtime.materializeStoryNode);
    await loadCompleteSessionRounds(loaded.sessionId);
    restoringSessionId = null;
  } catch (error) {
    if (restoringSessionId !== targetSessionId) {
      return;
    }

    failSessionSwitch(runtime.set, runtime.closeStoryNodeStream, error, '这段记录已经暂时无法续上。');
    throw error;
  }
}

export async function cloneSharedGameSession(
  runtime: SessionRestoreRuntime,
  sourceSessionId: string,
  sourceNodeId: string | null = null,
): Promise<{ sessionId: string; isEnding: boolean }> {
  const targetSessionId = sourceSessionId.trim();
  const targetNodeId = sourceNodeId?.trim() || null;
  if (!targetSessionId) {
    throw new Error('未找到要复制的记录编号。');
  }

  const activeRequest = getActiveCloneRequest(targetSessionId, targetNodeId);
  if (activeRequest) {
    return activeRequest;
  }

  const clonePromise = (async () => {
    beginSessionSwitch(runtime.set, runtime.closeStoryNodeStream, { invalidateStartup: true });
    restoringSessionId = null;

    try {
      const cloned = await cloneGameSession(targetSessionId, targetNodeId);
      track('share_clone_session_created', {
        sourceSessionId: targetSessionId,
        clonedSessionId: cloned.sessionId,
        sourceSessionIdFromAttribution: getAnalyticsSourceSessionId(),
        sourceNodeId: targetNodeId,
        sourceEndingType: cloned.worldState.endingType ?? null,
        isEnding: cloned.worldState.isEnding,
      });

      activateSessionSnapshot(runtime.set, cloned, runtime.materializeStoryNode);
      await loadCompleteSessionRounds(cloned.sessionId);
      return {
        sessionId: cloned.sessionId,
        isEnding: cloned.worldState.isEnding,
      };
    } catch (error) {
      failSessionSwitch(runtime.set, runtime.closeStoryNodeStream, error, '这段记录暂时无法复制。');
      throw error;
    }
  })();

  return trackCloneRequest(targetSessionId, targetNodeId, clonePromise);
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
  runtime.closeStoryNodeStream();

  try {
    const selected = await selectGameSessionStorylineNode(targetSessionId, {
      nodeId: targetNodeId,
    });
    activateSessionSnapshot(runtime.set, selected, runtime.materializeStoryNode, {
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
      runtime.materializeStoryNode(targetSessionId);
    }
    runtime.set({
      isLoading: false,
      error: error instanceof Error ? error.message : '切换故事线失败。',
    });
    throw error;
  }
}
