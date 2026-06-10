import type { StoreApi } from 'zustand';
import { generateProfiles } from '../../lib/api';
import type {
  Character,
  GeneratedProfiles,
  World,
} from '../../lib/api';
import { track } from '../../lib/analytics';
import { appRoutes } from '../../lib/appRoutes';
import { navigateTo } from '../../lib/navigation';
import {
  initialInternalState,
  useGameInternalStore,
} from '../gameStore';
import { useGameValueStore } from '../gameValueStore';
import {
  clearStartupStageTimer,
  isStartupFlowCurrent,
  scheduleStartupStageProgress,
  sleep,
  waitForNextPaint,
} from './lifecycle';
import type { GameUIStoreState } from '../gameUIStore';

const MIN_GENERATING_PAGE_MS = 1200;

export interface StartupProfileRuntime {
  set: StoreApi<GameUIStoreState>['setState'];
  get: StoreApi<GameUIStoreState>['getState'];
  closeSessionStream: () => void;
}

export function beginStartupProfileGeneration(runtime: StartupProfileRuntime) {
  runtime.closeSessionStream();
  clearStartupStageTimer();
  useGameInternalStore.setState({
    ...initialInternalState,
  });
  useGameValueStore.getState().resetValues();
  runtime.set({
    error: null,
    isLoading: true,
    startupStage: 'generating_world',
    preparedProfiles: null,
    stateView: null,
  });
  navigateTo(appRoutes.generating, { replace: true });
}

export function scheduleStartupProfileStageProgress(runtime: StartupProfileRuntime) {
  scheduleStartupStageProgress(() => {
    if (runtime.get().startupStage === 'generating_world') {
      runtime.set({
        startupStage: 'generating_protagonist',
      });
    }
  });
}

export async function generateStartupProfilesForRun(
  runId: number,
  character: Character,
  world: World,
): Promise<GeneratedProfiles | null> {
  const generatingStartedAt = Date.now();
  const generatedProfiles = await generateProfiles(character, world);
  const generatingElapsed = Date.now() - generatingStartedAt;
  track('profile_generate_completed');
  if (generatingElapsed < MIN_GENERATING_PAGE_MS) {
    await sleep(MIN_GENERATING_PAGE_MS - generatingElapsed);
  }
  if (!isStartupFlowCurrent(runId)) {
    return null;
  }
  return generatedProfiles;
}

export function failStartupProfileGeneration(
  runtime: StartupProfileRuntime,
  error: unknown,
) {
  clearStartupStageTimer();
  runtime.closeSessionStream();
  useGameInternalStore.setState({
    ...initialInternalState,
  });
  runtime.set({
    stateView: null,
    isLoading: false,
    startupStage: 'idle',
    error: error instanceof Error ? error.message : '开启回响失败。',
  });
  navigateTo(appRoutes.creation, { replace: true });
}

export async function markStartupProfilesReady(
  runtime: StartupProfileRuntime,
  generatedProfiles: GeneratedProfiles,
) {
  clearStartupStageTimer();
  runtime.set({
    startupStage: 'creating_session',
    preparedProfiles: generatedProfiles,
  });
  await waitForNextPaint();
}
