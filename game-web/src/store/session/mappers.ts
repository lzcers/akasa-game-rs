import type {
  GameSessionWorldStateData,
  RuntimeStateView,
  SessionRoundHistoryData,
} from "../../lib/api";
import {
  createRoundState,
  type GameInternalState,
  type RoundState,
} from "../gameStore";
import { toChoiceFromSession } from "./choiceMapping";
import { STREAM_PLACEHOLDER_TEXT } from "./taskContent";

function titleFromWorldState(
  worldState:
    | SessionRoundHistoryData["worldState"]
    | GameSessionWorldStateData["worldState"]
    | null
    | undefined,
  fallback = "",
): string {
  return worldState?.sceneTitle?.trim() || fallback;
}

export function effectiveDisplayRound(
  session: GameSessionWorldStateData,
): number {
  if (session.phase === "awaiting_player") {
    return Math.max(session.activeTurnId, 1);
  }

  return Math.max(session.turnIndex, session.activeTurnId, 1);
}

function latestHistoryFromSession(session: GameSessionWorldStateData): string {
  return (
    session.latestNarration.trim() ||
    session.worldState.description.trim() ||
    STREAM_PLACEHOLDER_TEXT
  );
}

function latestBroadcastItemsFromSession(
  session: GameSessionWorldStateData,
): string[] {
  const nextItems = session.worldState.newInfo
    .map((item) => item.trim())
    .filter(Boolean);
  if (nextItems.length > 0) {
    return nextItems;
  }

  const fallback =
    session.worldState.currentEvent.trim() ||
    session.worldState.description.trim();
  return fallback ? [fallback] : [];
}

export function stateViewFromSession(
  session: GameSessionWorldStateData,
): RuntimeStateView {
  const latestBroadcastItems = latestBroadcastItemsFromSession(session);
  return {
    gameState: "playing",
    phase: session.phase,
    flowEnd: session.flowEnd,
    turnIndex: session.turnIndex,
    activeTurnId: session.activeTurnId,
    currentLocation: session.worldState.locationName || "记录现场",
    currentScene: session.worldState.sceneTitle || "",
    characterState: session.worldState.characterCondition || "记录仍在酝酿",
    npcsState: session.worldState.currentEvent || "诸多回响正在汇聚",
    latestHistory: latestHistoryFromSession(session),
    latestBroadcastSummary:
      session.worldState.currentEvent ||
      session.worldState.description ||
      "记录已续上",
    latestBroadcastItems,
    latestCharacterAction: session.currentOutcome || "你还没有写下选择",
    isEnding: session.worldState.isEnding,
    endingType: session.worldState.endingType ?? null,
  };
}

export function internalStateFromSession(
  session: GameSessionWorldStateData,
): GameInternalState {
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
  const choices = entry.choices.map((choice) =>
    toChoiceFromSession(
      choice,
      entry.choiceExplorations?.[choice.option.action],
    ),
  );
  const selectedChoiceText =
    entry.selectedChoiceText?.trim() || deriveSelectedChoiceText(entry) || null;
  const selectedChoiceAction = entry.committedActions[0]?.action.trim() || null;

  return createRoundState(entry.round, {
    nodeId: entry.nodeId,
    title: titleFromWorldState(entry.worldState),
    narrationText: entry.narrationText.trim(),
    narrationStatus: entry.narrationText.trim() ? "done" : null,
    choices,
    choicesStatus: choices.length > 0 ? "ready" : "idle",
    branchExplorations: entry.branchExplorations ?? [],
    selectedChoiceText,
    selectedChoiceAction,
    isAwaitingNarration: false,
  });
}

function deriveSelectedChoiceText(
  entry: SessionRoundHistoryData,
): string | null {
  const committed = entry.committedActions[0];
  const committedAction = committed?.action.trim();
  if (!committedAction) {
    return null;
  }

  if (committed?.action_type === "free_text") {
    return committedAction === "continue" ? "继续回响" : "[执念]";
  }

  const matchedChoice = entry.choices.find(
    (choice) => choice.option.action === committedAction,
  );
  return matchedChoice?.option.title || committed?.title || committedAction;
}

function currentRoundStateFromSession(
  session: GameSessionWorldStateData,
  round: number,
): RoundState {
  return {
    ...createRoundState(round, {
      nodeId: session.activeNodeId,
      title: titleFromWorldState(session.worldState),
      narrationText: latestHistoryFromSession(session),
      narrationStatus: session.latestNarration.trim() ? "done" : null,
      choices: session.choices.map((choice) =>
        toChoiceFromSession(
          choice,
          session.choiceExplorations?.[choice.option.action],
        ),
      ),
      choicesStatus:
        session.choices.length > 0 || session.phase === "awaiting_player"
          ? "ready"
          : "idle",
      branchExplorations: session.branchExplorations ?? [],
      selectedChoiceText: null,
      selectedChoiceAction: null,
      isAwaitingNarration: false,
    }),
  };
}
