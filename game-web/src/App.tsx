import { useEffect } from 'react';
import { Navigate, Route, Routes, useNavigate } from 'react-router-dom';
import { appRoutes } from './lib/appRoutes';
import { installNavigator } from './lib/navigation';
import LobbyPage from './pages/LobbyPage';
import ArchiveListPage from './pages/ArchiveListPage';
import CreationPage from './pages/CreationPage';
import { useGameInternalStore } from './store/gameStore';
import {
  captureAttribution,
  setAnalyticsGameSessionId,
  track,
} from './lib/analytics';
import { EndingRoute, GameplayRoute, GeneratingRoute, StorylineRoute } from './routes/GameSessionRoutes';

function NavigationBridge() {
  const navigate = useNavigate();

  useEffect(() => {
    installNavigator(navigate);
    return () => installNavigator(null);
  }, [navigate]);

  return null;
}

function App() {
  useEffect(() => {
    captureAttribution();
    track('app_opened');
  }, []);

  useEffect(() => {
    const unsubscribe = useGameInternalStore.subscribe((state) => {
      setAnalyticsGameSessionId(state.sessionId);
    });
    setAnalyticsGameSessionId(useGameInternalStore.getState().sessionId);
    return unsubscribe;
  }, []);

  return (
    <div className="relative h-dvh w-full overflow-hidden bg-background">
      <div className="pointer-events-none absolute inset-0 z-0">
        <div className="absolute -left-24 top-12 h-72 w-72 rounded-full bg-sky-500/10 blur-3xl" />
        <div className="absolute bottom-10 -right-16 h-80 w-80 rounded-full bg-indigo-500/10 blur-3xl" />
        <div className="absolute inset-y-0 left-[8%] w-px bg-white/5" />
        <div className="absolute inset-y-0 right-[8%] w-px bg-white/5" />
      </div>

      <main className="akashic-scroll relative z-10 h-full w-full touch-pan-y overflow-y-auto overflow-x-hidden">
        <NavigationBridge />
        <Routes>
          <Route path={appRoutes.lobby} element={<LobbyPage />} />
          <Route path={appRoutes.archives} element={<ArchiveListPage />} />
          <Route path={appRoutes.creation} element={<CreationPage />} />
          <Route path={appRoutes.generating} element={<GeneratingRoute />} />
          <Route path={appRoutes.gameplay} element={<GameplayRoute />} />
          <Route path={appRoutes.storyline} element={<StorylineRoute />} />
          <Route path={appRoutes.ending} element={<EndingRoute />} />
          <Route path="*" element={<Navigate to={appRoutes.lobby} replace />} />
        </Routes>
      </main>
    </div>
  );
}

export default App;
