import type {
  Character,
  RuntimeStateView,
  World,
} from '../lib/api';
import {
  createRoundState,
  initialInternalState,
  type GameInternalState,
  useGameInternalStore,
} from './gameStore';
import { STREAM_PLACEHOLDER_TEXT } from './gameStoreHelpers';

const OPENING_ROUND = 1;
const FIRST_ROUND_READY_TIMEOUT_MS = 45000;

export function createOpeningInternalState(sessionId: string): GameInternalState {
  return {
    ...initialInternalState,
    sessionId,
    displayRound: OPENING_ROUND,
    roundStates: {
      [OPENING_ROUND]: createRoundState(OPENING_ROUND, {
        title: '第一轮',
        isAwaitingNarration: true,
      }),
    },
  };
}

export function createOpeningStateView(
  character: Character,
  world: World,
): RuntimeStateView {
  return {
    gameState: 'playing',
    phase: 'booting',
    turnIndex: 0,
    activeTurnId: 0,
    currentLocation: '记录现场',
    currentScene: '记录共鸣中',
    protagonistState: `${character.name || '无名旅人'} 正踏入 ${world.era} 的记录`,
    npcsState: '诸多回响正在汇聚',
    latestHistory: STREAM_PLACEHOLDER_TEXT,
    latestBroadcastSummary: '阿卡夏记录已开始共鸣，开场正在显影...',
    latestBroadcastItems: ['阿卡夏记录已开始共鸣，开场正在显影...'],
    latestProtagonistAction: '你还没有写下选择',
    isEnding: false,
    endingType: null,
  };
}

export function waitForRoundNarrationStarted(
  sessionId: string,
  round = OPENING_ROUND,
) {
  return new Promise<void>((resolve, reject) => {
    const hasStarted = () => {
      const internalState = useGameInternalStore.getState();
      if (internalState.sessionId !== sessionId) {
        return false;
      }

      const roundState = internalState.roundStates[round];
      return Boolean(roundState?.narrationText.trim());
    };

    if (hasStarted()) {
      resolve();
      return;
    }

    let unsubscribe = () => {};
    const timeoutId = window.setTimeout(() => {
      unsubscribe();
      reject(new Error('开场记录比预想中更慢一些，请再试一次。'));
    }, FIRST_ROUND_READY_TIMEOUT_MS);

    unsubscribe = useGameInternalStore.subscribe((state) => {
      const roundState = state.roundStates[round];
      if (
        state.sessionId === sessionId
        && roundState?.narrationText.trim()
      ) {
        window.clearTimeout(timeoutId);
        unsubscribe();
        resolve();
      }
    });
  });
}
