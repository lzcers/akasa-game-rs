import type { StoreApi } from 'zustand';
import {
  clearSessionBootstrapState,
} from '../session/bootstrapRuntime';
import {
  clearSessionRestoreState,
} from '../session/restoreRuntime';
import {
  closeStoryNodeStream,
  materializeActiveStoryNode,
  materializeStoryNode as materializeSpecificStoryNode,
} from '../session/streamRuntime';
import type { GameUIStoreState } from '../gameUIStore';

type SetGameUIState = StoreApi<GameUIStoreState>['setState'];
type GetGameUIState = StoreApi<GameUIStoreState>['getState'];

function closeActiveStoryNodeStream() {
  closeStoryNodeStream();
  clearSessionBootstrapState();
  clearSessionRestoreState();
}

function requestStoryNodeMaterialization(sessionId: string, nodeId?: string) {
  const runtime = {
    set: useGameUIStoreAccess.set,
    get: useGameUIStoreAccess.get,
  };
  if (nodeId) {
    void materializeSpecificStoryNode(sessionId, nodeId, runtime);
    return;
  }
  materializeActiveStoryNode(sessionId, runtime);
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
    closeStoryNodeStream: closeActiveStoryNodeStream,
    materializeStoryNode: requestStoryNodeMaterialization,
  };
}

export function createSessionBootstrapRuntime(
  set: SetGameUIState,
  get: GetGameUIState,
) {
  return {
    set,
    get,
    materializeStoryNode: requestStoryNodeMaterialization,
  };
}

export function createStartupFlowRuntime(
  set: SetGameUIState,
  get: GetGameUIState,
) {
  return {
    set,
    get,
    closeStoryNodeStream: closeActiveStoryNodeStream,
    materializeStoryNode: requestStoryNodeMaterialization,
    enterWorld: () => get().enterWorld(),
  };
}

export function resetGameWithBindings(set: SetGameUIState) {
  return {
    set,
    closeStoryNodeStream: closeActiveStoryNodeStream,
  };
}
