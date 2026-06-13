import type { StoreApi } from 'zustand';
import { submitGameSessionControl } from '../../lib/api';
import { useGameInternalStore } from '../gameStore';
import type { GameUIStoreState } from '../gameUIStore';

export interface SessionBootstrapRuntime {
  set: StoreApi<GameUIStoreState>['setState'];
  get: StoreApi<GameUIStoreState>['getState'];
  materializeStoryNode: (sessionId: string, nodeId?: string) => void;
}

let bootstrappingSessionId: string | null = null;

export function clearSessionBootstrapState() {
  bootstrappingSessionId = null;
}

export async function bootstrapOpeningSession(
  runtime: SessionBootstrapRuntime,
): Promise<void> {
  const { stateView } = runtime.get();
  const { sessionId } = useGameInternalStore.getState();

  if (!sessionId || !stateView || stateView.phase !== 'booting') {
    return;
  }

  if (bootstrappingSessionId === sessionId) {
    return;
  }

  bootstrappingSessionId = sessionId;

  try {
    const result = await submitGameSessionControl(sessionId, {
      control: { type: 'continue' },
    });
    runtime.materializeStoryNode(sessionId, result.targetNodeId);
  } catch (error) {
    if (bootstrappingSessionId === sessionId) {
      bootstrappingSessionId = null;
    }
    runtime.set({
      isLoading: false,
      error: error instanceof Error ? error.message : '进入回响失败。',
    });
    throw error;
  }
}
