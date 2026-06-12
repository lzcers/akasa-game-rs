import type { GameSessionWorldStateData, RuntimeStateView } from '../../lib/api';
import {
  type GameInternalState,
  type RoundState,
  useGameInternalStore,
} from '../gameStore';
import { useGameValueStore } from '../gameValueStore';
import {
  effectiveDisplayRound,
  internalStateFromSession,
  stateViewFromSession,
} from './mappers';

interface ApplySessionSnapshotOptions {
  resetValues?: boolean;
  replaceTimeline?: boolean;
}

function mergeRoundSnapshot(
  currentRoundState: RoundState | undefined,
  nextRoundState: RoundState,
): RoundState {
  if (!currentRoundState?.selectedChoiceText || nextRoundState.selectedChoiceText) {
    return nextRoundState;
  }

  return {
    ...nextRoundState,
    choices: currentRoundState.choices,
    choicesStatus: currentRoundState.choicesStatus,
    selectedChoiceText: currentRoundState.selectedChoiceText,
    selectedChoiceAction: currentRoundState.selectedChoiceAction,
  };
}

function mergeInternalSessionState(
  currentInternalState: GameInternalState,
  nextInternalState: GameInternalState,
): GameInternalState {
  const roundStates = { ...currentInternalState.roundStates };
  for (const [roundKey, nextRoundState] of Object.entries(nextInternalState.roundStates)) {
    const round = Number(roundKey);
    roundStates[round] = mergeRoundSnapshot(roundStates[round], nextRoundState);
  }

  return {
    ...nextInternalState,
    turnIndex: Math.max(currentInternalState.turnIndex, nextInternalState.turnIndex),
    displayRound: Math.max(
      currentInternalState.displayRound,
      nextInternalState.displayRound,
    ),
    roundStates,
  };
}

export function applySessionSnapshotToStores(
  session: GameSessionWorldStateData,
  options: ApplySessionSnapshotOptions = {},
): RuntimeStateView {
  const stateView = stateViewFromSession(session);
  const nextInternalState = internalStateFromSession(session);
  const currentInternalState = useGameInternalStore.getState();
  const shouldMergeRounds =
    currentInternalState.sessionId === session.sessionId &&
    !options.replaceTimeline;
  useGameInternalStore.setState(
    shouldMergeRounds
      ? mergeInternalSessionState(currentInternalState, nextInternalState)
      : nextInternalState,
  );

  if (options.resetValues) {
    useGameValueStore.getState().resetValues(effectiveDisplayRound(session));
  }

  return stateView;
}
