import type { StoreApi } from 'zustand';
import type { GeneratedProfiles } from '../../lib/api';
import { useGameInternalStore } from '../gameStore';
import {
  currentStartupFlowRunId,
  isStartupFlowCurrent,
  startStartupFlow,
  waitForNextPaint,
} from './lifecycle';
import {
  beginStartupProfileGeneration,
  failStartupProfileGeneration,
  generateStartupProfilesForRun,
  markStartupProfilesReady,
  scheduleStartupProfileStageProgress,
} from './profileRuntime';
import {
  activateStartupGameSession,
  consumeReadyStartupSession,
  createStartupGameSession,
  failStartupGameSession,
  markStartupOpeningReady,
  requestStartupOpeningNarration,
} from './sessionRuntime';
import type { GameUIStoreState } from '../gameUIStore';

export interface StartupFlowRuntime {
  set: StoreApi<GameUIStoreState>['setState'];
  get: StoreApi<GameUIStoreState>['getState'];
  closeStoryNodeStream: () => void;
  materializeStoryNode: (sessionId: string, nodeId?: string) => void;
  enterWorld: () => Promise<{ sessionId: string } | null>;
}

function startupProfileRuntime(runtime: StartupFlowRuntime) {
  return {
    set: runtime.set,
    get: runtime.get,
    closeStoryNodeStream: runtime.closeStoryNodeStream,
  };
}

function startupSessionRuntime(runtime: StartupFlowRuntime) {
  return {
    set: runtime.set,
    closeStoryNodeStream: runtime.closeStoryNodeStream,
    materializeStoryNode: runtime.materializeStoryNode,
  };
}

export async function startGameFlow(runtime: StartupFlowRuntime): Promise<void> {
  const runId = startStartupFlow();
  const { character, world } = runtime.get();
  const profileRuntime = startupProfileRuntime(runtime);
  beginStartupProfileGeneration(profileRuntime);
  await waitForNextPaint();
  scheduleStartupProfileStageProgress(profileRuntime);

  let generatedProfiles: GeneratedProfiles | null;
  try {
    generatedProfiles = await generateStartupProfilesForRun(runId, character, world);
    if (!generatedProfiles) {
      return;
    }
  } catch (error) {
    if (!isStartupFlowCurrent(runId)) {
      return;
    }
    failStartupProfileGeneration(profileRuntime, error);
    throw error;
  }

  await markStartupProfilesReady(profileRuntime, generatedProfiles);
  if (!isStartupFlowCurrent(runId)) {
    return;
  }
  await runtime.enterWorld();
}

export async function enterWorldFlow(
  runtime: StartupFlowRuntime,
): Promise<{ sessionId: string } | null> {
  const runId = currentStartupFlowRunId();
  const { character, world, preparedProfiles, startupStage, stateView } = runtime.get();
  const { sessionId } = useGameInternalStore.getState();
  const sessionRuntime = startupSessionRuntime(runtime);

  if (startupStage === 'ready_to_enter' && sessionId && stateView) {
    if (!isStartupFlowCurrent(runId)) {
      return null;
    }
    return consumeReadyStartupSession(sessionRuntime, sessionId);
  }

  if (!preparedProfiles) {
    throw new Error('记录还在共鸣中，请稍后再进入。');
  }

  runtime.set({
    error: null,
    isLoading: true,
  });
  await waitForNextPaint();

  try {
    const created = await createStartupGameSession(preparedProfiles, character.name);
    if (!isStartupFlowCurrent(runId)) {
      return null;
    }

    activateStartupGameSession(sessionRuntime, created.sessionId, character, world);
    await requestStartupOpeningNarration(sessionRuntime, created.sessionId);
    if (!isStartupFlowCurrent(runId)) {
      return null;
    }
    markStartupOpeningReady(sessionRuntime);
    return null;
  } catch (error) {
    if (!isStartupFlowCurrent(runId)) {
      return null;
    }
    failStartupGameSession(sessionRuntime, error);
    throw error;
  }
}
