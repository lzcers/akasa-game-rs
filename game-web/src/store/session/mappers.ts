import type {
  GameSessionWorldStateData,
  RuntimeStateView,
  SessionRoundHistoryData,
} from '../../lib/api';
import {
  createRoundState,
  type GameInternalState,
  type RoundState,
} from '../gameStore';
import { toChoiceFromSession } from './choiceMapping';
import { STREAM_PLACEHOLDER_TEXT } from './taskContent';

function titleFromWorldState(
  worldState: SessionRoundHistoryData['worldState'] | GameSessionWorldStateData['worldState'] | null | undefined,
  fallback = '记录回响',
): string {
  return worldState?.sceneTitle?.trim() || fallback;
}

export function effectiveDisplayRound(session: GameSessionWorldStateData): number {
  if (session.phase === 'awaiting_player') {
    return Math.max(session.activeTurnId, 1);
  }

  return Math.max(session.turnIndex, session.activeTurnId, 1);
}

function latestHistoryFromSession(session: GameSessionWorldStateData): string {
  return session.latestNarration.trim()
    || session.worldState.description.trim()
    || STREAM_PLACEHOLDER_TEXT;
}

function latestBroadcastItemsFromSession(session: GameSessionWorldStateData): string[] {
  const nextItems = session.worldState.newInfo
    .map((item) => item.trim())
    .filter(Boolean);
  if (nextItems.length > 0) {
    return nextItems;
  }

  const fallback = session.worldState.currentEvent.trim() || session.worldState.description.trim();
  return fallback ? [fallback] : [];
}

export function stateViewFromSession(session: GameSessionWorldStateData): RuntimeStateView {
  const latestBroadcastItems = latestBroadcastItemsFromSession(session);
  return {
    gameState: 'playing',
    phase: session.phase,
    flowEnd: session.flowEnd,
    turnIndex: session.turnIndex,
    activeTurnId: session.activeTurnId,
    currentLocation: session.worldState.locationName || '记录现场',
    currentScene: session.worldState.sceneTitle || '记录回响',
    characterState: session.worldState.characterCondition || '记录仍在酝酿',
    npcsState: session.worldState.currentEvent || '诸多回响正在汇聚',
    latestHistory: latestHistoryFromSession(session),
    latestBroadcastSummary:
      session.worldState.currentEvent
      || session.worldState.description
      || '记录已续上',
    latestBroadcastItems,
    latestCharacterAction: session.currentOutcome || '你还没有写下选择',
    isEnding: session.worldState.isEnding,
    endingType: session.worldState.endingType ?? null,
  };
}

export function internalStateFromSession(session: GameSessionWorldStateData): GameInternalState {
  const round = effectiveDisplayRound(session);
  return {
    sessionId: session.sessionId,
    turnIndex: session.turnIndex,
    displayRound: round,
    roundStates: {
      [round]: currentRoundStateFromSession(session, round),
    },
  };
}

export function roundStateFromPersistedHistoryEntry(
  entry: SessionRoundHistoryData,
): RoundState {
  const choices = entry.choices.map(toChoiceFromSession);
  const selectedChoiceText = entry.selectedChoiceText?.trim()
    || deriveSelectedChoiceText(entry)
    || null;
  const selectedChoiceAction = entry.committedActions[0]?.action.trim() || null;

  return createRoundState(entry.round, {
    title: titleFromWorldState(entry.worldState),
    narrationText: entry.narrationText.trim(),
    narrationStatus: entry.narrationText.trim() ? 'done' : null,
    choices,
    choicesStatus: choices.length > 0 ? 'ready' : 'idle',
    selectedChoiceText,
    selectedChoiceAction,
    isAwaitingNarration: false,
  });
}

function deriveSelectedChoiceText(entry: SessionRoundHistoryData): string | null {
  const committedAction = entry.committedActions[0]?.action.trim();
  if (!committedAction) {
    return null;
  }

  const matchedChoice = entry.choices.find((choice) => choice.option.action === committedAction);
  return matchedChoice?.option.title || entry.committedActions[0]?.title || committedAction;
}

function currentRoundStateFromSession(
  session: GameSessionWorldStateData,
  round: number,
): RoundState {
  return {
    ...createRoundState(round, {
      title: titleFromWorldState(session.worldState),
      narrationText: latestHistoryFromSession(session),
      narrationStatus: session.latestNarration.trim() ? 'done' : null,
      choices: session.choices.map(toChoiceFromSession),
      choicesStatus: session.choices.length > 0 || session.phase === 'awaiting_player' ? 'ready' : 'idle',
      selectedChoiceText: null,
      selectedChoiceAction: null,
      isAwaitingNarration: false,
    }),
  };
}
