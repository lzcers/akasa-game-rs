import type { StoreApi } from 'zustand';
import { createGameSave } from '../save/runtime';
import {
  cloneSharedGameSession,
  loadStoredGameSave,
  restoreExistingGameSession,
  selectStorylineNodeForSession,
} from '../session/restoreRuntime';
import {
  bootstrapOpeningSession,
} from '../session/bootstrapRuntime';
import { resetGameRuntime } from './resetRuntime';
import { backtrackGameChoice } from '../choice/backtrackRuntime';
import { submitGameChoice } from '../choice/submissionRuntime';
import {
  clearGameError,
  updateGameCharacter,
  updateGameStory,
  updateGameWorld,
} from './formActionsRuntime';
import {
  enterWorldFlow,
  startGameFlow,
} from '../startup/flowRuntime';
import {
  createSessionBootstrapRuntime,
  createSessionRestoreRuntime,
  createStartupFlowRuntime,
  resetGameWithBindings,
} from './runtimeBindings';
import type {
  GameUIActions,
  GameUIStoreState,
} from '../gameUIStore';

export function createGameUIActions(
  set: StoreApi<GameUIStoreState>['setState'],
  get: StoreApi<GameUIStoreState>['getState'],
): GameUIActions {
  return {
    updateCharacter: (updates) => updateGameCharacter(set, updates),
    updateWorld: (updates) => updateGameWorld(set, updates),
    updateStory: (updates) => updateGameStory(set, updates),
    clearError: () => clearGameError(set),
    startGame: () => startGameFlow(createStartupFlowRuntime(set, get)),
    enterWorld: () => enterWorldFlow(createStartupFlowRuntime(set, get)),
    bootstrapSession: () => bootstrapOpeningSession(createSessionBootstrapRuntime(set, get)),
    submitChoice: (submission, useObsession = false) => (
      submitGameChoice(set, submission, useObsession)
    ),
    backtrackChoice: (sourceRound, submission) => (
      backtrackGameChoice(set, sourceRound, submission)
    ),
    createSave: (title) => createGameSave(set, title),
    loadSave: (saveId) => loadStoredGameSave(createSessionRestoreRuntime(set, get), saveId),
    restoreSession: (sessionId) => (
      restoreExistingGameSession(createSessionRestoreRuntime(set, get), sessionId)
    ),
    cloneSharedSession: (sourceSessionId, sourceRound = null) => (
      cloneSharedGameSession(createSessionRestoreRuntime(set, get), sourceSessionId, sourceRound)
    ),
    selectStorylineNode: (sessionId, nodeId) => (
      selectStorylineNodeForSession(createSessionRestoreRuntime(set, get), sessionId, nodeId)
    ),
    resetGame: () => {
      const runtime = resetGameWithBindings(set);
      resetGameRuntime(runtime.set, runtime.closeSessionStream);
    },
  };
}
