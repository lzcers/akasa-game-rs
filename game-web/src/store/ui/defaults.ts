import type {
  Character,
  StoryPreferences,
  World,
} from '../../lib/api';

export const initialCharacter: Character = {
  name: '',
  gender: '',
  age: 18,
  appearance: '',
  traits: {
    intellect: 5,
    physique: 5,
    endurance: 5,
    courage: 5,
    rationality: 5,
    altruism: 5,
  },
  background: '',
};

export const initialWorld: World = {
  era: '蒸汽朋克',
  description: '',
};

export const initialStory: StoryPreferences = {
  theme: '',
  atmosphere: '',
  narrativeStyle: '',
  taboos: '',
};

export function cloneCharacter(character: Character): Character {
  return {
    ...character,
    traits: { ...character.traits },
  };
}

export function cloneWorld(world: World): World {
  return {
    era: world.era,
    description: world.description,
  };
}

export function cloneStory(story: StoryPreferences): StoryPreferences {
  return {
    ...story,
  };
}
