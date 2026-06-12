import type { EngineEvent, PlayerActionItem, RuntimeStateView } from '../../lib/api';
import {
  createRoundState,
  type GameInternalState,
  type RoundState,
} from '../gameStore';
import { summarizeFatePlanning } from './fatePlanningSummary';
import { parseJsonValue } from './jsonValue';
import {
  characterActionChoices,
  characterActionText,
} from './characterChoices';
import { streamEntityLabel } from './taskContent';

interface StreamEventUIState {
  stateView: RuntimeStateView | null;
  isLoading: boolean;
  skipRestoredNarrationAnimation: boolean;
}

export interface StreamEventReductionInput {
  internalState: GameInternalState;
  uiState: StreamEventUIState;
  event: EngineEvent;
}

export interface StreamEventReduction {
  internalStatePatch: Partial<GameInternalState> | null;
  uiStatePatch: Partial<StreamEventUIState> | null;
  shouldSyncSnapshot: boolean;
}

function areChoicesEqual(left: RoundState['choices'], right: RoundState['choices']): boolean {
  if (left.length !== right.length) {
    return false;
  }

  return left.every((choice, index) => {
    const nextChoice = right[index];
    return choice.id === nextChoice.id
      && choice.text === nextChoice.text
      && choice.action === nextChoice.action
      && choice.motivationAndRisk === nextChoice.motivationAndRisk
      && choice.visited === nextChoice.visited
      && choice.disabled === nextChoice.disabled;
  });
}

function roundFromEvent(event: EngineEvent, fallback: number): number {
  return 'round' in event ? Math.max(event.round, 1) : Math.max(fallback, 1);
}

export function reduceStreamEvent({
  internalState,
  uiState,
  event,
}: StreamEventReductionInput): StreamEventReduction {
  const activeRound = roundFromEvent(
    event,
    internalState.displayRound || uiState.stateView?.activeTurnId || 1,
  );
  const isLoading = event.type === 'task_update'
    || (event.type === 'flow_turn_update'
      && (event.stage === 'simulation' || event.stage === 'application'));
  let internalStatePatch: Partial<GameInternalState> | null = null;
  let uiStatePatch: Partial<StreamEventUIState> | null = null;
  let shouldSyncSnapshot = false;

  if (event.type === 'task_update' && event.entity_name === 'UpperNarrator') {
    const reduction = reduceNarrationText(
      internalState,
      uiState.stateView,
      activeRound,
      event.chunk,
      'running',
      'append',
    );
    internalStatePatch = reduction.internalStatePatch;
    uiStatePatch = reduction.uiStatePatch;
  }

  if (event.type === 'flow_turn_update') {
    const content = typeof event.content === 'string' ? event.content : null;

    shouldSyncSnapshot = true;

    if (event.stage === 'simulation' && event.output_type === 'json' && content) {
      const reduction = reduceWorldSnapshotEvent(
        internalState,
        uiState.stateView,
        activeRound,
        event.entity_name,
        content,
      );
      internalStatePatch = reduction.internalStatePatch;
      uiStatePatch = reduction.uiStatePatch;
    }

    if (event.stage === 'application' && event.output_type === 'text' && content) {
      const reduction = reduceNarrationText(
        internalState,
        uiState.stateView,
        activeRound,
        content,
        'done',
        'complete',
      );
      internalStatePatch = reduction.internalStatePatch;
      uiStatePatch = reduction.uiStatePatch;
    }

    if (event.stage === 'application' && event.output_type === 'json' && content) {
      const reduction = reduceCharacterOptions(
        internalState,
        uiState.stateView,
        activeRound,
        content,
      );
      internalStatePatch = reduction.internalStatePatch;
      uiStatePatch = reduction.uiStatePatch;
    }
  }

  if (event.type === 'player_input') {
    const reduction = reducePlayerInput(
      internalState,
      activeRound,
      event.actions[0],
    );
    internalStatePatch = reduction.internalStatePatch;
  }

  if (event.type === 'flow_turn_error') {
    const reduction = reduceNarrationText(
      internalState,
      uiState.stateView,
      activeRound,
      event.msg,
      'error',
      'complete',
    );
    internalStatePatch = reduction.internalStatePatch;
    uiStatePatch = reduction.uiStatePatch;
    shouldSyncSnapshot = true;
  }

  if (event.type === 'flow_turn_completed' || event.type === 'flow_turn_end') {
    if (event.type === 'flow_turn_end' && uiState.stateView) {
      uiStatePatch = {
        ...(uiStatePatch ?? {}),
        stateView: {
          ...uiState.stateView,
          phase: 'ended',
          flowEnd: true,
          activeTurnId: event.round,
          turnIndex: Math.max(uiState.stateView.turnIndex, event.round),
        },
      };
    }
    shouldSyncSnapshot = true;
  }

  if (uiState.isLoading !== isLoading) {
    uiStatePatch = {
      ...(uiStatePatch ?? {}),
      isLoading,
    };
  }

  if (uiState.skipRestoredNarrationAnimation && internalStatePatch) {
    uiStatePatch = {
      ...(uiStatePatch ?? {}),
      skipRestoredNarrationAnimation: false,
    };
  }

  return {
    internalStatePatch,
    uiStatePatch,
    shouldSyncSnapshot,
  };
}

