export const appRoutes = {
  lobby: '/',
  archives: '/archives',
  creation: '/creation',
  generating: '/generating',
  gameplay: '/play',
  storyline: '/storyline',
  ending: '/ending',
} as const;

export const SESSION_ID_QUERY_KEY = 'session_id';
export const SHARE_MODE_QUERY_KEY = 'share';
export const CLONE_SHARE_MODE = 'clone';
export const SHARE_ROUND_QUERY_KEY = 'round';
export const VIEW_MODE_QUERY_KEY = 'view';
export const STORY_REVIEW_VIEW_MODE = 'story';
export const FOCUS_ROUND_QUERY_KEY = 'focus_round';

export function readSessionIdFromSearch(search: string): string | null {
  const sessionId = new URLSearchParams(search).get(SESSION_ID_QUERY_KEY)?.trim();
  return sessionId || null;
}

export function isCloneShareSearch(search: string): boolean {
  return new URLSearchParams(search).get(SHARE_MODE_QUERY_KEY) === CLONE_SHARE_MODE;
}

export function readShareRoundFromSearch(search: string): number | null {
  const rawRound = new URLSearchParams(search).get(SHARE_ROUND_QUERY_KEY);
  if (!rawRound) {
    return null;
  }

  const round = Number(rawRound);
  return Number.isSafeInteger(round) && round > 0 ? round : null;
}

export function readFocusRoundFromSearch(search: string): number | null {
  const rawRound = new URLSearchParams(search).get(FOCUS_ROUND_QUERY_KEY);
  if (!rawRound) {
    return null;
  }

  const round = Number(rawRound);
  return Number.isSafeInteger(round) && round > 0 ? round : null;
}

export function isStoryReviewSearch(search: string): boolean {
  return new URLSearchParams(search).get(VIEW_MODE_QUERY_KEY) === STORY_REVIEW_VIEW_MODE;
}

export function routeWithSession(route: string, sessionId: string): string {
  const search = new URLSearchParams({
    [SESSION_ID_QUERY_KEY]: sessionId,
  });

  return `${route}?${search.toString()}`;
}

export function routeWithClonedSession(
  route: string,
  sessionId: string,
  sourceRound?: number | null,
): string {
  const search = new URLSearchParams({
    [SESSION_ID_QUERY_KEY]: sessionId,
    [SHARE_MODE_QUERY_KEY]: CLONE_SHARE_MODE,
  });
  if (sourceRound != null) {
    search.set(SHARE_ROUND_QUERY_KEY, String(sourceRound));
  }

  return `${route}?${search.toString()}`;
}

export function routeWithStoryReviewSession(route: string, sessionId: string): string {
  const search = new URLSearchParams({
    [SESSION_ID_QUERY_KEY]: sessionId,
    [VIEW_MODE_QUERY_KEY]: STORY_REVIEW_VIEW_MODE,
  });

  return `${route}?${search.toString()}`;
}

export function routeWithFocusedRound(route: string, sessionId: string, round: number): string {
  const search = new URLSearchParams({
    [SESSION_ID_QUERY_KEY]: sessionId,
    [FOCUS_ROUND_QUERY_KEY]: String(round),
  });

  return `${route}?${search.toString()}`;
}
