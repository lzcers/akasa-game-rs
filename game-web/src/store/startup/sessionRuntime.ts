import type { StoreApi } from 'zustand';
import {
  createGameSession,
  submitGameSessionControl,
} from '../../lib/api';
import type {
  Character,
  CreateGameSessionData,
  GeneratedProfiles,
  World,
} from '../../lib/api';
import {
  setAnalyticsGameSessionId,
  track,
} from '../../lib/analytics';
import { appRoutes } from '../../lib/appRoutes';
import { navigateTo } from '../../lib/navigation';
import {
  initialInternalState,
  useGameInternalStore,
} from '../gameStore';
import { useGameValueStore } from '../gameValueStore';
import {
  createOpeningInternalState,
  createOpeningStateView,
  waitForRoundNarrationStarted,
} from './openingSession';
import { sleep } from './lifecycle';
import type { GameUIStoreState } from '../gameUIStore';

const MIN_CREATING_SESSION_STAGE_MS = 450;

interface StartupSessionRuntime {
  set: StoreApi<GameUIStoreState>['setState'];
  closeStoryNodeStream: () => void;
  materializeStoryNode: (sessionId: string, nodeId?: string) => void;
}

function errorAnalyticsType(error: unknown) {
  return error instanceof Error ? error.name || 'Error' : typeof error;
}

export async function createStartupGameSession(
  preparedProfiles: GeneratedProfiles,
  characterName: string,
): Promise<CreateGameSessionData> {
  const [created] = await Promise.all([
    createGameSession({
      characterName,
      worldProfile: preparedProfiles.world,
      characterProfile: preparedProfiles.character,
      keyStoryBeats: preparedProfiles.keyStoryBeats,
    }),
    sleep(MIN_CREATING_SESSION_STAGE_MS),
  ]);

  return created;
}

export function activateStartupGameSession(
  runtime: StartupSessionRuntime,
  sessionId: string,
  character: Character,
  world: World,
) {
  setAnalyticsGameSessionId(sessionId);
  useGameInternalStore.setState(createOpeningInternalState(sessionId));
  useGameValueStore.getState().resetValues(1);
  runtime.set({
    stateView: createOpeningStateView(character, world),
    error: null,
    isLoading: true,
  });
}

export async function requestStartupOpeningNarration(
  runtime: StartupSessionRuntime,
  sessionId: string,
) {
  const result = await submitGameSessionControl(sessionId, {
    control: { type: 'continue' },
  });
  runtime.materializeStoryNode(sessionId, result.targetNodeId);
  await waitForRoundNarrationStarted(sessionId, 1);
}

export function consumeReadyStartupSession(
  runtime: StartupSessionRuntime,
  sessionId: string,
): { sessionId: string } {
  runtime.set({
    startupStage: 'idle',
    preparedProfiles: null,
  });
  return { sessionId };
}

export function markStartupOpeningReady(runtime: StartupSessionRuntime) {
  runtime.set((state) => ({
    error: null,
    isLoading: false,
    skipRestoredNarrationAnimation: false,
    startupStage: 'ready_to_enter',
    stateView: state.stateView
      ? {
        ...state.stateView,
        phase: 'opening',
      }
      : state.stateView,
  }));
}

export function failStartupGameSession(
  runtime: StartupSessionRuntime,
  error: unknown,
) {
  track('game_session_create_failed', {
    errorType: errorAnalyticsType(error),
  });
  runtime.closeStoryNodeStream();
  useGameInternalStore.setState({
    ...initialInternalState,
  });
  runtime.set({
    stateView: null,
    isLoading: false,
    startupStage: 'ready_to_enter',
    skipRestoredNarrationAnimation: false,
    error: error instanceof Error ? error.message : '进入回响失败。',
  });
  navigateTo(appRoutes.generating, { replace: true });
}
