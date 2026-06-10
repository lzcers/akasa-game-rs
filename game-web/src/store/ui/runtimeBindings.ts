import type { StoreApi } from 'zustand';
import {
  clearSessionBootstrapState,
} from '../session/bootstrapRuntime';
import {
  clearSessionRestoreState,
} from '../session/restoreRuntime';
import {
  closeSessionStream,
  connectGameSessionStream,
} from '../session/streamRuntime';
import type { GameUIStoreState } from '../gameUIStore';

type SetGameUIState = StoreApi<GameUIStoreState>['setState'];
type GetGameUIState = StoreApi<GameUIStoreState>['getState'];

function closeActiveSessionStream() {
  closeSessionStream();
  clearSessionBootstrapState();
  clearSessionRestoreState();
}

function connectSessionStream(sessionId: string) {
  connectGameSessionStream(sessionId, {
    set: useGameUIStoreAccess.set,
    get: useGameUIStoreAccess.get,
  });
}

const useGameUIStoreAccess: {
  set: SetGameUIState;
  get: GetGameUIState;
} = {
  set: () => undefined,
  get: () => {
    throw new Error('Game UI store runtime bindings are not initialized.');
  },
};

export function bindGameUIStoreAccess(
  set: SetGameUIState,
  get: GetGameUIState,
) {
  useGameUIStoreAccess.set = set;
  useGameUIStoreAccess.get = get;
}

export function createSessionRestoreRuntime(
  set: SetGameUIState,
  get: GetGameUIState,
) {
  return {
    set,
    get,
    closeSessionStream: closeActiveSessionStream,
    connectSessionStream,
  };
}

export function createSessionBootstrapRuntime(
  set: SetGameUIState,
  get: GetGameUIState,
) {
  return {
    set,
    get,
    connectSessionStream,
  };
}

export function createStartupFlowRuntime(
  set: SetGameUIState,
  get: GetGameUIState,
) {
  return {
    set,
    get,
    closeSessionStream: closeActiveSessionStream,
    connectSessionStream,
    enterWorld: () => get().enterWorld(),
  };
}

export function resetGameWithBindings(set: SetGameUIState) {
  return {
    set,
    closeSessionStream: closeActiveSessionStream,
  };
}
