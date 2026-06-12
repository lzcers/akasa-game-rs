import { type ReactNode, useEffect } from 'react';
import { Navigate, useLocation, useNavigate } from 'react-router-dom';
import {
  appRoutes,
  isCloneShareSearch,
  isStoryReviewSearch,
  readShareRoundFromSearch,
  readSessionIdFromSearch,
  routeWithSession,
} from '../lib/appRoutes';
import { isSessionRestoreSuppressed } from '../lib/sessionRestore';
import EndingPage from '../pages/EndingPage';
import GameplayPage from '../pages/GameplayPage';
import GeneratingPage from '../pages/GeneratingPage';
import StorylinePage from '../pages/StorylinePage';
import { useGameInternalStore } from '../store/gameStore';
import { useGameUIStore } from '../store/gameUIStore';

export function GeneratingRoute() {
  const sessionId = useGameInternalStore((state) => state.sessionId);
  const stateView = useGameUIStore((state) => state.stateView);
  const startupStage = useGameUIStore((state) => state.startupStage);
  const preparedProfiles = useGameUIStore((state) => state.preparedProfiles);

  if (sessionId && stateView && startupStage === 'idle') {
    return <Navigate to={routeWithSession(appRoutes.gameplay, sessionId)} replace />;
  }

  if (startupStage === 'idle' && !preparedProfiles) {
    return <Navigate to={appRoutes.creation} replace />;
  }

  return <GeneratingPage />;
}

function SessionRestoreGate({ sessionId }: { sessionId: string }) {
  const restoreSession = useGameUIStore((state) => state.restoreSession);
  const isLoading = useGameUIStore((state) => state.isLoading);
  const error = useGameUIStore((state) => state.error);

  useEffect(() => {
    void restoreSession(sessionId).catch(() => {
      // The store keeps the user-facing error and navigates back to the lobby.
    });
  }, [restoreSession, sessionId]);

  return (
    <div className="flex h-full w-full items-center justify-center px-6 text-center">
      <div className="max-w-md space-y-3 rounded-[1.1rem] border border-[#6d86b7]/25 bg-[#101827]/78 px-5 py-5 text-[#efe4cd] shadow-[0_10px_30px_rgba(3,8,18,0.25)]">
        <p className="text-sm font-semibold tracking-[0.18em] text-[#8fa4ca]">记录续接</p>
        <p className="text-lg font-medium">
          {isLoading ? '正在续接这段记录...' : '正在打开记录...'}
        </p>
        {error ? <p className="text-sm leading-6 text-[#ffd7d7]">{error}</p> : null}
      </div>
    </div>
  );
}

function SessionCloneGate({
  sourceSessionId,
  sourceRound,
}: {
  sourceSessionId: string;
  sourceRound: number | null;
}) {
  const cloneSharedSession = useGameUIStore((state) => state.cloneSharedSession);
  const isLoading = useGameUIStore((state) => state.isLoading);
  const error = useGameUIStore((state) => state.error);
  const navigate = useNavigate();

  useEffect(() => {
    void cloneSharedSession(sourceSessionId, sourceRound)
      .then((cloned) => {
        navigate(
          routeWithSession(
            cloned.isEnding ? appRoutes.ending : appRoutes.gameplay,
            cloned.sessionId,
          ),
          { replace: true },
        );
      })
      .catch(() => {
        // The store keeps the user-facing error and navigates back to the lobby.
      });
  }, [cloneSharedSession, navigate, sourceRound, sourceSessionId]);

  return (
    <div className="flex h-full w-full items-center justify-center px-6 text-center">
      <div className="max-w-md space-y-3 rounded-[1.1rem] border border-[#6d86b7]/25 bg-[#101827]/78 px-5 py-5 text-[#efe4cd] shadow-[0_10px_30px_rgba(3,8,18,0.25)]">
        <p className="text-sm font-semibold tracking-[0.18em] text-[#8fa4ca]">共鸣分支</p>
        <p className="text-lg font-medium">
          {isLoading ? '正在复制这段记录...' : '正在打开分享记录...'}
        </p>
        <p className="text-sm leading-6 text-[#a8b4c7]">
          会为你生成一条独立共鸣分支，原玩家的记录不会被改变。
        </p>
        {error ? <p className="text-sm leading-6 text-[#ffd7d7]">{error}</p> : null}
      </div>
    </div>
  );
}

function SessionRouteGuard({
  route,
  children,
}: {
  route: typeof appRoutes.gameplay | typeof appRoutes.ending | typeof appRoutes.storyline;
  children: ReactNode;
}) {
  const location = useLocation();
  const requestedSessionId = readSessionIdFromSearch(location.search);
  const requestedShareRound = readShareRoundFromSearch(location.search);
  const shouldCloneSession = isCloneShareSearch(location.search);
  const shouldReviewEndedStory = isStoryReviewSearch(location.search);
  const sessionId = useGameInternalStore((state) => state.sessionId);
  const stateView = useGameUIStore((state) => state.stateView);

  if (requestedSessionId && shouldCloneSession) {
    return (
      <SessionCloneGate
        sourceSessionId={requestedSessionId}
        sourceRound={requestedShareRound}
      />
    );
  }

  if (isSessionRestoreSuppressed(requestedSessionId)) {
    return <Navigate to={appRoutes.lobby} replace />;
  }

  if (requestedSessionId && (sessionId !== requestedSessionId || !stateView)) {
    return <SessionRestoreGate sessionId={requestedSessionId} />;
  }

  if (!sessionId || !stateView) {
    return <Navigate to={appRoutes.lobby} replace />;
  }

  if (!requestedSessionId) {
    return <Navigate to={routeWithSession(route, sessionId)} replace />;
  }

  if (
    route === appRoutes.gameplay
    && (stateView.phase === 'ended' || stateView.flowEnd)
    && !shouldReviewEndedStory
  ) {
    return <Navigate to={routeWithSession(appRoutes.ending, sessionId)} replace />;
  }

  if (
    route === appRoutes.ending
    && stateView.phase !== 'ended'
    && !stateView.flowEnd
    && !stateView.isEnding
  ) {
    return <Navigate to={routeWithSession(appRoutes.gameplay, sessionId)} replace />;
  }

  return children;
}

export function GameplayRoute() {
  return (
    <SessionRouteGuard route={appRoutes.gameplay}>
      <GameplayPage />
    </SessionRouteGuard>
  );
}

export function EndingRoute() {
  return (
    <SessionRouteGuard route={appRoutes.ending}>
      <EndingPage />
    </SessionRouteGuard>
  );
}

export function StorylineRoute() {
  return (
    <SessionRouteGuard route={appRoutes.storyline}>
      <StorylinePage />
    </SessionRouteGuard>
  );
}
