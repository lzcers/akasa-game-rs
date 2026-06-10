import { create } from 'zustand';
import type { StoreApi } from 'zustand';
import {
  getGameSession,
  submitGameSessionControl,
} from '../lib/api';
import type {
  Character,
  GeneratedProfiles,
  PlayerActionInput,
  RuntimeStateView,
  StoryPreferences,
  TaskView,
  World,
} from '../lib/api';
import { appRoutes } from '../lib/appRoutes';
import { navigateTo } from '../lib/navigation';
import {
  setAnalyticsGameSessionId,
  track,
} from '../lib/analytics';
import { useGameValueStore } from './gameValueStore';
import {
  initialInternalState,
  useGameInternalStore,
} from './gameStore';
import {
  applySessionSnapshotToStores,
} from './sessionStateSync';
import { reduceStreamTask } from './gameStreamReducer';
import {
  closeSessionStream,
  connectSessionStream as connectSessionStreamRuntime,
  isSessionStreamActive,
} from './gameSessionStreamRuntime';
import {
  clearStartupStageTimer,
  currentStartupFlowRunId,
  isStartupFlowCurrent,
  startStartupFlow,
  waitForNextPaint,
} from './gameStartupRuntime';
import {
  applyChoiceSubmissionOptimisticUpdate,
  planChoiceSubmission,
  rollbackChoiceSubmissionOptimisticUpdate,
} from './gameChoiceSubmission';
import {
  initialUIState,
  resetUIState,
} from './gameUIInitialState';
import { createGameSave } from './gameSaveRuntime';
import {
  clearSessionRestoreState,
  cloneSharedGameSession,
  loadStoredGameSave,
  restoreExistingGameSession,
} from './gameSessionRestoreRuntime';
import {
  bootstrapOpeningSession,
  clearSessionBootstrapState,
} from './gameSessionBootstrapRuntime';
import {
  beginStartupProfileGeneration,
  failStartupProfileGeneration,
  generateStartupProfilesForRun,
  markStartupProfilesReady,
  scheduleStartupProfileStageProgress,
} from './gameStartupProfileRuntime';
import {
  activateStartupGameSession,
  createStartupGameSession,
  requestStartupOpeningNarration,
} from './gameStartupSessionRuntime';

export type StartupStage =
  | 'idle'
  | 'generating_world'
  | 'generating_protagonist'
  | 'ready_to_enter'
  | 'creating_session';

export interface GameUIState {
  // 角色设定表单与存档摘要会读取的人物信息。
  character: Character;
  // 世界设定表单与存档摘要会读取的世界信息。
  world: World;
  // 故事设定表单会读取的叙事偏好与禁区。
  story: StoryPreferences;
  // 运行时视图模型，驱动右侧状态面板等聚合信息。
  stateView: RuntimeStateView | null;
  // 全局加载态，控制按钮禁用、骨架屏等。
  isLoading: boolean;
  // 开局前过渡页当前聚焦的阶段。
  startupStage: StartupStage;
  // 已生成但尚未正式注入会话的世界/主角设定。
  preparedProfiles: GeneratedProfiles | null;
  // 全局错误消息。
  error: string | null;
  // 从存档恢复后，当前已存在叙事应直接展示，不再重新打字。
  skipRestoredNarrationAnimation: boolean;
}

export interface GameUIActions {
  // 操作：更新角色设定。
  updateCharacter: (updates: Partial<Character>) => void;
  // 操作：更新世界设定。
  updateWorld: (updates: Partial<World>) => void;
  // 操作：更新故事设定。
  updateStory: (updates: Partial<StoryPreferences>) => void;
  // 操作：清除错误提示。
  clearError: () => void;
  // 操作：生成设定并进入过渡页。
  startGame: () => Promise<void>;
  // 操作：基于已生成设定正式创建会话并进入游戏。
  enterWorld: () => Promise<{ sessionId: string } | null>;
  // 操作：在进入游玩页后触发开场叙事。
  bootstrapSession: () => Promise<void>;
  // 操作：提交当前选择；执念模式下也可直接提交自定义行动文本。
  submitChoice: (
    submission: { input: PlayerActionInput; displayText: string },
    useObsession?: boolean,
  ) => Promise<void>;
  // 操作：创建当前进度的存档。
  createSave: (title?: string) => Promise<string>;
  // 操作：加载指定存档。
  loadSave: (saveId: string) => Promise<{ sessionId: string }>;
  // 操作：通过仍然存活的后端会话 id 恢复当前进度。
  restoreSession: (sessionId: string) => Promise<void>;
  // 操作：基于分享链接复制一份独立会话并切换到该分支。
  cloneSharedSession: (sourceSessionId: string) => Promise<{ sessionId: string; isEnding: boolean }>;
  // 操作：重置本地游戏状态并关闭流连接。
  resetGame: () => void;
}

