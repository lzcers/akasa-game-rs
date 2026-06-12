export interface Character {
  name: string;
  gender: string;
  age: number;
  appearance: string;
  traits: {
    intellect: number;
    physique: number;
    endurance: number;
    courage: number;
    rationality: number;
    altruism: number;
  };
  background: string;
}

export interface World {
  era: string;
  description: string;
}

export interface StoryPreferences {
  theme: string;
  atmosphere: string;
  narrativeStyle: string;
  taboos: string;
}

export type TurnPhase =
  | 'start'
  | 'simulation'
  | 'application'
  | 'awaiting_player'
  | 'turn_completed'
  | 'ended'
  | 'failed';

export type RuntimePhase = TurnPhase | 'booting' | 'opening';

export interface Choice {
  id: string;
  text: string;
  action: string;
  disabled: boolean;
  visited?: boolean;
  motivationAndRisk?: string;
}

export interface RuntimeStateView {
  gameState: string;
  phase: RuntimePhase;
  flowEnd: boolean;
  turnIndex: number;
  activeTurnId: number;
  currentLocation: string;
  currentScene: string;
  characterState: string;
  npcsState: string;
  latestHistory: string;
  latestBroadcastSummary: string;
  latestBroadcastItems?: string[];
  latestCharacterAction: string;
  isEnding: boolean;
  endingType?: string | null;
}

export interface SessionWorldState {
  round: number;
  sceneTitle: string;
  timeAbsolute: string;
  timeRelative?: string | null;
  locationName: string;
  locationExits: string[];
  locationStatus: string;
  description: string;
  currentEvent: string;
  newInfo: string[];
  innerConflict: string;
  hardAnchors: string[];
  pace: string;
  atmosphere: string;
  focalPoint: string;
  isEnding: boolean;
  endingType?: string | null;
  characterCondition: string;
  characterKnownSecrets: string[];
}

export interface ApiResponse<T> {
  success: boolean;
  data: T;
}

export interface CreateGameSessionData {
  sessionId: string;
  createdAt: string;
}

export interface CreateSaveSlotInput {
  title?: string;
}

export interface TurnStateArchive {
  phase: TurnPhase;
  turn_index: number;
  active_turn_id: number;
}

export interface CharacterDecisionArchive {
  committed_actions: PlayerActionItem[];
  choices: PendingCharacterChoice[];
}

export interface SessionArchivePayload {
  session_id: string;
  title: string;
  character_name?: string;
  world_profile: string;
  character_profile: string;
  key_story_beats?: string;
  turn_state: TurnStateArchive;
  fate_weaver: unknown;
  upper_narrator: unknown;
  character_agent: unknown;
  world_snapshot: unknown;
  character_decision: CharacterDecisionArchive;
  history_log: unknown;
}

export interface SaveExportData {
  sessionId: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  compressedArchive: string;
}

export interface LoadArchiveInput {
  compressedArchive: string;
}

export interface GeneratedProfiles {
  world: string;
  character: string;
  keyStoryBeats: string;
}

export type CreationGenerationTarget = 'character' | 'world';

export interface GeneratedCreationDraft {
  character?: Character;
  world?: World;
}

export interface StorySummaryData {
  summary: string;
  narrationCount: number;
}

export interface CreateGameSessionInput {
  characterName?: string;
  worldProfile: string;
  characterProfile: string;
  keyStoryBeats?: string;
}

export interface CharacterOption {
  title: string;
  action: string;
  motivationAndRisk?: string;
  motivation_and_risk?: string;
}

export interface PendingCharacterChoice {
  id: string;
  option: CharacterOption;
}

export interface ChoiceExploration {
  visited: boolean;
}

// Keyed by CharacterOption.action so exploration state matches backtrack reuse.
export type ChoiceExplorations = Record<string, ChoiceExploration>;

export type PlayerActionType = 'selected_option' | 'free_text';

export interface PlayerActionItem {
  character_name?: string;
  player_id?: string | null;
  action_type?: PlayerActionType;
  title?: string;
  action: string;
  motivation_and_risk?: string;
}

export interface PlayerActionInput {
  actions: PlayerActionItem[];
}

export interface BranchExploration {
  action: PlayerActionItem;
  visited: boolean;
}

export interface GameSessionWorldStateData {
  sessionId: string;
  generatedProfiles: GeneratedProfiles;
  status: string;
  phase: TurnPhase;
  flowEnd: boolean;
  turnIndex: number;
  activeTurnId: number;
  worldState: SessionWorldState;
  latestNarration: string;
  currentOutcome: string;
  choices: PendingCharacterChoice[];
  choiceExplorations: ChoiceExplorations;
  branchExplorations: BranchExploration[];
}

