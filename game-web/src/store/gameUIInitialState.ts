import type { GameUIState } from './gameUIStore';
import {
  cloneCharacter,
  cloneStory,
  cloneWorld,
  initialCharacter,
  initialStory,
  initialWorld,
} from './gameStoreHelpers';

export const initialUIState: GameUIState = {
  character: initialCharacter,
  world: initialWorld,
  story: initialStory,
  stateView: null,
  isLoading: false,
  startupStage: 'idle',
  preparedProfiles: null,
  error: null,
  skipRestoredNarrationAnimation: false,
};

export function resetUIState(): GameUIState {
  return {
    character: cloneCharacter(initialCharacter),
    world: cloneWorld(initialWorld),
    story: cloneStory(initialStory),
    stateView: null,
    isLoading: false,
    startupStage: 'idle',
    preparedProfiles: null,
    error: null,
    skipRestoredNarrationAnimation: false,
  };
}