export type GameUIStoreState = GameUIState & GameUIActions;

function closeActiveSessionStream() {
  closeSessionStream();
  clearSessionBootstrapState();
  clearSessionRestoreState();
}

function createSessionRestoreRuntime(
  set: StoreApi<GameUIStoreState>['setState'],
  get: StoreApi<GameUIStoreState>['getState'],
) {
  return {
    set,
    get,
    closeSessionStream: closeActiveSessionStream,
    connectSessionStream,
  };
}

function createSessionBootstrapRuntime(
  set: StoreApi<GameUIStoreState>['setState'],
  get: StoreApi<GameUIStoreState>['getState'],
) {
  return {
    set,
    get,
    connectSessionStream,
  };
}

function createStartupProfileRuntime(
  set: StoreApi<GameUIStoreState>['setState'],
  get: StoreApi<GameUIStoreState>['getState'],
) {
  return {
    set,
    get,
    closeSessionStream: closeActiveSessionStream,
  };
}

function createStartupSessionRuntime(set: StoreApi<GameUIStoreState>['setState']) {
  return {
    set,
    connectSessionStream,
  };
}

function errorAnalyticsType(error: unknown) {
  return error instanceof Error ? error.name || 'Error' : typeof error;
}

function connectSessionStream(sessionId: string) {
  connectSessionStreamRuntime(sessionId, {
    onTaskUpdated: applyStreamTaskToStores,
    onSnapshotSyncRequested: (nextSessionId) => {
      void syncActiveSessionSnapshot(nextSessionId);
    },
    onStreamError: () => {
      useGameUIStore.setState({
        error: '连接有些不稳定，正在为你续接这段记录...',
      });
    },
  });
}

async function syncActiveSessionSnapshot(sessionId: string) {
  if (!isSessionStreamActive(sessionId)) {
    return;
  }

  try {
    const session = await getGameSession(sessionId);
    if (!isSessionStreamActive(sessionId)) {
      return;
    }

    useGameUIStore.setState({
      stateView: applySessionSnapshotToStores(session),
      isLoading: false,
      error: session.phase === 'failed' ? '故事推进失败，请稍后重试。' : null,
    });
  } catch (error) {
    if (!isSessionStreamActive(sessionId)) {
      return;
    }
    useGameUIStore.setState({
      isLoading: false,
      error: error instanceof Error ? error.message : '同步故事状态失败。',
    });
  }
}

function applyStreamTaskToStores(task: TaskView, boundRound?: number | null) {
  const { internalStatePatch, uiStatePatch } = reduceStreamTask({
    internalState: useGameInternalStore.getState(),
    uiState: useGameUIStore.getState(),
    task,
    boundRound,
  });

  if (internalStatePatch) {
    useGameInternalStore.setState(internalStatePatch);
  }

  if (uiStatePatch) {
    useGameUIStore.setState(uiStatePatch);
  }
}

