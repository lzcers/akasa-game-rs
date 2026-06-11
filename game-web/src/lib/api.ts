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
  protagonistState: string;
  npcsState: string;
  latestHistory: string;
  latestBroadcastSummary: string;
  latestBroadcastItems?: string[];
  latestProtagonistAction: string;
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
  protagonistCondition: string;
  protagonistKnownSecrets: string[];
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

export interface ProtagonistDecisionArchive {
  committed_action: string;
  choices: PendingProtagonistChoice[];
}

export interface SessionArchivePayload {
  session_id: string;
  title: string;
  world_profile: string;
  protagonist_profile: string;
  key_story_beats?: string;
  turn_state: TurnStateArchive;
  fate_weaver: unknown;
  upper_narrator: unknown;
  protagonist: unknown;
  world_snapshot: unknown;
  protagonist_decision: ProtagonistDecisionArchive;
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
  protagonist: string;
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
  worldProfile: string;
  protagonistProfile: string;
  keyStoryBeats?: string;
}

export interface ProtagonistOption {
  title: string;
  action: string;
  motivationAndRisk?: string;
  motivation_and_risk?: string;
}

export interface PendingProtagonistChoice {
  id: string;
  option: ProtagonistOption;
}

export type PlayerActionType = 'selected_option' | 'free_text';

export interface PlayerActionInput {
  type: PlayerActionType;
  action: string;
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
  choices: PendingProtagonistChoice[];
}

export interface SessionRoundHistoryData {
  round: number;
  worldState: SessionWorldState | null;
  narrationText: string;
  choices: PendingProtagonistChoice[];
  committedAction?: string | null;
  selectedChoiceText?: string | null;
}

export interface SessionRoundsPageData {
  sessionId: string;
  rounds: SessionRoundHistoryData[];
  nextBeforeRound?: number | null;
  hasMore: boolean;
}

export interface GetSessionRoundsOptions {
  beforeRound?: number | null;
  limit?: number;
}

export type GameSessionControlInput =
  | { control: { type: 'continue' }; action?: undefined }
  | { control?: undefined; action: PlayerActionInput };

export type EngineEvent =
  | {
      type: 'session_created';
      session_id: string;
      world_profile: string;
      protagonist_profile: string;
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
      content: string;
    }
  | {
      type: 'player_input';
      session_id: string;
      round: number;
      action_type: PlayerActionType;
      action: string;
    }
  | {
      type: 'agent_context_item_appended';
      session_id: string;
      round: number;
      agent_name: string;
      message: unknown;
    }
  | {
      type: 'agent_context_rollback';
      session_id: string;
      round: number;
      agent_name: string;
      policy: 'latest_input';
    }
  | {
      type: 'flow_turn_update';
      session_id: string;
      round: number;
      stage: TurnPhase;
      entity_name: string;
      output_type: 'json' | 'text';
      content: string;
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

export function buildGenerateProfilesPrompt(
  character: Character,
  world: World,
): string {
  return `请基于以下两组设定，像从“阿卡夏记录”中与玩家输入共鸣一样，生成“世界记录”和“角色记录”。

这些表单内容都是已确定事实，禁止改写、替换或否定，只能围绕它们做扩写、补完和强化。生成结果应像记录被唤醒、世界与角色逐步显影，而不是普通设定简介。

[角色记录种子]
- 姓名：${character.name}
- 性别：${character.gender}
- 年龄：${character.age}
- 角色烙印：${character.background || '未填写'}
- 角色描述：${character.appearance || '未填写'}
- 属性分配：
  - 智力：${character.traits.intellect}
  - 体力：${character.traits.physique}
  - 耐力：${character.traits.endurance}
  - 勇气：${character.traits.courage}
  - 理性：${character.traits.rationality}
  - 利他：${character.traits.altruism}

[世界记录种子]
- 时代：${world.era}
- 世界记录：${world.description || '未填写'}

[生成目标]
- 这是长期 AI 互动小说的记录底稿，不是一次性简介。
- 世界记录必须严格建立在“世界记录种子”事实上。
- 角色记录必须严格建立在“角色记录种子”事实上，并自然解释角色为何会被卷入这个故事。
- 世界记录重点写清世界如何运转、现实压力从何而来，以及什么样的秩序正在支配众人。
- 角色记录重点写清欲望、弱点、行动倾向，以及六项属性如何转化为行为习惯与判断方式。
- 语气可以带有“记录、共鸣、显影、回响”的阿卡夏感，但不要堆砌术语。
  `;
}

export function generateProfiles(character: Character, world: World) {
  const prompt = buildGenerateProfilesPrompt(character, world);
  return requestJson<GeneratedProfiles>(withApiOrigin('/api/profiles/generate'), {
    method: 'POST',
    body: JSON.stringify({ prompt }),
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

export function cloneGameSession(sessionId: string) {
  return requestJson<GameSessionWorldStateData>(
    withApiOrigin(`/api/game-sessions/${encodeURIComponent(sessionId)}/clone`),
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
