import { create } from 'zustand';
import type {
  Character,
  GeneratedProfiles,
  PlayerActionInput,
  RuntimeStateView,
  StoryPreferences,
  World,
} from '../lib/api';
import {
  initialUIState,
} from './ui/initialState';
import {
  bindGameUIStoreAccess,
} from './ui/runtimeBindings';
import { createGameUIActions } from './ui/actionsRuntime';

export type StartupStage =
  | 'idle'
  | 'generating_world'
  | 'generating_character'
  | 'ready_to_enter'
  | 'creating_session';

export interface GameUIState {
  // 角色设定表单与存档摘要会读取的人物信息。
  character: Character;
  // 世界设定表单与存档摘要会读取的世界信息。
  world: World;
  // 故事设定表单会读取的叙事偏好与禁区。
  story: StoryPreferences;
  // 运行时视图模型，驱动右侧状态面板等聚合信息。
  stateView: RuntimeStateView | null;
  // 全局加载态，控制按钮禁用、骨架屏等。
  isLoading: boolean;
  // 开局前过渡页当前聚焦的阶段。
  startupStage: StartupStage;
  // 已生成但尚未正式注入会话的世界/玩家角色设定。
  preparedProfiles: GeneratedProfiles | null;
  // 当前会话生成页显影出的记录文案，用于故事内回看。
  generatedProfiles: GeneratedProfiles | null;
  // 全局错误消息。
  error: string | null;
  // 从存档恢复后，当前已存在叙事应直接展示，不再重新打字。
  skipRestoredNarrationAnimation: boolean;
}

export interface GameUIActions {
  // 操作：更新角色设定。
  updateCharacter: (updates: Partial<Character>) => void;
  // 操作：更新世界设定。
  updateWorld: (updates: Partial<World>) => void;
  // 操作：更新故事设定。
  updateStory: (updates: Partial<StoryPreferences>) => void;
  // 操作：清除错误提示。
  clearError: () => void;
  // 操作：生成设定并进入过渡页。
  startGame: () => Promise<void>;
  // 操作：基于已生成设定正式创建会话并进入游戏。
  enterWorld: () => Promise<{ sessionId: string } | null>;
  // 操作：在进入游玩页后触发开场叙事。
  bootstrapSession: () => Promise<void>;
  // 操作：提交当前选择；执念模式下也可直接提交自定义行动文本。
  submitChoice: (
    submission: { input: PlayerActionInput; displayText: string },
    useObsession?: boolean,
  ) => Promise<void>;
  // 操作：从历史节点创建/切换回溯分支并提交所选行动。
  backtrackChoice: (
    sourceRound: number,
    submission: { input: PlayerActionInput; displayText: string },
  ) => Promise<void>;
  // 操作：创建当前进度的存档。
  createSave: (title?: string) => Promise<string>;
  // 操作：加载指定存档。
  loadSave: (saveId: string) => Promise<{ sessionId: string }>;
  // 操作：通过仍然存活的后端会话 id 恢复当前进度。
  restoreSession: (sessionId: string) => Promise<void>;
  // 操作：基于分享链接复制一份独立会话并切换到该分支。
  cloneSharedSession: (sourceSessionId: string, sourceRound?: number | null) => Promise<{ sessionId: string; isEnding: boolean }>;
  // 操作：重置本地游戏状态并关闭流连接。
  resetGame: () => void;
}

export type GameUIStoreState = GameUIState & GameUIActions;

export const useGameUIStore = create<GameUIStoreState>((set, get) => {
  bindGameUIStoreAccess(set, get);
  return {
    ...initialUIState,
    ...createGameUIActions(set, get),
  };
});