const createGameUIActions = (
  set: StoreApi<GameUIStoreState>['setState'],
  get: StoreApi<GameUIStoreState>['getState'],
): GameUIActions => ({
  updateCharacter: (updates) =>
    set((state) => ({
      character: {
        ...state.character,
        ...updates,
        traits: updates.traits ? { ...state.character.traits, ...updates.traits } : state.character.traits,
      },
    })),
  updateWorld: (updates) =>
    set((state) => ({
      world: {
        era: updates.era ?? state.world.era,
        description: updates.description ?? state.world.description,
      },
    })),
  updateStory: (updates) =>
    set((state) => ({
      story: {
        ...state.story,
        ...updates,
      },
    })),
  clearError: () => set({ error: null }),
  startGame: async () => {
    const runId = startStartupFlow();
    const { character, world } = get();
    const runtime = createStartupProfileRuntime(set, get);
    beginStartupProfileGeneration(runtime);
    await waitForNextPaint();
    scheduleStartupProfileStageProgress(runtime);

    let generatedProfiles: GeneratedProfiles | null;
    try {
      generatedProfiles = await generateStartupProfilesForRun(runId, character, world);
      if (!generatedProfiles) {
        return;
      }
    } catch (error) {
      if (!isStartupFlowCurrent(runId)) {
        return;
      }
      failStartupProfileGeneration(runtime, error);
      throw error;
    }

    await markStartupProfilesReady(runtime, generatedProfiles);
    if (!isStartupFlowCurrent(runId)) {
      return;
    }
    await get().enterWorld();
  },
  enterWorld: async () => {
    const runId = currentStartupFlowRunId();
    const { character, world, preparedProfiles, startupStage, stateView } = get();
    const { sessionId } = useGameInternalStore.getState();

    if (startupStage === 'ready_to_enter' && sessionId && stateView) {
      if (!isStartupFlowCurrent(runId)) {
        return null;
      }
      set({
        startupStage: 'idle',
        preparedProfiles: null,
      });
      return { sessionId };
    }

    if (!preparedProfiles) {
      throw new Error('记录还在共鸣中，请稍后再进入。');
    }

    set({
      error: null,
      isLoading: true,
    });
    await waitForNextPaint();

    try {
      const created = await createStartupGameSession(preparedProfiles);
      if (!isStartupFlowCurrent(runId)) {
        return null;
      }

      activateStartupGameSession(
        createStartupSessionRuntime(set),
        created.sessionId,
        character,
        world,
      );
      await requestStartupOpeningNarration(created.sessionId);
      if (!isStartupFlowCurrent(runId)) {
        return null;
      }
      set((state) => ({
        error: null,
        isLoading: false,
        skipRestoredNarrationAnimation: false,
        startupStage: 'ready_to_enter',
        stateView: state.stateView
          ? {
            ...state.stateView,
            phase: 'opening',
          }
          : state.stateView,
      }));
    } catch (error) {
      if (!isStartupFlowCurrent(runId)) {
        return null;
      }
      track('game_session_create_failed', {
        errorType: errorAnalyticsType(error),
      });
      closeActiveSessionStream();
      useGameInternalStore.setState({
        ...initialInternalState,
      });
      set({
        stateView: null,
        isLoading: false,
        startupStage: 'ready_to_enter',
        skipRestoredNarrationAnimation: false,
        error: error instanceof Error ? error.message : '进入回响失败。',
      });
      navigateTo(appRoutes.generating, { replace: true });
      throw error;
    }
  },
  bootstrapSession: () => bootstrapOpeningSession(createSessionBootstrapRuntime(set, get)),
  submitChoice: async (submission, useObsession = false) => {
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
      });
      track(
        'choice_submitted',
        submissionPlan.input.type === 'free_text'
          ? {
            choiceType: submissionPlan.input.type,
            actionText: submissionPlan.input.action,
          }
          : {
            choiceType: submissionPlan.input.type,
          },
      );
      if (useObsession) {
        consumeObsession();
      }
      syncRound(submissionPlan.nextRound);
      return;
    } catch (error) {
      set({
        isLoading: false,
        error: error instanceof Error ? error.message : '提交选择失败。',
      });
      useGameInternalStore.setState((state) => (
        rollbackChoiceSubmissionOptimisticUpdate(state, submissionPlan)
      ));
      throw error;
    }
  },
  createSave: (title) => createGameSave(set, title),
  loadSave: (saveId) => loadStoredGameSave(createSessionRestoreRuntime(set, get), saveId),
  restoreSession: (sessionId) => (
    restoreExistingGameSession(createSessionRestoreRuntime(set, get), sessionId)
  ),
  cloneSharedSession: (sourceSessionId) => (
    cloneSharedGameSession(createSessionRestoreRuntime(set, get), sourceSessionId)
  ),
  resetGame: () => {
    closeActiveSessionStream();
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
  },
});

export const useGameUIStore = create<GameUIStoreState>((set, get) => ({
  ...initialUIState,
  ...createGameUIActions(set, get),
}));
