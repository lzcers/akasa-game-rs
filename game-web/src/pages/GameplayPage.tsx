import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useLocation, useNavigate } from 'react-router-dom';
import { useGameInternalStore } from '../store/gameStore';
import { useGameUIStore } from '../store/gameUIStore';
import { useGameValueStore } from '../store/gameValueStore';
import {
  ScreenShell,
  StoryFrame,
} from '../components/AkashicUI';
import GameplayHeader from '../components/GameplayHeader';
import ChoicePanel from '../components/ChoicePanel';
import GameplayToolbar from '../components/GameplayToolbar';
import NarrationPanel from '../components/NarrationPanel';
import type { NarrationRoundEntry } from '../components/gameplayTypes';
import { appRoutes, isStoryReviewSearch, routeWithClonedSession } from '../lib/appRoutes';
import { track } from '../lib/analytics';
import { suppressSessionRestore } from '../lib/sessionRestore';
import type { Choice } from '../lib/api';

const EMPTY_BROADCAST_ITEMS: string[] = [];
const AUTO_CHOICE_STORAGE_KEY = 'akasa:auto-choice-enabled';

const GameplayPage: React.FC = () => {
  const navigate = useNavigate();
  const location = useLocation();
  const {
    phase,
    turnIndex,
    currentScene,
    latestBroadcastItems,
    latestBroadcastSummary,
    isEnding,
    isLoading,
    error,
    skipRestoredNarrationAnimation,
  } = useGameUIStore(useShallow((state) => ({
    phase: state.stateView?.phase ?? '',
    turnIndex: state.stateView?.turnIndex,
    currentScene: state.stateView?.currentScene ?? '',
    latestBroadcastItems: state.stateView?.latestBroadcastItems ?? EMPTY_BROADCAST_ITEMS,
    latestBroadcastSummary: state.stateView?.latestBroadcastSummary ?? '',
    isEnding: state.stateView?.isEnding ?? false,
    isLoading: state.isLoading,
    error: state.error,
    skipRestoredNarrationAnimation: state.skipRestoredNarrationAnimation,
  })));
  const {
    bootstrapSession,
    createSave,
    submitChoice,
    resetGame,
  } = useGameUIStore(useShallow((state) => ({
    bootstrapSession: state.bootstrapSession,
    createSave: state.createSave,
    submitChoice: state.submitChoice,
    resetGame: state.resetGame,
  })));
  const {
    obsessionPoints,
    intuitionPoints,
    consumeIntuition,
  } = useGameValueStore(useShallow((state) => ({
    obsessionPoints: state.obsessionPoints,
    intuitionPoints: state.intuitionPoints,
    consumeIntuition: state.consumeIntuition,
  })));
  const sessionId = useGameInternalStore((state) => state.sessionId);
  const displayRound = useGameInternalStore((state) => state.displayRound);
  const roundStates = useGameInternalStore((state) => state.roundStates);
  const [completedTypingKey, setCompletedTypingKey] = useState<string | null>(null);
  const [autoChoiceEnabled, setAutoChoiceEnabled] = useState(() => (
    import.meta.env.DEV && window.localStorage.getItem(AUTO_CHOICE_STORAGE_KEY) === '1'
  ));
  const [roundControls, setRoundControls] = useState<{
    round: number;
    activeObsession: boolean;
    obsessionInput: string;
    previews: Record<string, string>;
  }>({
    round: 1,
    activeObsession: false,
    obsessionInput: '',
    previews: {},
  });
  const [feedback, setFeedback] = useState<string | null>(null);
  const autoChoiceKeyRef = useRef<string | null>(null);
  const reachedRoundKeyRef = useRef<string | null>(null);

  const currentRound = Math.max(displayRound || turnIndex || 1, 1);
  const narrationHistory = useMemo<NarrationRoundEntry[]>(() => (
    Object.values(roundStates)
      .filter((entry) => entry.narrationText || entry.selectedChoiceText || entry.isAwaitingNarration)
      .sort((left, right) => left.round - right.round)
  ), [roundStates]);
  const activeRoundState = roundStates[currentRound];
  const isEndingReviewMode = isStoryReviewSearch(location.search) && (phase === 'ended' || isEnding);
  const hasCurrentRoundControls = roundControls.round === currentRound;
  const activeObsession = hasCurrentRoundControls ? roundControls.activeObsession : false;
  const obsessionInput = hasCurrentRoundControls ? roundControls.obsessionInput : '';
  const previews = hasCurrentRoundControls ? roundControls.previews : {};
  const currentRoundChoices = useMemo(
    () => activeRoundState?.choices ?? [],
    [activeRoundState?.choices],
  );
  const hasChoices = currentRoundChoices.length > 0;
  const isNarrationStreaming = activeRoundState?.narrationStatus === 'pending'
    || activeRoundState?.narrationStatus === 'running';
  const shouldType = Boolean(activeRoundState?.isAwaitingNarration) || isNarrationStreaming;
  const typingKey = `${currentRound}:${activeRoundState?.isAwaitingNarration ? '1' : '0'}:${activeRoundState?.narrationText ?? ''}`;
  const isTyping = shouldType && completedTypingKey !== typingKey;
  const isChoiceInteractionDisabled = isEndingReviewMode || isTyping || isLoading;
  const canContinueWithoutChoice = phase === 'awaiting_player'
    && activeRoundState?.choicesStatus === 'ready'
    && currentRoundChoices.length === 0
    && !isNarrationStreaming
    && !isEndingReviewMode;
  const isObsessionToggleDisabled = isChoiceInteractionDisabled || !hasChoices || obsessionPoints <= 0;
  const isObsessionSubmitDisabled = isChoiceInteractionDisabled || obsessionInput.trim().length === 0;
  const latestCompletedNarration = useMemo(() => (
    [...narrationHistory]
      .reverse()
      .find((entry) => (
        entry.narrationText.trim()
        && !entry.isAwaitingNarration
        && entry.narrationStatus !== 'pending'
        && entry.narrationStatus !== 'running'
      ))
  ), [narrationHistory]);
  const canArchiveLatestCompletedRound = Boolean(sessionId && latestCompletedNarration);
  const archiveActionUnavailableReason = canArchiveLatestCompletedRound
    ? null
    : '第一段回响显影后可分享或封存。';
  const archiveActionKey = `${sessionId ?? 'no-session'}:${latestCompletedNarration?.round ?? 'no-completed-round'}`;
  const statusMessage = feedback ?? error;
  const broadcastItems = latestBroadcastItems
    .map((item) => item.trim())
    .filter(Boolean);
  const broadcastMessages = broadcastItems.length > 0
    ? broadcastItems
    : (latestBroadcastSummary.trim() ? [latestBroadcastSummary.trim()] : []);
  const shareSummaryFallback = useMemo(() => {
    const latestNarration = latestCompletedNarration?.narrationText.trim();
    const broadcastSummary = latestBroadcastSummary.trim();

    if (latestNarration && broadcastSummary && !latestNarration.includes(broadcastSummary)) {
      return `${broadcastSummary} ${latestNarration}`;
    }

    if (latestNarration) {
      return latestNarration;
    }

    if (broadcastSummary) {
      return broadcastSummary;
    }

    return `${currentScene} 的记录仍在回响，下一轮选择正在逼近。`;
  }, [currentScene, latestBroadcastSummary, latestCompletedNarration]);
  const shareGameUrl = useMemo(() => (
    new URL(
      sessionId ? routeWithClonedSession(appRoutes.gameplay, sessionId) : appRoutes.lobby,
      window.location.origin,
    ).toString()
  ), [sessionId]);

  useEffect(() => {
    if (!feedback) return undefined;

    const timer = window.setTimeout(() => setFeedback(null), 2200);
    return () => window.clearTimeout(timer);
  }, [feedback]);

  useEffect(() => {
    if (!import.meta.env.DEV) {
      return;
    }

    window.localStorage.setItem(AUTO_CHOICE_STORAGE_KEY, autoChoiceEnabled ? '1' : '0');
  }, [autoChoiceEnabled]);

  useEffect(() => {
    if (!sessionId || !hasChoices || isNarrationStreaming) {
      return;
    }

    const reachedRoundKey = `${sessionId}:${currentRound}`;
    if (reachedRoundKeyRef.current === reachedRoundKey) {
      return;
    }

    reachedRoundKeyRef.current = reachedRoundKey;
    track('round_reached', {
      round: currentRound,
    });
  }, [currentRound, hasChoices, isNarrationStreaming, sessionId]);

  const handleTypewriterComplete = useCallback(() => {
    setCompletedTypingKey(typingKey);
  }, [typingKey]);

  const readErrorMessage = useCallback((cause: unknown, fallback: string) => {
    return cause instanceof Error ? cause.message : fallback;
  }, []);

  useEffect(() => {
    if (!sessionId || phase !== 'booting') {
      return;
    }

    void bootstrapSession().catch((bootstrapError) => {
      setFeedback(readErrorMessage(bootstrapError, '开场记录启动失败。'));
    });
  }, [bootstrapSession, phase, readErrorMessage, sessionId]);

  const handlePreview = async (choice: Choice, e: React.MouseEvent) => {
    e.stopPropagation();

    if (previews[choice.id]) {
      setFeedback(previews[choice.id]);
      return;
    }

    try {
      const motivationAndRisk = choice.motivationAndRisk?.trim();
      if (!motivationAndRisk) {
        throw new Error('这条分支暂时没有可窥见的记录碎片。');
      }
      consumeIntuition();
      track('intuition_preview_used');

      setRoundControls((prev) => ({
        round: currentRound,
        activeObsession: prev.round === currentRound ? prev.activeObsession : false,
        obsessionInput: prev.round === currentRound ? prev.obsessionInput : '',
        previews: {
          ...(prev.round === currentRound ? prev.previews : {}),
          [choice.id]: motivationAndRisk,
        },
      }));
      setFeedback(motivationAndRisk);
    } catch (previewError) {
      setFeedback(readErrorMessage(previewError, '记录窥见失败。'));
    }
  };

  const handleChoiceClick = useCallback(async (choice: Choice) => {
    try {
      await submitChoice({
        input: {
          type: 'selected_option',
          action: choice.action,
        },
        displayText: choice.text,
      }, activeObsession);
      setRoundControls({
        round: currentRound,
        activeObsession: false,
        obsessionInput: '',
        previews: {},
      });
      setFeedback(null);
    } catch (submitError) {
      setFeedback(readErrorMessage(submitError, '推进回响失败。'));
    }
  }, [activeObsession, currentRound, readErrorMessage, submitChoice]);

  const handleContinueClick = useCallback(async () => {
    try {
      await submitChoice({
        input: {
          type: 'free_text',
          action: 'continue',
        },
        displayText: '继续回响',
      });
      setRoundControls({
        round: currentRound,
        activeObsession: false,
        obsessionInput: '',
        previews: {},
      });
      setFeedback(null);
    } catch (submitError) {
      setFeedback(readErrorMessage(submitError, '续写回响失败。'));
    }
  }, [currentRound, readErrorMessage, submitChoice]);

  useEffect(() => {
    if (!autoChoiceEnabled || isEndingReviewMode) {
      autoChoiceKeyRef.current = null;
      return undefined;
    }

    if (activeObsession || !hasChoices || isChoiceInteractionDisabled) {
      return undefined;
    }

    const nextChoice = currentRoundChoices.find((choice) => !choice.disabled);

    if (!nextChoice) {
      return undefined;
    }

    const autoChoiceKey = [
      sessionId ?? 'no-session',
      currentRound,
      currentRoundChoices.map((choice) => `${choice.id}:${choice.action}`).join('|'),
    ].join(':');

    if (autoChoiceKeyRef.current === autoChoiceKey) {
      return undefined;
    }

    const timer = window.setTimeout(() => {
      autoChoiceKeyRef.current = autoChoiceKey;
      void handleChoiceClick(nextChoice);
    }, 450);

    return () => window.clearTimeout(timer);
  }, [
    activeObsession,
    autoChoiceEnabled,
    currentRound,
    currentRoundChoices,
    handleChoiceClick,
    hasChoices,
    isChoiceInteractionDisabled,
    isEndingReviewMode,
    sessionId,
  ]);

  const handleObsessionSubmit = async (actionText: string) => {
    if (!actionText) {
      setFeedback('请先写下这次想写入记录的执念。');
      return;
    }

    try {
      await submitChoice({
        input: {
          type: 'free_text',
          action: actionText,
        },
        displayText: actionText,
      }, true);
      setRoundControls({
        round: currentRound,
        activeObsession: false,
        obsessionInput: '',
        previews: {},
      });
      setFeedback(null);
    } catch (submitError) {
      setFeedback(readErrorMessage(submitError, '执念写入失败。'));
    }
  };

  const handleSave = async () => {
    if (!canArchiveLatestCompletedRound) {
      setFeedback(archiveActionUnavailableReason);
      return;
    }

    try {
      await createSave();
      setFeedback('这一段记录已封存');
    } catch (saveError) {
      setFeedback(readErrorMessage(saveError, '封存失败。'));
    }
  };

  return (
    <ScreenShell className="items-stretch">
      <StoryFrame className="relative flex max-w-5xl flex-col overflow-hidden px-2.5 py-2.5 sm:px-3 sm:py-3 md:px-4 md:py-4">
        <div className="pointer-events-none absolute inset-0 bg-linear-to-b from-transparent via-[#08111d]/35 to-[#08111d]" />
        <div className="relative z-10 flex min-h-0 flex-1 flex-col gap-3">
          <GameplayHeader
            currentRound={currentRound}
            currentScene={currentScene}
            isLoading={isLoading}
            broadcastMessages={broadcastMessages}
            statusLabel={isEndingReviewMode ? '记录完结' : undefined}
          />

          <div className="flex min-h-0 flex-1 flex-col gap-3">
            <NarrationPanel
              narrationHistory={narrationHistory}
              currentRound={currentRound}
              isAwaitingNarration={Boolean(activeRoundState?.isAwaitingNarration)}
              skipRestoredNarrationAnimation={skipRestoredNarrationAnimation || isEndingReviewMode}
              onTypewriterComplete={handleTypewriterComplete}
            />
            <div className="min-h-5">
              {statusMessage ? <p className="text-xs text-[#d9cbb1] sm:text-sm">{statusMessage}</p> : null}
            </div>
            <div className="mt-auto flex touch-pan-y flex-col gap-2">
              {!isEndingReviewMode ? (
                <ChoicePanel
                  hasChoices={hasChoices}
                  canContinue={canContinueWithoutChoice}
                  choices={currentRoundChoices}
                  previews={previews}
                  remainingIntuitionPoints={intuitionPoints}
                  activeObsession={activeObsession}
                  obsessionInput={obsessionInput}
                  autoChoiceEnabled={autoChoiceEnabled}
                  showAutoChoiceToggle={import.meta.env.DEV}
                  isChoiceInteractionDisabled={isChoiceInteractionDisabled}
                  isObsessionSubmitDisabled={isObsessionSubmitDisabled}
                  onChoiceClick={handleChoiceClick}
                  onContinue={handleContinueClick}
                  onAutoChoiceToggle={setAutoChoiceEnabled}
                  onPreview={handlePreview}
                  onObsessionInputChange={(nextValue) => {
                    setRoundControls((prev) => ({
                      round: currentRound,
                      activeObsession: prev.round === currentRound ? prev.activeObsession : false,
                      obsessionInput: nextValue,
                      previews: prev.round === currentRound ? prev.previews : {},
                    }));
                  }}
                  onObsessionSubmit={handleObsessionSubmit}
                />
              ) : null}
              <GameplayToolbar
                isReadOnly={isEndingReviewMode}
                activeObsession={!isEndingReviewMode && activeObsession}
                isObsessionToggleDisabled={isObsessionToggleDisabled}
                obsessionPoints={obsessionPoints}
                intuitionPoints={intuitionPoints}
                sessionId={sessionId}
                shareSummaryFallback={shareSummaryFallback}
                shareGameUrl={shareGameUrl}
                archiveActionKey={archiveActionKey}
                isArchiveActionDisabled={!canArchiveLatestCompletedRound}
                archiveActionUnavailableReason={archiveActionUnavailableReason}
                onToggleObsession={() => {
                  if (isEndingReviewMode) {
                    return;
                  }
                  setRoundControls((prev) => ({
                    round: currentRound,
                    activeObsession: prev.round === currentRound ? !prev.activeObsession : true,
                    obsessionInput: prev.round === currentRound ? prev.obsessionInput : '',
                    previews: prev.round === currentRound ? prev.previews : {},
                  }));
                  setFeedback(null);
                }}
                onBackToLobby={() => {
                  suppressSessionRestore(sessionId);
                  navigate(appRoutes.lobby, { replace: true });
                  resetGame();
                }}
                onSave={handleSave}
              />
            </div>
          </div>
        </div>
      </StoryFrame>
    </ScreenShell>
  );
};

export default GameplayPage;
