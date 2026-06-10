import type { Character, World } from './api';
import { cloneCharacter, cloneWorld } from '../store/gameStoreHelpers';

interface CreationDraft {
  character: Character;
  world: World;
}

const CREATION_DRAFT_STORAGE_KEY = 'akashic-creation-draft';
const traitKeys = [
  'intellect',
  'physique',
  'endurance',
  'courage',
  'rationality',
  'altruism',
] as const;

function canUseLocalStorage() {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function isCharacterDraft(value: unknown): value is Character {
  if (!value || typeof value !== 'object') {
    return false;
  }

  const character = value as Partial<Character>;
  const traits = character.traits as Partial<Character['traits']> | undefined;
  return typeof character.name === 'string'
    && typeof character.gender === 'string'
    && typeof character.age === 'number'
    && Number.isFinite(character.age)
    && typeof character.appearance === 'string'
    && typeof character.background === 'string'
    && Boolean(traits)
    && traitKeys.every((key) => typeof traits?.[key] === 'number');
}

function isWorldDraft(value: unknown): value is World {
  if (!value || typeof value !== 'object') {
    return false;
  }

  const world = value as Partial<World>;
  return typeof world.era === 'string'
    && typeof world.description === 'string';
}

export function readCreationDraft(): CreationDraft | null {
  if (!canUseLocalStorage()) {
    return null;
  }

  const raw = window.localStorage.getItem(CREATION_DRAFT_STORAGE_KEY);
  if (!raw) {
    return null;
  }

  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') {
      return null;
    }

    const draft = parsed as Partial<CreationDraft>;
    if (!isCharacterDraft(draft.character) || !isWorldDraft(draft.world)) {
      return null;
    }

    return {
      character: cloneCharacter(draft.character),
      world: cloneWorld(draft.world),
    };
  } catch {
    return null;
  }
}

export function writeCreationDraft(draft: CreationDraft) {
  if (!canUseLocalStorage()) {
    return false;
  }

  window.localStorage.setItem(CREATION_DRAFT_STORAGE_KEY, JSON.stringify(draft));
  return true;
}

export function removeCreationDraft() {
  if (!canUseLocalStorage()) {
    return false;
  }

  window.localStorage.removeItem(CREATION_DRAFT_STORAGE_KEY);
  return true;
}
