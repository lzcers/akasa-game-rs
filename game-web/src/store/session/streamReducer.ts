import type { RuntimeStateView, TaskView } from '../../lib/api';
import {
  createRoundState,
  type GameInternalState,
  type RoundState,
} from '../gameStore';
import {
  parseJsonValue,
  protagonistActionChoices,
  protagonistActionText,
  summarizeFatePlanning,
  taskLabel,
  taskRawContent,
  taskText,
} from '../gameStoreHelpers';

interface StreamTaskUIState {
  stateView: RuntimeStateView | null;
  isLoading: boolean;
  skipRestoredNarrationAnimation: boolean;
}

export interface StreamTaskReductionInput {
  internalState: GameInternalState;
  uiState: StreamTaskUIState;
  task: TaskView;
  boundRound?: number | null;
}

export interface StreamTaskReduction {
  internalStatePatch: Partial<GameInternalState> | null;
  uiStatePatch: Partial<StreamTaskUIState> | null;
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
      && choice.disabled === nextChoice.disabled;
  });
}

export function reduceStreamTask({
  internalState,
  uiState,
  task,
  boundRound,
}: StreamTaskReductionInput): StreamTaskReduction {
  const stateView = uiState.stateView;
  const activeRound = Math.max(
    boundRound ?? internalState.displayRound ?? stateView?.turnIndex ?? 1,
    1,
  );
  const isLoading = task.status === 'pending' || task.status === 'running';
  let internalStatePatch: Partial<GameInternalState> | null = null;
  let uiStatePatch: Partial<StreamTaskUIState> | null = null;

  switch (task.kind) {
    case 'narration': {
      const nextText = taskText(task);
      if (!nextText) {
        break;
      }

      const previousRoundState = internalState.roundStates[activeRound];
      const nextRoundState = createRoundState(activeRound, {
        ...(previousRoundState ?? {}),
        round: activeRound,
        title: previousRoundState?.title || stateView?.currentScene || '记录回响',
        narrationText: nextText,
        narrationStatus: task.status,
        isAwaitingNarration: false,
      });
      const nextTurnIndex = Math.max(internalState.turnIndex, activeRound);

      if (
        internalState.turnIndex !== nextTurnIndex
        || internalState.displayRound !== activeRound
        || previousRoundState?.narrationText !== nextRoundState.narrationText
        || previousRoundState?.narrationStatus !== nextRoundState.narrationStatus
        || previousRoundState?.isAwaitingNarration !== nextRoundState.isAwaitingNarration
      ) {
        internalStatePatch = {
          turnIndex: nextTurnIndex,
          displayRound: activeRound,
          roundStates: {
            ...internalState.roundStates,
            [activeRound]: nextRoundState,
          },
        };
      }

      if (stateView && stateView.latestHistory !== nextText) {
        uiStatePatch = {
          stateView: {
            ...stateView,
            latestHistory: nextText,
          },
        };
      }

      if (uiState.skipRestoredNarrationAnimation) {
        uiStatePatch = {
          ...(uiStatePatch ?? {}),
          skipRestoredNarrationAnimation: false,
        };
      }
      break;
    }
    case 'simulation':
    case 'fate_planning': {
      const raw = taskRawContent(task);
      const parsed = parseJsonValue(raw);
      const summary = summarizeFatePlanning(parsed);
      const nextRound = Math.max(summary?.round ?? activeRound, 1);
      const hadRoundState = Boolean(internalState.roundStates[nextRound]);
      const previousRoundState = internalState.roundStates[nextRound];
      const nextTurnIndex = Math.max(internalState.turnIndex, nextRound);
      const nextDisplayRound = internalState.displayRound || nextRound;
      const nextTitle = summary?.sceneTitle ?? previousRoundState?.title ?? '记录回响';

      if (
        internalState.turnIndex !== nextTurnIndex
        || internalState.displayRound !== nextDisplayRound
        || !hadRoundState
        || previousRoundState?.title !== nextTitle
      ) {
        const nextRoundState = createRoundState(nextRound, {
          ...(previousRoundState ?? {}),
          title: nextTitle,
          isAwaitingNarration: previousRoundState?.isAwaitingNarration ?? true,
        });
        internalStatePatch = {
          turnIndex: nextTurnIndex,
          displayRound: nextDisplayRound,
          roundStates: {
            ...internalState.roundStates,
            [nextRound]: nextRoundState,
          },
        };
      }

      if (stateView) {
        const nextBroadcastItems = summary?.newInfo.length
          ? summary.newInfo
          : stateView.latestBroadcastItems;
        const nextStateView = {
          ...stateView,
          turnIndex: nextRound,
          activeTurnId: nextRound,
          currentScene: summary?.sceneTitle ?? taskLabel(task.kind),
          currentLocation: summary?.locationName ?? stateView.currentLocation,
          protagonistState: summary?.protagonistCondition ?? stateView.protagonistState,
          latestBroadcastSummary:
            summary?.currentEvent
            ?? summary?.newInfo[0]
            ?? summary?.locationStatus
            ?? summary?.description
            ?? stateView.latestBroadcastSummary,
          latestBroadcastItems: nextBroadcastItems,
          isEnding: summary?.isEnding ?? stateView.isEnding,
          endingType: summary?.endingType ?? stateView.endingType,
        };

        if (
          stateView.turnIndex !== nextStateView.turnIndex
          || stateView.activeTurnId !== nextStateView.activeTurnId
          || stateView.currentScene !== nextStateView.currentScene
          || stateView.currentLocation !== nextStateView.currentLocation
          || stateView.protagonistState !== nextStateView.protagonistState
          || stateView.latestBroadcastSummary !== nextStateView.latestBroadcastSummary
          || stateView.latestBroadcastItems !== nextStateView.latestBroadcastItems
          || stateView.isEnding !== nextStateView.isEnding
          || stateView.endingType !== nextStateView.endingType
        ) {
          uiStatePatch = {
            stateView: nextStateView,
          };
        }
      }
      break;
    }
    case 'protagonist_action': {
      const nextChoices = protagonistActionChoices(task);
      const choicesStatus = nextChoices ? 'ready' : 'loading';
      const previousRoundState = internalState.roundStates[activeRound];
      const normalizedChoices = nextChoices ?? [];
      const nextRoundState = createRoundState(activeRound, {
        ...(previousRoundState ?? {}),
        round: activeRound,
        choices: normalizedChoices,
        choicesStatus,
        isAwaitingNarration: false,
      });

      if (
        !previousRoundState
        || previousRoundState.choicesStatus !== nextRoundState.choicesStatus
        || previousRoundState.isAwaitingNarration !== nextRoundState.isAwaitingNarration
        || !areChoicesEqual(previousRoundState.choices, normalizedChoices)
      ) {
        internalStatePatch = {
          roundStates: {
            ...internalState.roundStates,
            [activeRound]: nextRoundState,
          },
        };
      }

      const nextProtagonistAction = protagonistActionText(task);
      if (stateView) {
        const nextPhase = task.status === 'done' && nextChoices
          ? 'awaiting_player'
          : stateView.phase;
        if (
          (nextProtagonistAction && stateView.latestProtagonistAction !== nextProtagonistAction)
          || stateView.phase !== nextPhase
        ) {
          uiStatePatch = {
            stateView: {
              ...stateView,
              phase: nextPhase,
              latestProtagonistAction: nextProtagonistAction ?? stateView.latestProtagonistAction,
            },
          };
        }
      }
      break;
    }
    default:
      break;
  }

  if (uiState.isLoading !== isLoading) {
    uiStatePatch = {
      ...(uiStatePatch ?? {}),
      isLoading,
    };
  }

  return {
    internalStatePatch,
    uiStatePatch,
  };
}
