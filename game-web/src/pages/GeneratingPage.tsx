import React, { useEffect, useMemo, useState } from "react";
import { LoaderCircle, Orbit, Sparkles } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { useGameUIStore } from "../store/gameUIStore";
import type { StartupStage } from "../store/gameUIStore";
import {
  PrimaryButton,
  ScreenShell,
  SecondaryButton,
  SectionCard,
  StatusPill,
  StoryFrame,
} from "../components/AkashicUI";
import GeneratedProfilesCarousel from "../components/GeneratedProfilesCarousel";
import { generatedProfilePanels } from "../components/generatedProfilePanels";
import { useGameInternalStore } from "../store/gameStore";
import { track } from "../lib/analytics";
import { appRoutes, routeWithSession } from "../lib/appRoutes";
import { useNavigate } from "react-router-dom";

type StepStatus = "pending" | "active" | "done";

interface StartupStep {
  key: ProfileStepKey;
  label: string;
  title: string;
  description: string;
}

interface StageHeadline {
  title: string;
  subtitle: string;
}

type ProfileStepKey =
  | "generating_world"
  | "generating_character"
  | "creating_session";

const startupSteps: StartupStep[] = [
  {
    key: "generating_world",
    label: "世界显影中",
    title: "共鸣世界记录",
    description:
      "正在从你的设定中提取时代纹理、核心矛盾与规则压力，让世界从记录中浮现。",
  },
  {
    key: "generating_character",
    label: "角色显影中",
    title: "凝聚角色回响",
    description:
      "正在把烙印、欲望、弱点与性格倾向写入记录，让角色更适合长期演绎。",
  },
  {
    key: "creating_session",
    label: "写入回响中",
    title: "唤起第一段记录",
    description: "世界记录与角色记录已经落笔，正在汇入阿卡夏记录，并唤起开场叙事。",
  },
];

const stageOrder: Exclude<StartupStage, "idle">[] = [
  "generating_world",
  "generating_character",
  "creating_session",
  "ready_to_enter",
];

const rotatingMessages: Record<Exclude<StartupStage, "idle">, string[]> = {
  generating_world: [
    "正在显影时代压力",
    "正在收束世界规则与禁忌",
    "正在校准冲突将如何逼近角色",
  ],
  generating_character: [
    "正在收束角色欲望",
    "正在打磨角色弱点与裂缝",
    "正在让性格倾向落成可演绎的行动方式",
  ],
  ready_to_enter: [
    "第一段记录已经开始显影",
    "共鸣入口已经被推开一道缝隙",
    "你现在可以步入回响，直接看到故事继续流动",
  ],
  creating_session: [
    "正在写入世界记录",
    "正在唤起第一段回响",
    "正在为你铺开故事的开场",
  ],
};

function stepStatus(
  currentStage: StartupStage,
  targetStage: Exclude<StartupStage, "idle">,
): StepStatus {
  const currentIndex = stageOrder.indexOf(
    currentStage === "idle" ? "generating_world" : currentStage,
  );
  const targetIndex = stageOrder.indexOf(targetStage);
  if (targetIndex < currentIndex) {
    return "done";
  }
  if (targetIndex === currentIndex) {
    return "active";
  }
  return "pending";
}

function stageHeadline(
  stage: StartupStage,
  name: string,
  hasPlayableSession: boolean,
): StageHeadline {
  switch (stage) {
    case "generating_character":
      return {
        title: "角色记录正在显影",
        subtitle: `${name} 的欲望、弱点与行动倾向正在被收束成更适合展开剧情的记录底稿。`,
      };
    case "creating_session":
      return {
        title: "共鸣入口正在开启",
        subtitle:
          "世界记录与角色记录已经生成，正在将它们汇入回响，并点亮第一段叙事。",
      };
    case "ready_to_enter":
      return hasPlayableSession
        ? {
            title: "开场记录已经点亮",
            subtitle:
              "第一段叙事已经开始流动。你现在就可以步入回响，直接看着它继续展开。",
          }
        : {
            title: "共鸣入口稍有震颤",
            subtitle:
              "世界记录与角色记录已经完备。再试一次，就能继续唤起你的第一段回响。",
          };
    case "generating_world":
    case "idle":
    default:
      return {
        title: "世界记录正在显影",
        subtitle:
          "阿卡夏会先推演世界压力，再收束角色记录，让开场更像一个真正会继续生长的故事。",
      };
  }
}

