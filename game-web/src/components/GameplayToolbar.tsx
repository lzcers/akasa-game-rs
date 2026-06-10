import React, { useEffect, useMemo, useState } from "react";
import {
  BookOpenText,
  Clock3,
  Eye,
  Flame,
  House,
  MoreHorizontal,
  Save,
  Share2,
  X,
} from "lucide-react";
import { SecondaryButton } from "./AkashicUI";
import StoryShareCard from "./StoryShareCard";
import { generateGameSessionStorySummary } from "../lib/api";
import type { GeneratedProfiles } from "../lib/api";

interface GameplayToolbarProps {
  isReadOnly?: boolean;
  currentRound: number;
  obsessionPoints: number;
  intuitionPoints: number;
  sessionId?: string | null;
  shareSummaryFallback: string;
  shareGameUrl: string;
  generatedProfiles?: GeneratedProfiles | null;
  archiveActionKey: string;
  isArchiveActionDisabled: boolean;
  archiveActionUnavailableReason: string | null;
  onBackToLobby: () => void;
  onSave: () => void | Promise<void>;
}

const GameplayToolbar: React.FC<GameplayToolbarProps> = ({
  isReadOnly = false,
  currentRound,
  obsessionPoints,
  intuitionPoints,
  sessionId,
  shareSummaryFallback,
  shareGameUrl,
  generatedProfiles,
  archiveActionKey,
  isArchiveActionDisabled,
  archiveActionUnavailableReason,
  onBackToLobby,
  onSave,
}) => {
  const [isUtilityMenuOpen, setIsUtilityMenuOpen] = useState(false);
  const [isRecordViewerOpen, setIsRecordViewerOpen] = useState(false);
  const [shareCardOpenKey, setShareCardOpenKey] = useState<string | null>(null);
  const [shareSummary, setShareSummary] = useState<string | null>(null);
  const [shareError, setShareError] = useState<string | null>(null);
  const [isShareLoading, setIsShareLoading] = useState(false);
  const isShareCardOpen =
    shareCardOpenKey === archiveActionKey && !isArchiveActionDisabled;
  const hasGeneratedProfiles = Boolean(
    generatedProfiles?.world.trim() && generatedProfiles?.protagonist.trim(),
  );

  const recordPanels = useMemo(() => {
    if (!generatedProfiles) {
      return [];
    }

    return [
      {
        key: "world",
        title: "阿卡夏显影出的世界记录",
        text: generatedProfiles.world,
        className: "border-[#5b6f96]/30 bg-[#0f1624]/92 text-[#c7d5f2]",
      },
      {
        key: "protagonist",
        title: "阿卡夏显影出的角色记录",
        text: generatedProfiles.protagonist,
        className: "border-[#6f5f96]/30 bg-[#151325]/92 text-[#d8d0f2]",
      },
    ];
  }, [generatedProfiles]);

  const resolvedShareSummary = useMemo(() => {
    const fetchedSummary = shareSummary?.trim();
    if (fetchedSummary) {
      return fetchedSummary;
    }
    return shareSummaryFallback.trim();
  }, [shareSummary, shareSummaryFallback]);

  useEffect(() => {
    if (!isShareCardOpen && !isRecordViewerOpen) {
      return undefined;
    }

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setShareCardOpenKey(null);
        setIsRecordViewerOpen(false);
      }
    };

    window.addEventListener("keydown", handleEscape);
    return () => window.removeEventListener("keydown", handleEscape);
  }, [isRecordViewerOpen, isShareCardOpen]);

  useEffect(() => {
    if (!isShareCardOpen || !sessionId) {
      return;
    }

    let cancelled = false;

    const loadSummary = async () => {
      setIsShareLoading(true);
      setShareError(null);

      try {
        const data = await generateGameSessionStorySummary(sessionId);
        if (cancelled) {
          return;
        }
        setShareSummary(data.summary.trim());
      } catch (error) {
        if (cancelled) {
          return;
        }
        const message =
          error instanceof Error ? error.message : "回响摘录生成失败。";
        setShareError(message);
      } finally {
        if (!cancelled) {
          setIsShareLoading(false);
        }
      }
    };

    void loadSummary();

    return () => {
      cancelled = true;
    };
  }, [isShareCardOpen, sessionId]);

  return (
    <>
      <div className="game-opts inset-x-0 rounded-full border border-[rgba(116,103,80,0.34)] bg-[rgba(8,14,26,0.82)] px-1.5 py-1 backdrop-blur-md">
        <div className="relative grid grid-cols-[minmax(0,1fr)_auto] items-center gap-1.5">
          <div className="flex min-w-0 items-center ml-3 justify-start gap-1.5 text-[0.72rem] font-semibold leading-4 text-[#d9cbb1] sm:gap-2 sm:text-xs">
            <span className="inline-flex items-center gap-1">
              <Clock3 className="h-3.5 w-3.5" />
              <span>{currentRound}</span>
            </span>
            {!isReadOnly ? (
              <>
                <span className="text-[#8f98ab]">|</span>
                <span className="inline-flex items-center gap-1">
                  <Flame className="h-3.5 w-3.5" />
                  <span>{obsessionPoints}</span>
                </span>
                <span className="text-[#8f98ab]">|</span>
                <span className="inline-flex items-center gap-1">
                  <Eye className="h-3.5 w-3.5" />
                  <span>{`${intuitionPoints}/2`}</span>
                </span>
              </>
            ) : null}
          </div>
          <div className="relative shrink-0">
            <SecondaryButton
              type="button"
              onClick={() => setIsUtilityMenuOpen((prev) => !prev)}
              className="min-h-0 gap-1.5 px-2 py-1 text-[0.72rem] leading-4 sm:text-xs"
              aria-label="打开菜单"
            >
              <MoreHorizontal className="h-3.5 w-3.5" />
              菜单
            </SecondaryButton>
            {isUtilityMenuOpen ? (
              <div className="absolute bottom-[calc(100%+0.45rem)] right-0 z-[80] min-w-[8.8rem] rounded-[0.95rem] border border-[rgba(116,103,80,0.5)] bg-[rgba(7,13,24,0.96)] p-1.5 shadow-[0_10px_24px_rgba(0,0,0,0.45)]">
                <button
                  type="button"
                  onClick={() => {
                    onBackToLobby();
                    setIsUtilityMenuOpen(false);
                  }}
                  className="flex w-full items-center gap-1.5 rounded-[0.7rem] px-2 py-1.5 text-left text-[0.72rem] leading-4 text-[#f3ead8] transition-colors hover:bg-[rgba(188,169,124,0.14)] sm:text-xs"
                >
                  <House className="h-3.5 w-3.5" />
                  返回回响厅
                </button>
                <button
                  type="button"
                  disabled={!hasGeneratedProfiles}
                  title={hasGeneratedProfiles ? "查看记录" : "记录仍在显影中"}
                  onClick={() => {
                    if (!hasGeneratedProfiles) {
                      return;
                    }
                    setIsRecordViewerOpen(true);
                    setIsUtilityMenuOpen(false);
                  }}
                  className="flex w-full items-center gap-1.5 rounded-[0.7rem] px-2 py-1.5 text-left text-[0.72rem] leading-4 text-[#f3ead8] transition-colors hover:bg-[rgba(188,169,124,0.14)] disabled:cursor-not-allowed disabled:text-[#8f98ab] disabled:hover:bg-transparent sm:text-xs"
                >
                  <BookOpenText className="h-3.5 w-3.5" />
                  查看记录
                </button>
                <button
                  type="button"
                  disabled={isArchiveActionDisabled}
                  title={archiveActionUnavailableReason ?? "封存记录"}
                  onClick={() => {
                    if (isArchiveActionDisabled) {
                      return;
                    }
                    void onSave();
                    setIsUtilityMenuOpen(false);
                  }}
                  className="flex w-full items-center gap-1.5 rounded-[0.7rem] px-2 py-1.5 text-left text-[0.72rem] leading-4 text-[#f3ead8] transition-colors hover:bg-[rgba(188,169,124,0.14)] disabled:cursor-not-allowed disabled:text-[#8f98ab] disabled:hover:bg-transparent sm:text-xs"
                >
                  <Save className="h-3.5 w-3.5" />
                  存档
                </button>
                <button
                  type="button"
                  disabled={isArchiveActionDisabled}
                  title={archiveActionUnavailableReason ?? "分享记录"}
                  onClick={() => {
                    if (isArchiveActionDisabled) {
                      return;
                    }
                    setShareCardOpenKey(archiveActionKey);
                    setIsUtilityMenuOpen(false);
                  }}
                  className="flex w-full items-center gap-1.5 rounded-[0.7rem] px-2 py-1.5 text-left text-[0.72rem] leading-4 text-[#f3ead8] transition-colors hover:bg-[rgba(188,169,124,0.14)] disabled:cursor-not-allowed disabled:text-[#8f98ab] disabled:hover:bg-transparent sm:text-xs"
                >
                  <Share2 className="h-3.5 w-3.5" />
                  分享记录
                </button>
                {isArchiveActionDisabled && archiveActionUnavailableReason ? (
                  <p className="px-2 py-1 text-[0.68rem] leading-4 text-[#8f98ab]">
                    {archiveActionUnavailableReason}
                  </p>
                ) : null}
              </div>
            ) : null}
          </div>
        </div>
      </div>
      {isRecordViewerOpen ? (
        <div className="fixed inset-0 z-[60] flex items-end justify-center bg-[rgba(5,8,15,0.72)] px-3 py-4 backdrop-blur-sm sm:items-center sm:px-6">
          <div
            className="absolute inset-0"
            onClick={() => setIsRecordViewerOpen(false)}
            aria-hidden="true"
          />
          <div className="relative z-10 flex max-h-[88svh] w-full max-w-4xl flex-col">
            <div className="mb-3 flex justify-end">
              <button
                type="button"
                onClick={() => setIsRecordViewerOpen(false)}
                className="inline-flex h-10 w-10 items-center justify-center rounded-full border border-[rgba(116,103,80,0.5)] bg-[rgba(8,14,26,0.9)] text-[#f3ead8] transition-colors hover:bg-[rgba(188,169,124,0.14)]"
                aria-label="关闭记录查看"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <div className="game-card grid min-h-0 gap-3 overflow-y-auto rounded-3xl border border-[rgba(116,103,80,0.5)] bg-[rgba(8,14,26,0.95)] p-3 shadow-[0_24px_80px_rgba(1,8,20,0.6)] sm:p-4 md:grid-cols-2">
              {recordPanels.map((panel) => (
                <article
                  key={panel.key}
                  className={`flex min-h-[18rem] flex-col rounded-xl border p-3 md:min-h-[28rem] md:p-4 ${panel.className}`}
                >
                  <h2 className="shrink-0 text-base font-semibold leading-6 text-[#f8f1e3] md:text-lg">
                    {panel.title}
                  </h2>
                  <p className="akashic-scroll mt-2 min-h-0 flex-1 overflow-y-auto whitespace-pre-wrap pr-1 text-sm leading-6 sm:text-[0.95rem] sm:leading-7 md:text-base">
                    {panel.text}
                  </p>
                </article>
              ))}
            </div>
          </div>
        </div>
      ) : null}
      {isShareCardOpen ? (
        <div className="fixed inset-0 z-[60] flex items-end justify-center bg-[rgba(5,8,15,0.72)] px-3 py-4 backdrop-blur-sm sm:items-center sm:px-6">
          <div
            className="absolute inset-0"
            onClick={() => setShareCardOpenKey(null)}
            aria-hidden="true"
          />
          <div className="relative z-10 w-full max-w-3xl">
            <div className="mb-3 flex justify-end">
              <button
                type="button"
                onClick={() => setShareCardOpenKey(null)}
                className="inline-flex h-10 w-10 items-center justify-center rounded-full border border-[rgba(116,103,80,0.5)] bg-[rgba(8,14,26,0.9)] text-[#f3ead8] transition-colors hover:bg-[rgba(188,169,124,0.14)]"
                aria-label="关闭分享卡片"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            {isShareLoading ? (
              <div className="game-card rounded-3xl border border-[rgba(116,103,80,0.5)] bg-[rgba(8,14,26,0.95)] px-6 py-8 text-center text-sm text-[#d9cbb1] shadow-[0_24px_80px_rgba(1,8,20,0.6)]">
                正在整理这一段记录的共鸣摘要...
              </div>
            ) : (
              <div className="space-y-3">
                {shareError ? (
                  <div className="rounded-2xl border border-amber-300/20 bg-amber-100/8 px-4 py-3 text-xs leading-5 text-amber-100/85">
                    摘要接口暂时不可用，当前展示的是最近一段回响。{shareError}
                  </div>
                ) : null}
                <StoryShareCard
                  summary={resolvedShareSummary}
                  gameUrl={shareGameUrl}
                />
              </div>
            )}
          </div>
        </div>
      ) : null}
    </>
  );
};

export default GameplayToolbar;
