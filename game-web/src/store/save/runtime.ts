import type { StoreApi } from 'zustand';
import { exportGameSaveArchive } from '../../lib/api';
import {
  createStoredSaveSlotId,
  upsertStoredSaveSlot,
  writeStoredSaveArchive,
} from '../../lib/saveSlots';
import { useGameInternalStore } from '../gameStore';
import type { GameUIStoreState } from '../gameUIStore';

type SetGameUIState = StoreApi<GameUIStoreState>['setState'];

export async function createGameSave(
  set: SetGameUIState,
  title?: string,
): Promise<string> {
  const { sessionId } = useGameInternalStore.getState();
  if (!sessionId) {
    throw new Error('此刻还没有可封存的记录。');
  }

  const normalizedTitle = title?.trim();
  set({
    error: null,
    isLoading: true,
  });

  try {
    const saved = await exportGameSaveArchive(sessionId, {
      title: normalizedTitle || undefined,
    });
    const slotId = createStoredSaveSlotId();
    writeStoredSaveArchive(slotId, saved.compressedArchive);
    upsertStoredSaveSlot({
      slotId,
      sessionId: saved.sessionId,
      title: saved.title,
      createdAt: saved.createdAt,
      updatedAt: saved.updatedAt,
    });
    set({
      isLoading: false,
    });
    return slotId;
  } catch (error) {
    set({
      isLoading: false,
      error: error instanceof Error ? error.message : '封存失败。',
    });
    throw error;
  }
}
