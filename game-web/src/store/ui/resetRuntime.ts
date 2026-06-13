import type { StoreApi } from 'zustand';
import { setAnalyticsGameSessionId } from '../../lib/analytics';
import {
  initialInternalState,
  useGameInternalStore,
} from '../gameStore';
import { useGameValueStore } from '../gameValueStore';
import { clearStartupStageTimer } from '../startup/lifecycle';
import {
  resetUIState,
} from './initialState';
import type { GameUIStoreState } from '../gameUIStore';

export function resetGameRuntime(
  set: StoreApi<GameUIStoreState>['setState'],
  closeStoryNodeStream: () => void,
) {
  closeStoryNodeStream();
  clearStartupStageTimer();
  setAnalyticsGameSessionId(null);
  useGameInternalStore.setState({
    ...initialInternalState,
  });
  useGameValueStore.getState().resetValues();
  set((state) => ({
    ...state,
    ...resetUIState(),
  }));
}
