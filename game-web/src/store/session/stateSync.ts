import type { GameSessionWorldStateData, RuntimeStateView } from '../../lib/api';
import { useGameInternalStore } from '../gameStore';
import { useGameValueStore } from '../gameValueStore';
import {
  effectiveDisplayRound,
  internalStateFromSession,
  stateViewFromSession,
} from './mappers';

interface ApplySessionSnapshotOptions {
  resetValues?: boolean;
}

export function applySessionSnapshotToStores(
  session: GameSessionWorldStateData,
  options: ApplySessionSnapshotOptions = {},
): RuntimeStateView {
  const stateView = stateViewFromSession(session);
  useGameInternalStore.setState(internalStateFromSession(session));

  if (options.resetValues) {
    useGameValueStore.getState().resetValues(effectiveDisplayRound(session));
  }

  return stateView;
}
