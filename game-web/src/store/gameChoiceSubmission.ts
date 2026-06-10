import type { PlayerActionInput } from '../lib/api';
import {
  createRoundState,
  type GameInternalState,
  type RoundState,
} from './gameStore';

interface ChoiceSubmission {
  input: PlayerActionInput;
  displayText: string;
}

interface ChoiceSubmissionPlanInput {
  internalState: GameInternalState;
  submission: ChoiceSubmission;
  useObsession: boolean;
  obsessionPoints: number;
}

export interface ChoiceSubmissionPlan {
  sessionId: string;
  input: PlayerActionInput;
  activeRound: number;
  nextRound: number;
  selectedChoiceText: string;
  previousRoundState: RoundState | undefined;
  previousNextRoundState: RoundState | undefined;
}

export function planChoiceSubmission({
  internalState,
  submission,
  useObsession,
  obsessionPoints,
}: ChoiceSubmissionPlanInput): ChoiceSubmissionPlan {
  const { sessionId, displayRound, roundStates } = internalState;
  if (!sessionId) {
    throw new Error('当前还没有进行中的记录。');
  }

  const input: PlayerActionInput = {
    type: submission.input.type,
    action: submission.input.action.trim(),
  };
  if (!input.action) {
    throw new Error('写下你此刻想写入记录的事。');
  }

  if (useObsession && obsessionPoints <= 0) {
    throw new Error('执念点不足，无法继续写入记录。');
  }

  const activeRound = Math.max(displayRound || 1, 1);
  const nextRound = activeRound + 1;
  const currentRoundChoices = roundStates[activeRound]?.choices ?? [];
  if (
    input.type === 'selected_option'
    && !currentRoundChoices.some((choice) => choice.action === input.action)
  ) {
    throw new Error('这条分支已失效，请重新选择。');
  }

  return {
    sessionId,
    input,
    activeRound,
    nextRound,
    selectedChoiceText: useObsession
      ? `${submission.displayText} [执念]`
      : submission.displayText,
    previousRoundState: roundStates[activeRound],
    previousNextRoundState: roundStates[nextRound],
  };
}

export function applyChoiceSubmissionOptimisticUpdate(
  state: GameInternalState,
  plan: ChoiceSubmissionPlan,
): Partial<GameInternalState> {
  return {
    displayRound: plan.nextRound,
    roundStates: {
      ...state.roundStates,
      [plan.activeRound]: createRoundState(plan.activeRound, {
        ...(state.roundStates[plan.activeRound] ?? {}),
        round: plan.activeRound,
        selectedChoiceText: plan.selectedChoiceText,
        selectedChoiceAction: plan.input.action,
        choices: [],
        choicesStatus: 'idle',
        isAwaitingNarration: false,
      }),
      [plan.nextRound]: createRoundState(plan.nextRound, {
        ...(state.roundStates[plan.nextRound] ?? {}),
        round: plan.nextRound,
        choices: [],
        choicesStatus: 'loading',
        isAwaitingNarration: true,
      }),
    },
  };
}

export function rollbackChoiceSubmissionOptimisticUpdate(
  state: GameInternalState,
  plan: ChoiceSubmissionPlan,
): Partial<GameInternalState> {
  const nextRoundStates = { ...state.roundStates };
  if (plan.previousRoundState) {
    nextRoundStates[plan.activeRound] = plan.previousRoundState;
  } else {
    delete nextRoundStates[plan.activeRound];
  }

  if (plan.previousNextRoundState) {
    nextRoundStates[plan.nextRound] = plan.previousNextRoundState;
  } else {
    delete nextRoundStates[plan.nextRound];
  }

  return {
    displayRound: plan.activeRound,
    roundStates: nextRoundStates,
  };
}