function reducePlayerInput(
  internalState: GameInternalState,
  round: number,
  action: PlayerActionItem | undefined,
): Pick<StreamEventReduction, 'internalStatePatch'> {
  if (!action) {
    return { internalStatePatch: null };
  }
  const previousRoundState = internalState.roundStates[round];
  const actionText = action.action;
  const selectedChoiceText =
    previousRoundState?.selectedChoiceText
    ?? previousRoundState?.choices.find((choice) => choice.action === actionText)?.text
    ?? action.title
    ?? actionText;
  const choices = (previousRoundState?.choices ?? []).map((choice) => (
    choice.action === actionText ? { ...choice, visited: true } : choice
  ));
  const nextRoundState = createRoundState(round, {
    ...(previousRoundState ?? {}),
    round,
    selectedChoiceText,
    selectedChoiceAction: actionText,
    choices,
    choicesStatus: previousRoundState?.choicesStatus ?? 'idle',
    isAwaitingNarration: false,
  });

  return {
    internalStatePatch: {
      roundStates: {
        ...internalState.roundStates,
        [round]: nextRoundState,
      },
    },
  };
}

function reduceNarrationText(
  internalState: GameInternalState,
  stateView: RuntimeStateView | null,
  round: number,
  text: string,
  narrationStatus: RoundState['narrationStatus'],
  mode: 'append' | 'complete',
): Pick<StreamEventReduction, 'internalStatePatch' | 'uiStatePatch'> {
  const previousRoundState = internalState.roundStates[round];
  const narrationText = mergeNarrationText(
    previousRoundState?.narrationText ?? '',
    text,
    mode,
  );
  const nextRoundState = createRoundState(round, {
    ...(previousRoundState ?? {}),
    round,
    title: previousRoundState?.title || '记录回响',
    narrationText,
    narrationStatus,
    isAwaitingNarration: false,
  });
  const nextTurnIndex = Math.max(internalState.turnIndex, round);
  const internalStatePatch = (
    internalState.turnIndex === nextTurnIndex
    && internalState.displayRound === round
    && previousRoundState?.narrationText === nextRoundState.narrationText
    && previousRoundState?.narrationStatus === nextRoundState.narrationStatus
    && previousRoundState?.isAwaitingNarration === nextRoundState.isAwaitingNarration
  )
    ? null
    : {
        turnIndex: nextTurnIndex,
        displayRound: round,
        roundStates: {
          ...internalState.roundStates,
          [round]: nextRoundState,
        },
      };

  const uiStatePatch = stateView && stateView.latestHistory !== narrationText
    ? {
        stateView: {
          ...stateView,
          phase: narrationStatus === 'running' ? stateView.phase : 'application',
          latestHistory: narrationText,
        },
      }
    : null;

  return { internalStatePatch, uiStatePatch };
}

function mergeNarrationText(
  currentText: string,
  incomingText: string,
  mode: 'append' | 'complete',
): string {
  if (!currentText || mode === 'complete') {
    if (mode === 'complete' && currentText && incomingText.startsWith(currentText)) {
      return incomingText;
    }
    if (mode === 'complete' && currentText && currentText.startsWith(incomingText)) {
      return currentText;
    }
    return mode === 'complete' ? incomingText : incomingText;
  }

  if (!incomingText) {
    return currentText;
  }
  if (incomingText.startsWith(currentText)) {
    return incomingText;
  }
  if (currentText.endsWith(incomingText)) {
    return currentText;
  }
  return `${currentText}${incomingText}`;
}

