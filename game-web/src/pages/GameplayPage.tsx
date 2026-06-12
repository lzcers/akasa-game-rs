import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useShallow } from "zustand/react/shallow";
import { useLocation, useNavigate } from "react-router-dom";
import { useGameInternalStore } from "../store/gameStore";
import { useGameUIStore } from "../store/gameUIStore";
import { useGameValueStore } from "../store/gameValueStore";
import { loadCompleteSessionRounds } from "../store/session/roundHistoryRuntime";
import { ScreenShell, StoryFrame } from "../components/AkashicUI";
import ChoicePanel from "../components/ChoicePanel";
import GameplayToolbar from "../components/GameplayToolbar";
import NarrationPanel from "../components/NarrationPanel";
import type { NarrationRoundEntry } from "../components/gameplayTypes";
import {
  appRoutes,
  isStoryReviewSearch,
  routeWithClonedSession,
  routeWithSession,
} from "../lib/appRoutes";
import { track } from "../lib/analytics";
import { suppressSessionRestore } from "../lib/sessionRestore";
import type { Choice } from "../lib/api";

const EMPTY_BROADCAST_ITEMS: string[] = [];
const AUTO_CHOICE_STORAGE_KEY = "akasa:auto-choice-enabled";

interface SubmittedChoiceDisplay {
  action: string;
  text: string;
}

interface SubmittedChoiceState {
  choices: Record<number, SubmittedChoiceDisplay>;
  sessionId: string | null;
}

