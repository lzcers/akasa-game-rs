import type { StoreApi } from 'zustand';
import { submitGameSessionControl } from '../../lib/api';
import type { PlayerActionInput } from '../../lib/api';
import { track } from '../../lib/analytics';
import { useGameInternalStore } from '../gameStore';
import { useGameValueStore } from '../gameValueStore';
import {
  isSessionStreamActive,
} from '../session/streamRuntime';
import {
  applyChoiceSubmissionOptimisticUpdate,
  planChoiceSubmission,
  rollbackChoiceSubmissionOptimisticUpdate,
} from './submissionPlan';
import type { GameUIStoreState } from '../gameUIStore';

const submittingChoiceKeys = new Set<string>();

export async function submitGameChoice(
  set: StoreApi<GameUIStoreState>['setState'],
  submission: { input: PlayerActionInput; displayText: string },
  useObsession = false,
): Promise<void> {
  const internalState = useGameInternalStore.getState();
  const {
    obsessionPoints,
    consumeObsession,
    syncRound,
  } = useGameValueStore.getState();

  if (!internalState.sessionId) {
    throw new Error('当前还没有进行中的记录。');
  }

  if (!isSessionStreamActive(internalState.sessionId)) {
    throw new Error('记录还在铺展中，请稍后再选择。');
  }

  const submissionPlan = planChoiceSubmission({
    internalState,
    submission,
    useObsession,
    obsessionPoints,
  });
  const submissionKey = `${submissionPlan.sessionId}:${submissionPlan.activeRound}`;
  if (submittingChoiceKeys.has(submissionKey)) {
    throw new Error('这一轮选择正在写入，请稍候。');
  }
  submittingChoiceKeys.add(submissionKey);

  set({
    isLoading: true,
    error: null,
    skipRestoredNarrationAnimation: false,
  });
  useGameInternalStore.setState((state) => (
    applyChoiceSubmissionOptimisticUpdate(state, submissionPlan)
  ));

  try {
    await submitGameSessionControl(submissionPlan.sessionId, {
      action: submissionPlan.input,
      expectedRound: submissionPlan.activeRound,
    });
    const primaryAction = submissionPlan.input.actions[0];
    track(
      'choice_submitted',
      primaryAction?.action_type === 'free_text'
        ? {
          choiceType: primaryAction.action_type,
          actionText: primaryAction.action,
        }
        : {
          choiceType: primaryAction?.action_type ?? 'unknown',
        },
    );
    if (useObsession) {
      consumeObsession();
    }
    syncRound(submissionPlan.nextRound);
  } catch (error) {
    set({
      isLoading: false,
      error: error instanceof Error ? error.message : '提交选择失败。',
    });
    useGameInternalStore.setState((state) => (
      rollbackChoiceSubmissionOptimisticUpdate(state, submissionPlan)
    ));
    throw error;
  } finally {
    submittingChoiceKeys.delete(submissionKey);
  }
}
