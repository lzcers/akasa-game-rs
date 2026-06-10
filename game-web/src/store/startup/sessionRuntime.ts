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
  closeSessionStream: () => void;
  connectSessionStream: (sessionId: string) => void;
}

function errorAnalyticsType(error: unknown) {
  return error instanceof Error ? error.name || 'Error' : typeof error;
}

export async function createStartupGameSession(
  preparedProfiles: GeneratedProfiles,
): Promise<CreateGameSessionData> {
  const [created] = await Promise.all([
    createGameSession({
      worldProfile: preparedProfiles.world,
      protagonistProfile: preparedProfiles.protagonist,
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
  runtime.connectSessionStream(sessionId);
}

export async function requestStartupOpeningNarration(sessionId: string) {
  await submitGameSessionControl(sessionId, {
    control: { type: 'continue' },
  });
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
  runtime.closeSessionStream();
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
