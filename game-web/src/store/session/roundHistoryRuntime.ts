import { getGameSessionRounds } from '../../lib/api';
import { useGameInternalStore } from '../gameStore';
import { roundStateFromPersistedHistoryEntry } from './mappers';

const SESSION_ROUNDS_PAGE_LIMIT = 100;
let roundHistoryLoadVersion = 0;

interface LoadCompleteSessionRoundsOptions {
  replaceTimeline?: boolean;
}

export async function loadCompleteSessionRounds(
  sessionId: string,
  options: LoadCompleteSessionRoundsOptions = {},
): Promise<void> {
  const loadVersion = ++roundHistoryLoadVersion;
  let beforeRound: number | null | undefined = null;
  let isFirstPage = true;

  do {
    const page = await getGameSessionRounds(sessionId, {
      beforeRound,
      limit: SESSION_ROUNDS_PAGE_LIMIT,
    });

    if (
      loadVersion !== roundHistoryLoadVersion
      || useGameInternalStore.getState().sessionId !== sessionId
    ) {
      return;
    }

    useGameInternalStore.setState((state) => {
      if (
        loadVersion !== roundHistoryLoadVersion
        || state.sessionId !== sessionId
      ) {
        return state;
      }

      const roundStates =
        options.replaceTimeline && isFirstPage ? {} : { ...state.roundStates };
      for (const entry of page.rounds) {
        roundStates[entry.round] = roundStateFromPersistedHistoryEntry(entry);
      }

      return { roundStates };
    });

    isFirstPage = false;
    beforeRound = page.nextBeforeRound;
    if (!page.hasMore) {
      return;
    }
  } while (beforeRound != null);
}
