import type { StoreApi } from 'zustand';
import type {
  Character,
  StoryPreferences,
  World,
} from '../../lib/api';
import type { GameUIStoreState } from '../gameUIStore';

type SetGameUIState = StoreApi<GameUIStoreState>['setState'];

export function updateGameCharacter(
  set: SetGameUIState,
  updates: Partial<Character>,
) {
  set((state) => ({
    character: {
      ...state.character,
      ...updates,
      traits: updates.traits ? { ...state.character.traits, ...updates.traits } : state.character.traits,
    },
  }));
}

export function updateGameWorld(
  set: SetGameUIState,
  updates: Partial<World>,
) {
  set((state) => ({
    world: {
      era: updates.era ?? state.world.era,
      description: updates.description ?? state.world.description,
    },
  }));
}

export function updateGameStory(
  set: SetGameUIState,
  updates: Partial<StoryPreferences>,
) {
  set((state) => ({
    story: {
      ...state.story,
      ...updates,
    },
  }));
}

export function clearGameError(set: SetGameUIState) {
  set({ error: null });
}