export interface SessionRoundHistoryData {
  round: number;
  worldState: SessionWorldState | null;
  narrationText: string;
  choices: PendingCharacterChoice[];
  choiceExplorations: ChoiceExplorations;
  branchExplorations: BranchExploration[];
  committedActions: PlayerActionItem[];
  selectedChoiceText?: string | null;
}

export interface SessionRoundsPageData {
  sessionId: string;
  rounds: SessionRoundHistoryData[];
  nextBeforeRound?: number | null;
  hasMore: boolean;
}

export interface StorylineNodeData {
  nodeId: string;
  parentNodeId?: string | null;
  round: number;
  sequenceIndex: number;
  phase: TurnPhase;
  flowEnd: boolean;
  title: string;
  narrationText: string;
  createdAt: string;
  updatedAt: string;
  lastAccessedAt: string;
}

export interface StorylineEdgeData {
  fromNodeId: string;
  toNodeId: string;
  actions: PlayerActionItem[];
  createdAt: string;
}

export interface StorylineData {
  sessionId: string;
  rootNodeId: string;
  activeNodeId: string;
  nodes: StorylineNodeData[];
  edges: StorylineEdgeData[];
}

export interface SelectStorylineNodeInput {
  nodeId: string;
}

export interface GetSessionRoundsOptions {
  beforeRound?: number | null;
  limit?: number;
}

export type GameSessionControlInput =
  | { control: { type: 'continue' }; action?: undefined; expectedRound?: undefined }
  | { control?: undefined; action: PlayerActionInput; expectedRound: number };

export interface BacktrackGameSessionInput {
  sourceRound: number;
  action: PlayerActionInput;
}

export interface BacktrackGameSessionData {
  session: GameSessionWorldStateData;
  sourceRound: number;
  branchRound: number;
  reusedExistingBranch: boolean;
}

export type EngineEvent =
  | {
      type: 'session_created';
      session_id: string;
      world_profile: string;
      character_profile: string;
      key_story_beats: string;
    }
  | {
      type: 'task_update';
      session_id: string;
      round: number;
      entity_name: string;
      chunk: string;
    }
  | {
      type: 'task_completed';
      session_id: string;
      round: number;
      entity_name: string;
      content?: string;
    }
  | {
      type: 'player_input';
      session_id: string;
      round: number;
      actions: PlayerActionItem[];
    }
  | {
      type: 'entity_context_item_appended';
      session_id: string;
      round: number;
      entity_name: string;
      message: unknown;
    }
  | {
      type: 'entity_context_rollback';
      session_id: string;
      round: number;
      entity_name: string;
      policy: 'latest_input';
    }
  | {
      type: 'flow_turn_update';
      session_id: string;
      round: number;
      stage: TurnPhase;
      entity_name: string;
      output_type: 'json' | 'text';
      content?: string;
    }
  | {
      type: 'flow_turn_completed' | 'flow_turn_end';
      session_id: string;
      round: number;
    }
  | {
      type: 'flow_turn_error';
      session_id: string;
      round: number;
      stage: TurnPhase;
      entity_name: string;
      msg: string;
    };

export interface LiveEngineEvent {
  eventId: number;
  event: EngineEvent;
}

const API_ORIGIN = import.meta.env.PROD ? 'https://game.akasa.fun' : '';

function withApiOrigin(path: string) {
  return `${API_ORIGIN}${path}`;
}

async function requestJson<T>(input: string, init?: RequestInit): Promise<T> {
  const response = await fetch(input, {
    headers: {
      'Content-Type': 'application/json',
      ...(init?.headers ?? {}),
    },
    ...init,
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || '网络似乎起了雾，请稍后再试。');
  }

  const payload = (await response.json()) as ApiResponse<T>;
  if (!payload.success) {
    throw new Error('这次操作没能完成，请稍后再试。');
  }

  return payload.data;
}

export function generateProfiles(character: Character, world: World) {
  return requestJson<GeneratedProfiles>(withApiOrigin('/api/profiles/generate'), {
    method: 'POST',
    body: JSON.stringify({ character, world }),
  });
}

export function generateCreationDraft(
  target: CreationGenerationTarget,
  character: Character,
  world: World,
) {
  const requestCharacter = target === 'character'
    ? {
        name: character.name,
        gender: character.gender,
        age: character.age,
        traits: character.traits,
      }
    : character;
  const requestWorld = target === 'world' ? {} : world;

  return requestJson<GeneratedCreationDraft>(withApiOrigin('/api/creation/generate'), {
    method: 'POST',
    body: JSON.stringify({ target, character: requestCharacter, world: requestWorld }),
  });
}