function reduceWorldSnapshotEvent(
  internalState: GameInternalState,
  stateView: RuntimeStateView | null,
  round: number,
  entityName: string,
  content: string,
): Pick<StreamEventReduction, 'internalStatePatch' | 'uiStatePatch'> {
  const parsed = parseJsonValue(content);
  const summary = summarizeFatePlanning(parsed);
  const nextRound = Math.max(summary?.round ?? round, 1);
  const previousRoundState = internalState.roundStates[nextRound];
  const nextTurnIndex = Math.max(internalState.turnIndex, nextRound);
  const nextDisplayRound = internalState.displayRound || nextRound;
  const nextTitle = summary?.sceneTitle ?? previousRoundState?.title ?? '记录回响';
  let internalStatePatch: Partial<GameInternalState> | null = null;
  let uiStatePatch: Partial<StreamEventUIState> | null = null;

  if (
    internalState.turnIndex !== nextTurnIndex
    || internalState.displayRound !== nextDisplayRound
    || !previousRoundState
    || previousRoundState.title !== nextTitle
  ) {
    internalStatePatch = {
      turnIndex: nextTurnIndex,
      displayRound: nextDisplayRound,
      roundStates: {
        ...internalState.roundStates,
        [nextRound]: createRoundState(nextRound, {
          ...(previousRoundState ?? {}),
          title: nextTitle,
          isAwaitingNarration: previousRoundState?.isAwaitingNarration ?? true,
        }),
      },
    };
  }

  if (stateView) {
    const nextBroadcastItems = summary?.newInfo.length
      ? summary.newInfo
      : stateView.latestBroadcastItems;
    uiStatePatch = {
      stateView: {
        ...stateView,
        phase: 'simulation',
        turnIndex: nextRound,
        activeTurnId: nextRound,
        currentScene: summary?.sceneTitle ?? streamEntityLabel(entityName),
        currentLocation: summary?.locationName ?? stateView.currentLocation,
        characterState: summary?.characterCondition ?? stateView.characterState,
        latestBroadcastSummary:
          summary?.currentEvent
          ?? summary?.newInfo[0]
          ?? summary?.locationStatus
          ?? summary?.description
          ?? stateView.latestBroadcastSummary,
        latestBroadcastItems: nextBroadcastItems,
        isEnding: summary?.isEnding ?? stateView.isEnding,
        endingType: summary?.endingType ?? stateView.endingType,
      },
    };
  }

  return { internalStatePatch, uiStatePatch };
}

function reduceCharacterOptions(
  internalState: GameInternalState,
  stateView: RuntimeStateView | null,
  round: number,
  content: string,
): Pick<StreamEventReduction, 'internalStatePatch' | 'uiStatePatch'> {
  const nextChoices = characterActionChoices(content);
  const previousRoundState = internalState.roundStates[round];
  const normalizedChoices = nextChoices ?? [];
  let internalStatePatch: Partial<GameInternalState> | null = null;
  let uiStatePatch: Partial<StreamEventUIState> | null = null;

  if (
    !previousRoundState
    || previousRoundState.choicesStatus !== 'ready'
    || previousRoundState.isAwaitingNarration
    || !areChoicesEqual(previousRoundState.choices, normalizedChoices)
  ) {
    internalStatePatch = {
      roundStates: {
        ...internalState.roundStates,
        [round]: createRoundState(round, {
          ...(previousRoundState ?? {}),
          round,
          choices: normalizedChoices,
          choicesStatus: nextChoices ? 'ready' : 'loading',
          isAwaitingNarration: false,
        }),
      },
    };
  }

  const nextCharacterAction = characterActionText(content);
  if (stateView) {
    uiStatePatch = {
      stateView: {
        ...stateView,
        phase: nextChoices ? 'awaiting_player' : stateView.phase,
        latestCharacterAction:
          nextCharacterAction ?? stateView.latestCharacterAction,
      },
    };
  }

  return { internalStatePatch, uiStatePatch };
}
