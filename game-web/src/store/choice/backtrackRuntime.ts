import type { StoreApi } from 'zustand';
import { backtrackGameSession } from '../../lib/api';
import type { PlayerActionInput } from '../../lib/api';
import { track } from '../../lib/analytics';
import {
  createRoundState,
  type GameInternalState,
  type RoundState,
  useGameInternalStore,
} from '../gameStore';
import { useGameValueStore } from '../gameValueStore';
import { loadCompleteSessionRounds } from '../session/roundHistoryRuntime';
import { applySessionSnapshotToStores } from '../session/stateSync';
import { isSessionStreamActive } from '../session/streamRuntime';
import type { GameUIStoreState } from '../gameUIStore';

interface ChoiceBacktrackSubmission {
  input: PlayerActionInput;
  displayText: string;
  visited?: boolean;
}

interface BacktrackOptimisticPlan {
  sessionId: string;
  sourceRound: number;
  branchRound: number;
  selectedChoiceText: string;
  selectedChoiceAction: string | null;
  previousInternalState: GameInternalState;
}

function createBacktrackOptimisticPlan(
  internalState: GameInternalState,
  sourceRound: number,
  submission: ChoiceBacktrackSubmission,
): BacktrackOptimisticPlan {
  if (!internalState.sessionId) {
    throw new Error('当前还没有可回溯的记录。');
  }

  if (sourceRound <= 0) {
    throw new Error('回溯章节必须大于 0。');
  }

  const selectedChoiceAction = submission.input.actions[0]?.action.trim() || null;
  return {
    sessionId: internalState.sessionId,
    sourceRound,
    branchRound: sourceRound + 1,
    selectedChoiceText:
      submission.displayText.trim()
      || selectedChoiceAction
      || '继续回响',
    selectedChoiceAction,
    previousInternalState: internalState,
  };
}

function branchHasResolvedContent(roundState: RoundState | undefined): boolean {
  return Boolean(
    roundState?.narrationText.trim()
    || roundState?.narrationStatus === 'done'
    || roundState?.narrationStatus === 'error',
  );
}

function pendingBacktrackPatch(
  state: GameInternalState,
  plan: BacktrackOptimisticPlan,
  options: { preserveResolvedBranch?: boolean } = {},
): Partial<GameInternalState> {
  if (state.sessionId !== plan.sessionId) {
    return {};
  }

  const combinedRoundStates = {
    ...plan.previousInternalState.roundStates,
    ...state.roundStates,
  };
  const roundStates: Record<number, RoundState> = {};
  for (const [roundKey, roundState] of Object.entries(combinedRoundStates)) {
    const round = Number(roundKey);
    if (round <= plan.sourceRound) {
      roundStates[round] = roundState;
    }
  }

  const sourceRoundState =
    combinedRoundStates[plan.sourceRound] ?? createRoundState(plan.sourceRound);
  const sourceRoundChoices = sourceRoundState.choices.map((choice) => (
    plan.selectedChoiceAction && choice.action === plan.selectedChoiceAction
      ? { ...choice, visited: true }
      : choice
  ));
  roundStates[plan.sourceRound] = createRoundState(plan.sourceRound, {
    ...sourceRoundState,
    round: plan.sourceRound,
    selectedChoiceText: plan.selectedChoiceText,
    selectedChoiceAction: plan.selectedChoiceAction,
    choices: sourceRoundChoices,
    choicesStatus: sourceRoundState.choicesStatus,
    isAwaitingNarration: false,
  });

  const branchRoundState = state.roundStates[plan.branchRound];
  if (
    options.preserveResolvedBranch
    && branchHasResolvedContent(branchRoundState)
  ) {
    roundStates[plan.branchRound] = branchRoundState;
  } else {
    roundStates[plan.branchRound] = createRoundState(plan.branchRound, {
      ...(branchRoundState ?? {}),
      round: plan.branchRound,
      narrationText: '',
      narrationStatus: null,
      choices: [],
      choicesStatus: 'loading',
      branchExplorations: [],
      selectedChoiceText: null,
      selectedChoiceAction: null,
      isAwaitingNarration: true,
    });
  }

  return {
    displayRound: plan.branchRound,
    roundStates,
  };
}

function rollbackBacktrackOptimisticUpdate(plan: BacktrackOptimisticPlan) {
  useGameInternalStore.setState((state) => (
    state.sessionId === plan.sessionId ? plan.previousInternalState : state
  ));
}

export async function backtrackGameChoice(
  set: StoreApi<GameUIStoreState>['setState'],
  sourceRound: number,
  submission: ChoiceBacktrackSubmission,
): Promise<void> {
  const internalState = useGameInternalStore.getState();
  const plan = createBacktrackOptimisticPlan(
    internalState,
    sourceRound,
    submission,
  );

  if (!isSessionStreamActive(plan.sessionId)) {
    throw new Error('记录还在铺展中，请稍后再回溯。');
  }

  set({
    isLoading: true,
    error: null,
    skipRestoredNarrationAnimation: false,
  });
  if (!submission.visited) {
    useGameInternalStore.setState((state) => (
      pendingBacktrackPatch(state, plan)
    ));
  }

  try {
    const result = await backtrackGameSession(plan.sessionId, {
      sourceRound,
      action: submission.input,
    });
    const stateView = applySessionSnapshotToStores(result.session, {
      replaceTimeline: true,
    });
    if (!result.reusedExistingBranch) {
      useGameInternalStore.setState((state) => (
        pendingBacktrackPatch(state, plan, { preserveResolvedBranch: true })
      ));
    }
    set({
      stateView,
      generatedProfiles: result.session.generatedProfiles,
      isLoading: false,
      error: null,
      skipRestoredNarrationAnimation: true,
    });
    await loadCompleteSessionRounds(result.session.sessionId, {
      replaceTimeline: true,
    });
    if (!result.reusedExistingBranch) {
      useGameInternalStore.setState((state) => (
        pendingBacktrackPatch(state, plan, { preserveResolvedBranch: true })
      ));
    }
    useGameValueStore.getState().syncRound(result.branchRound);
    track('choice_backtracked', {
      sourceRound: result.sourceRound,
      branchRound: result.branchRound,
      reusedExistingBranch: result.reusedExistingBranch,
      displayText: submission.displayText,
    });
  } catch (error) {
    set({
      isLoading: false,
      error: error instanceof Error ? error.message : '回溯展开失败。',
    });
    rollbackBacktrackOptimisticUpdate(plan);
    throw error;
  }
}
