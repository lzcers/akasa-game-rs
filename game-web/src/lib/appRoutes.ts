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
export const SHARE_NODE_QUERY_KEY = 'node_id';
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

export function readShareNodeIdFromSearch(search: string): string | null {
  const nodeId = new URLSearchParams(search).get(SHARE_NODE_QUERY_KEY)?.trim();
  return nodeId || null;
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
  sourceNodeId?: string | null,
): string {
  const search = new URLSearchParams({
    [SESSION_ID_QUERY_KEY]: sessionId,
    [SHARE_MODE_QUERY_KEY]: CLONE_SHARE_MODE,
  });
  const normalizedNodeId = sourceNodeId?.trim();
  if (normalizedNodeId) {
    search.set(SHARE_NODE_QUERY_KEY, normalizedNodeId);
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