const GeneratingPage: React.FC = () => {
  const navigate = useNavigate();
  const {
    startupStage,
    character,
    world,
    preparedProfiles,
    isLoading,
    error,
    startGame,
    enterWorld,
    hasPlayableSession,
  } = useGameUIStore(
    useShallow((state) => ({
      startupStage: state.startupStage,
      character: state.character,
      world: state.world,
      preparedProfiles: state.preparedProfiles,
      isLoading: state.isLoading,
      error: state.error,
      startGame: state.startGame,
      enterWorld: state.enterWorld,
      hasPlayableSession: Boolean(state.stateView),
    })),
  );
  const sessionId = useGameInternalStore((state) => state.sessionId);
  const canEnterWorld =
    startupStage === "ready_to_enter" &&
    Boolean(sessionId) &&
    hasPlayableSession;
  const isEnterWorldPending = !canEnterWorld || isLoading;
  const headline = stageHeadline(
    startupStage,
    character.name || "这位角色",
    canEnterWorld,
  );
  const stageKey = startupStage === "idle" ? "generating_world" : startupStage;
  const currentMessages = useMemo(() => {
    if (stageKey !== "ready_to_enter") {
      return rotatingMessages[stageKey];
    }

    return canEnterWorld
      ? [
          "第一段记录已经开始显影",
          "共鸣入口已经被推开一道缝隙",
          "你现在可以步入回响，直接看到故事继续流动",
        ]
      : [
          "设定已经落笔，只差把记录重新续上",
          "共鸣入口短暂摇晃，你可以再次尝试",
          "再推开一次门，回响会继续向前",
        ];
  }, [canEnterWorld, stageKey]);
  const messageKey = currentMessages.join("||");
  const [messageCursor, setMessageCursor] = useState({ key: "", index: 0 });
  const messageIndex =
    messageCursor.key === messageKey
      ? Math.min(messageCursor.index, Math.max(currentMessages.length - 1, 0))
      : 0;

  const handleEnterWorld = () => {
    if (preparedProfiles) {
      track("generated_profiles_accepted", {
        generatedWorldProfile: preparedProfiles.world,
        generatedCharacterProfile: preparedProfiles.character,
        generatedKeyStoryBeats: preparedProfiles.keyStoryBeats,
      });
    }
    void enterWorld().then((entered) => {
      if (entered) {
        navigate(routeWithSession(appRoutes.gameplay, entered.sessionId), { replace: true });
      }
    });
  };
  const profilePanels = useMemo(
    () => (preparedProfiles ? generatedProfilePanels(preparedProfiles) : []),
    [preparedProfiles],
  );
  const profileSetKey = preparedProfiles
    ? [
        preparedProfiles.world,
        preparedProfiles.character,
        preparedProfiles.keyStoryBeats,
      ].join("\n---\n")
    : "";

  useEffect(() => {
    if (currentMessages.length <= 1) {
      return undefined;
    }

    const timer = window.setInterval(() => {
      setMessageCursor((current) => {
        const currentIndex = current.key === messageKey ? current.index : 0;
        return {
          key: messageKey,
          index: (currentIndex + 1) % currentMessages.length,
        };
      });
    }, 2200);

    return () => window.clearInterval(timer);
  }, [currentMessages.length, messageKey]);

  return (
    <ScreenShell className="items-stretch px-2 py-2 sm:px-3 sm:py-3 md:items-center md:px-6 md:py-5">
      <StoryFrame className="flex h-[calc(100svh-1rem)] max-w-3xl p-2.5 md:h-[calc(100svh-2.5rem)] md:p-5">
        <div className="flex min-h-0 flex-1 flex-col gap-2 md:gap-4">
          <div className="shrink-0 space-y-1 text-center">
            <h1 className="text-lg font-semibold tracking-wide text-[#f6eddc] sm:text-2xl md:text-4xl">
              {headline.title}
            </h1>
            <p className="mx-auto line-clamp-2 max-w-2xl text-xs leading-5 text-[#9ca7be] sm:text-sm md:text-base">
              {headline.subtitle}
            </p>
          </div>

          <div className="shrink-0 rounded-xl border border-[#6d86b7]/25 bg-[#101827]/78 px-3 py-1.5 text-center text-xs text-[#c7d5f2] shadow-[0_10px_30px_rgba(3,8,18,0.25)] sm:py-2 sm:text-sm">
            {currentMessages[messageIndex]}
          </div>

          {error ? (
            <div className="rounded-[1.1rem] border border-[#7f3b3b]/50 bg-[#2a1216]/85 px-4 py-3 text-sm text-[#ffd7d7]">
              {error}
            </div>
          ) : null}

          {preparedProfiles ? (
            <SectionCard className="flex min-h-0 flex-1 flex-col gap-2 p-2 sm:p-3 md:gap-2.5 md:p-4">
              <div className="grid shrink-0 grid-cols-3 gap-1.5">
                {startupSteps.map((step) => {
                  const status = stepStatus(startupStage, step.key);
                  const iconClassName =
                    status === "active"
                      ? "text-[#7dd3fc] animate-spin"
                      : status === "done"
                        ? "text-[#f4d58d]"
                        : "text-[#5f6c86]";

                  return (
                    <div
                      key={step.key}
                      className={`min-w-0 rounded-lg border px-2 py-1 ${
                        status === "active"
                          ? "border-[#60a5fa]/40 bg-[#101a2c]/92"
                          : status === "done"
                            ? "border-[#8a7755]/35 bg-[#14110f]/85"
                            : "border-white/8 bg-[#0f1420]/70"
                      }`}
                    >
                      <div className="flex min-w-0 items-center gap-1.5">
                        <LoaderCircle
                          className={`h-3.5 w-3.5 shrink-0 ${iconClassName}`}
                        />
                        <span className="truncate text-[0.66rem] font-semibold text-[#efe4cd] sm:text-xs">
                          {step.label}
                        </span>
                      </div>
                    </div>
                  );
                })}
              </div>

              <GeneratedProfilesCarousel
                panels={profilePanels}
                resetKey={profileSetKey}
              />
            </SectionCard>
          ) : null}

          {!preparedProfiles ? (
            <SectionCard className="space-y-2 p-2.5">
              <div className="flex flex-wrap gap-1.5">
                <StatusPill
                  icon={Orbit}
                  iconClassName="h-3.5 w-3.5"
                  className="max-w-full border-[#3b82f6]/30 bg-[#0f2141]/80 px-2 py-1 text-[0.68rem] text-[#cfe0ff]"
                >
                  {world.era}
                </StatusPill>
                <StatusPill
                  icon={Sparkles}
                  iconClassName="h-3.5 w-3.5"
                  className="max-w-full border-[#8b5cf6]/30 bg-[#1b1733]/80 px-2 py-1 text-[0.68rem] text-[#e3d8ff]"
                >
                  {character.background || "角色烙印待显影"}
                </StatusPill>
              </div>

              <div className="space-y-2">
                {startupSteps.map((step) => {
                  const status = stepStatus(startupStage, step.key);
                  const iconClassName =
                    status === "active"
                      ? "text-[#7dd3fc] animate-spin"
                      : status === "done"
                        ? "text-[#f4d58d]"
                        : "text-[#5f6c86]";

                  return (
                    <button
                      type="button"
                      key={step.key}
                      className={`rounded-xl border px-3 py-2.5 text-left transition-colors ${
                        status === "active"
                          ? "border-[#60a5fa]/40 bg-[#101a2c]/92"
                          : status === "done"
                            ? "border-[#8a7755]/35 bg-[#14110f]/85"
                            : "border-white/8 bg-[#0f1420]/70"
                      } w-full`}
                    >
                      <div className="flex items-start gap-2.5">
                        <LoaderCircle
                          className={`mt-0.5 h-4 w-4 shrink-0 ${iconClassName}`}
                        />
                        <div className="min-w-0 space-y-0.5">
                          <p className="truncate text-xs font-semibold text-[#efe4cd]">
                            {step.label}
                          </p>
                          <p className="text-sm font-medium leading-5 text-[#f8f1e3]">
                            {step.title}
                          </p>
                          <p className="line-clamp-2 text-xs leading-5 text-[#9ca7be]">
                            {step.description}
                          </p>
                        </div>
                      </div>
                    </button>
                  );
                })}
              </div>
            </SectionCard>
          ) : null}
          {preparedProfiles ? (
            <div className="flex shrink-0 flex-row items-center justify-center gap-2 sm:gap-3">
              <SecondaryButton
                onClick={() => {
                  void startGame();
                }}
                className="min-h-9 min-w-0 flex-1 px-3 py-2 text-xs sm:flex-none sm:px-4 sm:text-sm"
              >
                重新共鸣
              </SecondaryButton>
              <PrimaryButton
                onClick={handleEnterWorld}
                disabled={isEnterWorldPending}
                className="min-h-9 min-w-0 flex-1 px-3 py-2 text-xs sm:flex-none sm:px-4 sm:text-sm"
              >
                {isEnterWorldPending ? "共鸣中..." : "步入回响"}
              </PrimaryButton>
            </div>
          ) : null}
        </div>
      </StoryFrame>
    </ScreenShell>
  );
};

export default GeneratingPage;