const GameplayPage: React.FC = () => {
  const navigate = useNavigate();
  const location = useLocation();
  const {
    phase,
    turnIndex,
    latestBroadcastItems,
    latestBroadcastSummary,
    generatedProfiles,
    characterName,
    isEnding,
    isLoading,
    error,
    skipRestoredNarrationAnimation,
  } = useGameUIStore(
    useShallow((state) => ({
      phase: state.stateView?.phase ?? "",
      turnIndex: state.stateView?.turnIndex,
      latestBroadcastItems:
        state.stateView?.latestBroadcastItems ?? EMPTY_BROADCAST_ITEMS,
      latestBroadcastSummary: state.stateView?.latestBroadcastSummary ?? "",
      generatedProfiles: state.generatedProfiles,
      characterName: state.character.name,
      isEnding: state.stateView?.isEnding ?? false,
      isLoading: state.isLoading,
      error: state.error,
      skipRestoredNarrationAnimation: state.skipRestoredNarrationAnimation,
    })),
  );
  const { bootstrapSession, cloneSharedSession, createSave, submitChoice, resetGame } =
    useGameUIStore(
      useShallow((state) => ({
        bootstrapSession: state.bootstrapSession,
        cloneSharedSession: state.cloneSharedSession,
        createSave: state.createSave,
        submitChoice: state.submitChoice,
        resetGame: state.resetGame,
      })),
    );
  const { obsessionPoints, intuitionPoints, consumeIntuition } =
    useGameValueStore(
      useShallow((state) => ({
        obsessionPoints: state.obsessionPoints,
        intuitionPoints: state.intuitionPoints,
        consumeIntuition: state.consumeIntuition,
      })),
    );
  const sessionId = useGameInternalStore((state) => state.sessionId);
  const displayRound = useGameInternalStore((state) => state.displayRound);
  const roundStates = useGameInternalStore((state) => state.roundStates);
  const [completedTypingKey, setCompletedTypingKey] = useState<string | null>(
    null,
  );
  const [autoChoiceEnabled, setAutoChoiceEnabled] = useState(
    () =>
      import.meta.env.DEV &&
      window.localStorage.getItem(AUTO_CHOICE_STORAGE_KEY) === "1",
  );
  const [roundControls, setRoundControls] = useState<{
    round: number;
    activeObsession: boolean;
    obsessionInput: string;
    previews: Record<string, string>;
  }>({
    round: 1,
    activeObsession: false,
    obsessionInput: "",
    previews: {},
  });
  const [feedback, setFeedback] = useState<string | null>(null);
  const [expandedChoicePanelRound, setExpandedChoicePanelRound] = useState<
    number | null
  >(null);
  const [submittedChoiceState, setSubmittedChoiceState] =
    useState<SubmittedChoiceState>({
      choices: {},
      sessionId: null,
    });
  const autoChoiceKeyRef = useRef<string | null>(null);
  const reachedRoundKeyRef = useRef<string | null>(null);

  const currentRound = Math.max(displayRound || turnIndex || 1, 1);
  const playableCharacterName = characterName.trim() || "玩家角色";
  const submittedChoices = useMemo(
    () =>
      submittedChoiceState.sessionId === sessionId
        ? submittedChoiceState.choices
        : {},
    [sessionId, submittedChoiceState],
  );
  const narrationHistory = useMemo<NarrationRoundEntry[]>(
    () =>
      Object.values(roundStates)
        .filter(
          (entry) =>
            entry.narrationText ||
            entry.selectedChoiceText ||
            submittedChoices[entry.round] ||
            entry.isAwaitingNarration,
        )
        .map((entry) => {
          const submittedChoice = submittedChoices[entry.round];
          if (!submittedChoice || entry.selectedChoiceText) {
            return entry;
          }

          return {
            ...entry,
            selectedChoiceText: submittedChoice.text,
            selectedChoiceAction: submittedChoice.action,
          };
        })
        .sort((left, right) => left.round - right.round),
    [roundStates, submittedChoices],
  );
  const activeRoundState = roundStates[currentRound];
  const choicePanelRound = expandedChoicePanelRound ?? currentRound;
  const choicePanelState = roundStates[choicePanelRound];
  const isBacktrackChoicePanel = choicePanelRound !== currentRound;
  const isEndingReviewMode =
    isStoryReviewSearch(location.search) && (phase === "ended" || isEnding);
  const hasChoicePanelRoundControls = roundControls.round === choicePanelRound;
  const activeObsession = hasChoicePanelRoundControls && !isBacktrackChoicePanel
    ? roundControls.activeObsession
    : false;
  const isChoicePanelCollapsed =
    !isBacktrackChoicePanel && expandedChoicePanelRound !== currentRound;
  const obsessionInput = hasChoicePanelRoundControls
    ? roundControls.obsessionInput
    : "";
  const previews = hasChoicePanelRoundControls ? roundControls.previews : {};
  const currentRoundChoices = useMemo(
    () => activeRoundState?.choices ?? [],
    [activeRoundState?.choices],
  );
  const choicePanelChoices = useMemo(
    () => choicePanelState?.choices ?? [],
    [choicePanelState?.choices],
  );
  const hasChoices = currentRoundChoices.length > 0;
  const hasChoicePanelChoices = choicePanelChoices.length > 0;
  const isNarrationStreaming =
    activeRoundState?.narrationStatus === "running";
  const shouldAnimateCurrentNarration =
    !skipRestoredNarrationAnimation && !isEndingReviewMode;
  const currentNarrationText = activeRoundState?.narrationText ?? "";
  const typingKey = `${currentRound}:${currentNarrationText}`;
  const isCurrentNarrationTyping =
    shouldAnimateCurrentNarration &&
    currentNarrationText.trim().length > 0 &&
    completedTypingKey !== typingKey;
  const isNarrationOutputPending =
    Boolean(activeRoundState?.isAwaitingNarration) ||
    isNarrationStreaming ||
    isCurrentNarrationTyping;
  const isChoiceInteractionDisabled =
    isEndingReviewMode || isNarrationOutputPending || isLoading;
  const isChoicePanelInteractionDisabled =
    isEndingReviewMode ||
    isLoading ||
    (!isBacktrackChoicePanel && isNarrationOutputPending);
  const canContinueWithoutChoice =
    phase === "awaiting_player" &&
    activeRoundState?.choicesStatus === "ready" &&
    currentRoundChoices.length === 0 &&
    !isNarrationStreaming &&
    !isEndingReviewMode;
  const isObsessionToggleDisabled =
    isBacktrackChoicePanel ||
    isChoicePanelInteractionDisabled ||
    !hasChoicePanelChoices ||
    obsessionPoints <= 0;
  const isObsessionSubmitDisabled =
    isBacktrackChoicePanel ||
    isChoicePanelInteractionDisabled ||
    obsessionInput.trim().length === 0;
  const latestCompletedNarration = useMemo(
    () =>
      [...narrationHistory]
        .reverse()
        .find(
          (entry) =>
            entry.narrationText.trim() &&
            !entry.isAwaitingNarration &&
            entry.narrationStatus !== "running",
        ),
    [narrationHistory],
  );
  const canArchiveLatestCompletedRound = Boolean(
    sessionId && latestCompletedNarration,
  );
  const archiveActionUnavailableReason = canArchiveLatestCompletedRound
    ? null
    : "第一段回响显影后可分享或封存。";
  const archiveActionKey = `${sessionId ?? "no-session"}:${latestCompletedNarration?.round ?? "no-completed-round"}`;
  const statusMessage = feedback ?? error;
  const broadcastItems = latestBroadcastItems
    .map((item) => item.trim())
    .filter(Boolean);
  const broadcastMessages =
    broadcastItems.length > 0
      ? broadcastItems
      : latestBroadcastSummary.trim()
        ? [latestBroadcastSummary.trim()]
        : [];
  const shareSummaryFallback = useMemo(() => {
    const latestNarration = latestCompletedNarration?.narrationText.trim();
    const broadcastSummary = latestBroadcastSummary.trim();

    if (
      latestNarration &&
      broadcastSummary &&
      !latestNarration.includes(broadcastSummary)
    ) {
      return `${broadcastSummary} ${latestNarration}`;
    }

    if (latestNarration) {
      return latestNarration;
    }

    if (broadcastSummary) {
      return broadcastSummary;
    }

    return "记录仍在回响，下一轮选择正在逼近。";
  }, [latestBroadcastSummary, latestCompletedNarration]);
  const shareGameUrl = useMemo(
    () =>
      new URL(
        sessionId
          ? routeWithClonedSession(
              appRoutes.gameplay,
              sessionId,
              latestCompletedNarration?.round,
            )
          : appRoutes.lobby,
        window.location.origin,
      ).toString(),
    [latestCompletedNarration?.round, sessionId],
  );

  useEffect(() => {
    if (!feedback) return undefined;

    const timer = window.setTimeout(() => setFeedback(null), 2200);
    return () => window.clearTimeout(timer);
  }, [feedback]);

  useEffect(() => {
    if (!import.meta.env.DEV) {
      return;
    }

    window.localStorage.setItem(
      AUTO_CHOICE_STORAGE_KEY,
      autoChoiceEnabled ? "1" : "0",
    );
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
    track("round_reached", {
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
    if (!sessionId || !isEndingReviewMode) {
      return;
    }

    void loadCompleteSessionRounds(sessionId).catch((roundsError) => {
      setFeedback(readErrorMessage(roundsError, "读取完整回响失败。"));
    });
  }, [isEndingReviewMode, readErrorMessage, sessionId]);

  useEffect(() => {
    if (!sessionId || phase !== "booting") {
      return;
    }

    void bootstrapSession().catch((bootstrapError) => {
      setFeedback(readErrorMessage(bootstrapError, "开场记录启动失败。"));
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
        throw new Error("这条分支暂时没有可窥见的记录碎片。");
      }
      consumeIntuition();
      track("intuition_preview_used");

      setRoundControls((prev) => ({
        round: choicePanelRound,
        activeObsession:
          prev.round === choicePanelRound ? prev.activeObsession : false,
        obsessionInput:
          prev.round === choicePanelRound ? prev.obsessionInput : "",
        previews: {
          ...(prev.round === choicePanelRound ? prev.previews : {}),
          [choice.id]: motivationAndRisk,
        },
      }));
      setFeedback(motivationAndRisk);
    } catch (previewError) {
      setFeedback(readErrorMessage(previewError, "记录窥见失败。"));
    }
  };

  const handleBacktrackRound = useCallback(
    (round: number) => {
      const targetRoundState = roundStates[round];
      if (!targetRoundState?.choices.length) {
        setFeedback("这一章暂时没有可回溯的候选项。");
        return;
      }

      setExpandedChoicePanelRound((prev) => (prev === round ? null : round));
      setRoundControls((prev) => ({
        round,
        activeObsession: false,
        obsessionInput: prev.round === round ? prev.obsessionInput : "",
        previews: prev.round === round ? prev.previews : {},
      }));
      setFeedback(null);
    },
    [roundStates],
  );

  const resetChoicePanelAfterSubmission = useCallback(() => {
    setExpandedChoicePanelRound(null);
    setRoundControls({
      round: currentRound,
      activeObsession: false,
      obsessionInput: "",
      previews: {},
    });
  }, [currentRound]);

  const rememberSubmittedChoice = useCallback(
    (choice: SubmittedChoiceDisplay) => {
      setSubmittedChoiceState((prev) => ({
        sessionId,
        choices: {
          ...(prev.sessionId === sessionId ? prev.choices : {}),
          [currentRound]: choice,
        },
      }));
    },
    [currentRound, sessionId],
  );

  const forgetSubmittedChoice = useCallback(() => {
    setSubmittedChoiceState((prev) => {
      if (prev.sessionId !== sessionId || !prev.choices[currentRound]) {
        return prev;
      }

      const next = { ...prev.choices };
      delete next[currentRound];
      return {
        sessionId,
        choices: next,
      };
    });
  }, [currentRound, sessionId]);

  const handleChoiceClick = useCallback(
    async (choice: Choice) => {
      try {
        rememberSubmittedChoice({
          action: choice.action,
          text: activeObsession ? '[执念]' : choice.text,
        });
        const submission = submitChoice(
          {
            input: {
              actions: [{
                character_name: playableCharacterName,
                action_type: 'selected_option',
                title: choice.text,
                action: choice.action,
                motivation_and_risk: choice.motivationAndRisk,
              }],
            },
            displayText: choice.text,
          },
          activeObsession,
        );
        resetChoicePanelAfterSubmission();
        await submission;
        setFeedback(null);
      } catch (submitError) {
        forgetSubmittedChoice();
        setFeedback(readErrorMessage(submitError, "推进回响失败。"));
      }
    },
    [
      activeObsession,
      forgetSubmittedChoice,
      playableCharacterName,
      rememberSubmittedChoice,
      readErrorMessage,
      resetChoicePanelAfterSubmission,
      submitChoice,
    ],
  );

  const handleBacktrackChoiceClick = useCallback(
    async (choice: Choice) => {
      const sourceSessionId = sessionId;
      const sourceRound = choicePanelRound;
      if (!sourceSessionId) {
        setFeedback("当前还没有可回溯的记录。");
        return;
      }

      try {
        setFeedback(`正在从第 ${sourceRound} 章回溯...`);
        const cloned = await cloneSharedSession(sourceSessionId, sourceRound);
        navigate(routeWithSession(appRoutes.gameplay, cloned.sessionId), {
          replace: true,
        });
        setExpandedChoicePanelRound(null);
        setRoundControls({
          round: sourceRound,
          activeObsession: false,
          obsessionInput: "",
          previews: {},
        });
        await submitChoice({
          input: {
            actions: [{
              character_name: playableCharacterName,
              action_type: 'selected_option',
              title: choice.text,
              action: choice.action,
              motivation_and_risk: choice.motivationAndRisk,
            }],
          },
          displayText: choice.text,
        });
        setFeedback(null);
      } catch (backtrackError) {
        setFeedback(readErrorMessage(backtrackError, "回溯展开失败。"));
      }
    },
    [
      choicePanelRound,
      cloneSharedSession,
      navigate,
      playableCharacterName,
      readErrorMessage,
      sessionId,
      submitChoice,
    ],
  );

  const handleContinueClick = useCallback(async () => {
    try {
      rememberSubmittedChoice({
        action: "continue",
        text: "继续回响",
      });
      const submission = submitChoice({
        input: {
          actions: [{
            character_name: playableCharacterName,
            action_type: 'free_text',
            action: "continue",
          }],
        },
        displayText: "继续回响",
      });
      resetChoicePanelAfterSubmission();
      await submission;
      setFeedback(null);
    } catch (submitError) {
      forgetSubmittedChoice();
      setFeedback(readErrorMessage(submitError, "续写回响失败。"));
    }
  }, [
    forgetSubmittedChoice,
    playableCharacterName,
    readErrorMessage,
    rememberSubmittedChoice,
    resetChoicePanelAfterSubmission,
    submitChoice,
  ]);

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
      sessionId ?? "no-session",
      currentRound,
      currentRoundChoices
        .map((choice) => `${choice.id}:${choice.action}`)
        .join("|"),
    ].join(":");

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
      setFeedback("请先写下这次想写入记录的执念。");
      return;
    }

    try {
      rememberSubmittedChoice({
        action: actionText,
        text: '[执念]',
      });
      const submission = submitChoice(
        {
          input: {
            actions: [{
              character_name: playableCharacterName,
              action_type: 'free_text',
              action: actionText,
            }],
          },
          displayText: actionText,
        },
        true,
      );
      resetChoicePanelAfterSubmission();
      await submission;
      setFeedback(null);
    } catch (submitError) {
      forgetSubmittedChoice();
      setFeedback(readErrorMessage(submitError, "执念写入失败。"));
    }
  };

  const handleSave = async () => {
    if (!canArchiveLatestCompletedRound) {
      setFeedback(archiveActionUnavailableReason);
      return;
    }

    try {
      await createSave();
      setFeedback("这一段记录已封存");
    } catch (saveError) {
      setFeedback(readErrorMessage(saveError, "封存失败。"));
    }
  };

  const handleToggleObsession = () => {
    if (isEndingReviewMode) {
      return;
    }
    const wasChoicePanelCollapsed = expandedChoicePanelRound !== currentRound;
    setExpandedChoicePanelRound(currentRound);
    setRoundControls((prev) => ({
      round: currentRound,
      activeObsession:
        wasChoicePanelCollapsed || prev.round !== currentRound
          ? true
          : !prev.activeObsession,
      obsessionInput: prev.round === currentRound ? prev.obsessionInput : "",
      previews: prev.round === currentRound ? prev.previews : {},
    }));
    setFeedback(null);
  };

  return (
    <ScreenShell className="h-full min-h-0 items-stretch overflow-hidden py-2 sm:py-2 md:py-2">
      <StoryFrame className="relative flex h-full max-w-5xl flex-col overflow-hidden px-2.5 py-2.5 sm:px-3 sm:py-3">
        <div className="pointer-events-none absolute inset-0 bg-linear-to-b from-transparent via-[#08111d]/35 to-[#08111d]" />
        <div className="relative z-10 flex min-h-0 flex-1 flex-col gap-2">
          <div className="relative flex min-h-0 flex-1 flex-col">
            <NarrationPanel
              narrationHistory={narrationHistory}
              currentRound={currentRound}
              isAwaitingNarration={Boolean(
                activeRoundState?.isAwaitingNarration,
              )}
              skipRestoredNarrationAnimation={
                skipRestoredNarrationAnimation || isEndingReviewMode
              }
              broadcastMessages={broadcastMessages}
              onTypewriterComplete={handleTypewriterComplete}
              activeBacktrackRound={isBacktrackChoicePanel ? choicePanelRound : null}
              onBacktrackRound={
                isEndingReviewMode ? undefined : handleBacktrackRound
              }
            />
            {!isEndingReviewMode ? (
              <div className="pointer-events-none absolute inset-x-1 bottom-1 z-10 sm:inset-x-3">
                <div className="mx-auto w-full max-w-3xl">
                  {statusMessage ? (
                    <div className="mb-1.5 flex justify-center px-2">
                      <p className="pointer-events-none max-w-full rounded-lg border border-[rgba(116,103,80,0.45)] bg-[rgba(8,14,26,0.88)] px-3 py-1 text-left text-xs leading-5 text-[#d9cbb1] shadow-[0_12px_28px_rgba(0,0,0,0.35)] backdrop-blur-md sm:text-sm">
                        {statusMessage}
                      </p>
                    </div>
                  ) : null}
                  <div className="pointer-events-auto">
                  <ChoicePanel
                    hasChoices={hasChoicePanelChoices}
                    canContinue={
                      !isBacktrackChoicePanel && canContinueWithoutChoice
                    }
                    choices={choicePanelChoices}
                    previews={previews}
                    remainingIntuitionPoints={intuitionPoints}
                    activeObsession={activeObsession}
                    isObsessionToggleDisabled={isObsessionToggleDisabled}
                    obsessionInput={obsessionInput}
                    autoChoiceEnabled={autoChoiceEnabled}
                    showAutoChoiceToggle={import.meta.env.DEV}
                    isCollapsed={isChoicePanelCollapsed}
                    isChoiceInteractionDisabled={isChoicePanelInteractionDisabled}
                    isObsessionSubmitDisabled={isObsessionSubmitDisabled}
                    onToggleCollapsed={() => {
                      if (isBacktrackChoicePanel) {
                        setExpandedChoicePanelRound(null);
                        return;
                      }
                      const isExpandingChoicePanel =
                        expandedChoicePanelRound !== currentRound;
                      setExpandedChoicePanelRound((prev) =>
                        prev === currentRound ? null : currentRound,
                      );
                      if (isExpandingChoicePanel) {
                        setRoundControls((prev) => ({
                          round: currentRound,
                          activeObsession: false,
                          obsessionInput:
                            prev.round === currentRound
                              ? prev.obsessionInput
                              : "",
                          previews:
                            prev.round === currentRound ? prev.previews : {},
                        }));
                      }
                    }}
                    onChoiceClick={
                      isBacktrackChoicePanel
                        ? handleBacktrackChoiceClick
                        : handleChoiceClick
                    }
                    onContinue={handleContinueClick}
                    onAutoChoiceToggle={setAutoChoiceEnabled}
                    onToggleObsession={handleToggleObsession}
                    onPreview={handlePreview}
                    onObsessionInputChange={(nextValue) => {
                      setRoundControls((prev) => ({
                        round: choicePanelRound,
                        activeObsession:
                          prev.round === choicePanelRound
                            ? prev.activeObsession
                            : false,
                        obsessionInput: nextValue,
                        previews:
                          prev.round === choicePanelRound ? prev.previews : {},
                      }));
                    }}
                    onObsessionSubmit={handleObsessionSubmit}
                  />
                  </div>
                </div>
              </div>
            ) : null}
          </div>
          <div className="relative z-40 shrink-0 touch-pan-y">
            <GameplayToolbar
              isReadOnly={isEndingReviewMode}
              currentRound={currentRound}
              obsessionPoints={obsessionPoints}
              intuitionPoints={intuitionPoints}
              sessionId={sessionId}
              shareSummaryFallback={shareSummaryFallback}
              shareGameUrl={shareGameUrl}
              generatedProfiles={generatedProfiles}
              archiveActionKey={archiveActionKey}
              isArchiveActionDisabled={!canArchiveLatestCompletedRound}
              archiveActionUnavailableReason={archiveActionUnavailableReason}
              onBackToLobby={() => {
                suppressSessionRestore(sessionId);
                navigate(appRoutes.lobby, { replace: true });
                resetGame();
              }}
              onSave={handleSave}
            />
          </div>
        </div>
      </StoryFrame>
    </ScreenShell>
  );
};

export default GameplayPage;
