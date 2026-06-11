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
  const nextInternalState = internalStateFromSession(session);
  const currentInternalState = useGameInternalStore.getState();
  const shouldMergeRounds = currentInternalState.sessionId === session.sessionId;
  useGameInternalStore.setState({
    ...nextInternalState,
    roundStates: shouldMergeRounds
      ? {
          ...currentInternalState.roundStates,
          ...nextInternalState.roundStates,
        }
      : nextInternalState.roundStates,
  });

  if (options.resetValues) {
    useGameValueStore.getState().resetValues(effectiveDisplayRound(session));
  }

  return stateView;
}
