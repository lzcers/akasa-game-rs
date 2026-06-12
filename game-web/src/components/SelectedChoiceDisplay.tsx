import React from "react";
import { MousePointer2, RotateCcw } from "lucide-react";

interface SelectedChoiceDisplayProps {
  selectedChoiceText: string;
  selectedChoiceAction?: string | null;
  canBacktrack?: boolean;
  isBacktrackActive?: boolean;
  onBacktrack?: () => void;
}

const SelectedChoiceDisplay: React.FC<SelectedChoiceDisplayProps> = ({
  selectedChoiceText,
  selectedChoiceAction,
  canBacktrack = false,
  isBacktrackActive = false,
  onBacktrack,
}) => {
  const normalizedAction = selectedChoiceAction?.trim();

  return (
    <div
      className={[
        "relative inline-flex w-full flex-col overflow-hidden rounded-[0.85rem] border border-amber-200/55 bg-[linear-gradient(135deg,rgba(251,191,36,0.18),rgba(14,165,233,0.1)_52%,rgba(15,23,42,0.18))] px-2.5 py-1.5 pl-3.5 text-[0.82rem] font-medium leading-5 text-amber-50 shadow-[0_0_0_1px_rgba(251,191,36,0.16),0_10px_28px_rgba(251,191,36,0.1)] sm:text-[0.92rem]",
        onBacktrack ? "min-w-52" : "",
      ].join(" ")}
    >
      <span className="absolute inset-y-1.5 left-1 w-1 rounded-full bg-amber-200/85 shadow-[0_0_12px_rgba(251,191,36,0.58)]" />
      <div className="flex min-w-0 items-start justify-between gap-2">
        <span className="flex min-w-0 flex-1">
          <MousePointer2 className="mr-1 mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-100" />
          <span className="min-w-0 break-words">{selectedChoiceText}</span>
        </span>
        {onBacktrack ? (
          <button
            type="button"
            onClick={onBacktrack}
            disabled={!canBacktrack}
            aria-pressed={isBacktrackActive}
            title={canBacktrack ? "从此节点回溯" : "此节点暂无可回溯选项"}
            className={[
              "inline-flex h-6 shrink-0 items-center gap-1 rounded-full border px-1.5 text-[0.65rem] leading-none transition-colors sm:text-[0.7rem]",
              isBacktrackActive
                ? "border-amber-200/70 bg-amber-200/16 text-amber-50"
                : "border-amber-200/30 bg-black/15 text-amber-100/82 hover:border-amber-200/60 hover:text-amber-50",
              canBacktrack ? "" : "cursor-not-allowed opacity-45",
            ].join(" ")}
          >
            <RotateCcw className="h-3 w-3" />
            回溯
          </button>
        ) : null}
      </div>
      {normalizedAction ? (
        <span className="mt-1 block wrap-break-word text-[0.76rem] leading-5 text-amber-100/72 sm:text-[0.84rem]">
          {normalizedAction}
        </span>
      ) : null}
    </div>
  );
};

export default SelectedChoiceDisplay;
