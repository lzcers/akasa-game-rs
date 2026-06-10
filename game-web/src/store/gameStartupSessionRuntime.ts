import type { StoreApi } from 'zustand';
import {
  createGameSession,
  submitGameSessionControl,
} from '../lib/api';
import type {
  Character,
  CreateGameSessionData,
  GeneratedProfiles,
  World,
} from '../lib/api';
import { setAnalyticsGameSessionId } from '../lib/analytics';
import { useGameInternalStore } from './gameStore';
import { useGameValueStore } from './gameValueStore';
import {
  createOpeningInternalState,
  createOpeningStateView,
  waitForRoundNarrationStarted,
} from './gameOpeningSession';
import { sleep } from './gameStartupRuntime';
import type { GameUIStoreState } from './gameUIStore';

const MIN_CREATING_SESSION_STAGE_MS = 450;

interface StartupSessionRuntime {
  set: StoreApi<GameUIStoreState>['setState'];
  connectSessionStream: (sessionId: string) => void;
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