export function createGameSession(input: CreateGameSessionInput) {
  return requestJson<CreateGameSessionData>(withApiOrigin('/api/game-sessions/create'), {
    method: 'POST',
    body: JSON.stringify(input),
  });
}

export function getGameSession(sessionId: string) {
  return requestJson<GameSessionWorldStateData>(
    withApiOrigin(`/api/game-sessions/${encodeURIComponent(sessionId)}`),
  );
}

export function getGameSessionRounds(
  sessionId: string,
  options: GetSessionRoundsOptions = {},
) {
  const params = new URLSearchParams();
  if (options.beforeRound != null) {
    params.set('beforeRound', String(options.beforeRound));
  }
  if (options.limit != null) {
    params.set('limit', String(options.limit));
  }
  const search = params.size > 0 ? `?${params.toString()}` : '';

  return requestJson<SessionRoundsPageData>(
    withApiOrigin(`/api/game-sessions/${encodeURIComponent(sessionId)}/rounds${search}`),
  );
}

export function getGameSessionStoryline(sessionId: string) {
  return requestJson<StorylineData>(
    withApiOrigin(`/api/game-sessions/${encodeURIComponent(sessionId)}/storyline`),
  );
}

export function selectGameSessionStorylineNode(
  sessionId: string,
  input: SelectStorylineNodeInput,
) {
  return requestJson<GameSessionWorldStateData>(
    withApiOrigin(`/api/game-sessions/${encodeURIComponent(sessionId)}/storyline/select`),
    {
      method: 'POST',
      body: JSON.stringify(input),
    },
  );
}

export function cloneGameSession(sessionId: string, sourceRound?: number | null) {
  const params = new URLSearchParams();
  if (sourceRound != null) {
    params.set('round', String(sourceRound));
  }
  const search = params.size > 0 ? `?${params.toString()}` : '';

  return requestJson<GameSessionWorldStateData>(
    withApiOrigin(`/api/game-sessions/${encodeURIComponent(sessionId)}/clone${search}`),
    {
      method: 'POST',
    },
  );
}

export function exportGameSaveArchive(sessionId: string, input: CreateSaveSlotInput) {
  return requestJson<SaveExportData>(withApiOrigin(`/api/game-sessions/${sessionId}/save-export`), {
    method: 'POST',
    body: JSON.stringify(input),
  });
}

export function generateGameSessionStorySummary(sessionId: string) {
  return requestJson<StorySummaryData>(withApiOrigin(`/api/game-sessions/${sessionId}/summary`), {
    method: 'POST',
  });
}

export function loadGameSessionFromArchive(input: LoadArchiveInput) {
  return requestJson<GameSessionWorldStateData>(withApiOrigin('/api/game-sessions/load-archive'), {
    method: 'POST',
    body: JSON.stringify(input),
  });
}

export function submitGameSessionControl(
  sessionId: string,
  input: GameSessionControlInput,
) {
  return requestJson<{ action: string }>(withApiOrigin(`/api/game-sessions/${sessionId}/control`), {
    method: 'POST',
    body: JSON.stringify(input),
  });
}

export function backtrackGameSession(
  sessionId: string,
  input: BacktrackGameSessionInput,
) {
  return requestJson<BacktrackGameSessionData>(
    withApiOrigin(`/api/game-sessions/${sessionId}/backtrack`),
    {
      method: 'POST',
      body: JSON.stringify(input),
    },
  );
}

export function openGameSessionStream(
  sessionId: string,
  handlers: {
    onEngineEvent: (event: LiveEngineEvent, lastEventId: string) => void;
    onOpen?: () => void;
    onError?: () => void;
  },
  since?: string | null,
) {
  const search = since ? `?since=${encodeURIComponent(since)}` : '';
  const eventSource = new EventSource(
    withApiOrigin(`/api/game-sessions/${sessionId}/stream${search}`),
  );

  if (handlers.onOpen) {
    eventSource.addEventListener('open', () => {
      handlers.onOpen?.();
    });
  }

  eventSource.addEventListener('engine.event', (rawEvent) => {
    const event = rawEvent as MessageEvent<string>;
    handlers.onEngineEvent(JSON.parse(event.data) as LiveEngineEvent, event.lastEventId);
  });

  if (handlers.onError) {
    eventSource.addEventListener('error', () => {
      handlers.onError?.();
    });
  }

  return eventSource;
}
