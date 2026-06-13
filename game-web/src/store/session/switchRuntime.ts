import type { StoreApi } from 'zustand';
import type { GameSessionWorldStateData } from '../../lib/api';
import { appRoutes } from '../../lib/appRoutes';
import { navigateTo } from '../../lib/navigation';
import { setAnalyticsGameSessionId } from '../../lib/analytics';
import { initialInternalState, useGameInternalStore } from '../gameStore';
import type { GameUIStoreState } from '../gameUIStore';
import { applySessionSnapshotToStores } from './stateSync';
import {
  clearStartupStageTimer,
  invalidateStartupFlow,
} from '../startup/lifecycle';
import { resetUIState } from '../ui/initialState';

type SetGameUIState = StoreApi<GameUIStoreState>['setState'];
type CloseStoryNodeStream = () => void;
type MaterializeStoryNode = (sessionId: string, nodeId?: string) => void;

interface ActivateSessionSnapshotOptions {
  replaceTimeline?: boolean;
}

export function resetInternalGameState() {
  useGameInternalStore.setState({
    ...initialInternalState,
  });
}

function setSessionSwitchLoadingState(set: SetGameUIState) {
  resetInternalGameState();
  set({
    stateView: null,
    isLoading: true,
    startupStage: 'idle',
    preparedProfiles: null,
    generatedProfiles: null,
    error: null,
    skipRestoredNarrationAnimation: true,
  });
}

export function beginSessionSwitch(
  set: SetGameUIState,
  closeStoryNodeStream: CloseStoryNodeStream,
  options: { invalidateStartup?: boolean } = {},
) {
  closeStoryNodeStream();
  clearStartupStageTimer();
  if (options.invalidateStartup) {
    invalidateStartupFlow();
  }
  setSessionSwitchLoadingState(set);
}

export function activateSessionSnapshot(
  set: SetGameUIState,
  session: GameSessionWorldStateData,
  materializeStoryNode: MaterializeStoryNode,
  options: ActivateSessionSnapshotOptions = {},
) {
  setAnalyticsGameSessionId(session.sessionId);
  const stateView = applySessionSnapshotToStores(session, {
    resetValues: true,
    replaceTimeline: options.replaceTimeline,
  });
  set({
    stateView,
    isLoading: false,
    startupStage: 'idle',
    preparedProfiles: null,
    generatedProfiles: session.generatedProfiles,
    error: null,
    skipRestoredNarrationAnimation: true,
  });
  materializeStoryNode(session.sessionId);
}

export function failSessionSwitch(
  set: SetGameUIState,
  closeStoryNodeStream: CloseStoryNodeStream,
  error: unknown,
  fallbackMessage: string,
) {
  closeStoryNodeStream();
  resetInternalGameState();
  set({
    ...resetUIState(),
    error: error instanceof Error ? error.message : fallbackMessage,
  });
  navigateTo(appRoutes.lobby, { replace: true });
}
