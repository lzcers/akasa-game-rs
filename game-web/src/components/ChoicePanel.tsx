import React, { useEffect, useRef, useState } from "react";
import { Bot, ChevronDown, Eye, MousePointer2 } from "lucide-react";
import type { Choice } from "../lib/api";
import { SecondaryButton } from "./AkashicUI";

interface ChoicePanelProps {
  hasChoices: boolean;
  canContinue: boolean;
  choices: Choice[];
  previews: Record<string, string>;
  remainingIntuitionPoints: number;
  activeObsession: boolean;
  obsessionInput: string;
  autoChoiceEnabled?: boolean;
  showAutoChoiceToggle?: boolean;
  isCollapsed?: boolean;
  isChoiceInteractionDisabled: boolean;
  isObsessionSubmitDisabled: boolean;
  onToggleCollapsed?: () => void;
  onChoiceClick: (choice: Choice) => void | Promise<void>;
  onContinue: () => void | Promise<void>;
  onAutoChoiceToggle?: (enabled: boolean) => void;
  onPreview: (
    choice: Choice,
    event: React.MouseEvent<HTMLButtonElement>,
  ) => void | Promise<void>;
  onObsessionInputChange: (value: string) => void;
  onObsessionSubmit: (actionText: string) => void | Promise<void>;
}

const ChoicePanel: React.FC<ChoicePanelProps> = ({
  hasChoices,
  canContinue,
  choices,
  previews,
  remainingIntuitionPoints,
  activeObsession,
  obsessionInput,
  autoChoiceEnabled = false,
  showAutoChoiceToggle = false,
  isCollapsed = false,
  isChoiceInteractionDisabled,
  isObsessionSubmitDisabled,
  onToggleCollapsed,
  onChoiceClick,
  onContinue,
  onAutoChoiceToggle,
  onPreview,
  onObsessionInputChange,
  onObsessionSubmit,
}) => {
  const obsessionInputRef = useRef<HTMLTextAreaElement | null>(null);
  const collapsedDragStartRef = useRef<{
    pointerId: number;
    clientX: number;
    clientY: number;
    offsetX: number;
    offsetY: number;
  } | null>(null);
  const suppressCollapsedClickRef = useRef(false);
  const [collapsedOffset, setCollapsedOffset] = useState({ x: 0, y: 0 });
  const [isCollapsedDragging, setIsCollapsedDragging] = useState(false);
  const submitObsessionAction = () => onObsessionSubmit(obsessionInput.trim());

  const releaseCollapsedPointer = (
    event: React.PointerEvent<HTMLButtonElement>,
  ) => {
    collapsedDragStartRef.current = null;
    setIsCollapsedDragging(false);

    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const handleCollapsedPointerDown = (
    event: React.PointerEvent<HTMLButtonElement>,
  ) => {
    if (event.pointerType === "mouse" && event.button !== 0) {
      return;
    }

    collapsedDragStartRef.current = {
      pointerId: event.pointerId,
      clientX: event.clientX,
      clientY: event.clientY,
      offsetX: collapsedOffset.x,
      offsetY: collapsedOffset.y,
    };
    suppressCollapsedClickRef.current = false;
    setIsCollapsedDragging(true);
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const handleCollapsedPointerMove = (
    event: React.PointerEvent<HTMLButtonElement>,
  ) => {
    const start = collapsedDragStartRef.current;
    if (!start || start.pointerId !== event.pointerId) {
      return;
    }

    const deltaX = event.clientX - start.clientX;
    const deltaY = event.clientY - start.clientY;
    if (Math.abs(deltaX) > 4 || Math.abs(deltaY) > 4) {
      suppressCollapsedClickRef.current = true;
    }

    setCollapsedOffset({
      x: start.offsetX + deltaX,
      y: start.offsetY + deltaY,
    });
  };

  const handleCollapsedPointerUp = (
    event: React.PointerEvent<HTMLButtonElement>,
  ) => {
    const start = collapsedDragStartRef.current;
    if (!start || start.pointerId !== event.pointerId) {
      return;
    }

    releaseCollapsedPointer(event);
  };

  const handleCollapsedClick = () => {
    if (suppressCollapsedClickRef.current) {
      suppressCollapsedClickRef.current = false;
      return;
    }

    onToggleCollapsed?.();
  };

  useEffect(() => {
    if (!activeObsession) {
      return;
    }

    obsessionInputRef.current?.focus();
  }, [activeObsession]);

  if (!hasChoices && !canContinue) {
    return null;
  }

  if (isCollapsed) {
    return (
      <div className="flex w-full justify-center">
        <button
          type="button"
          onPointerDown={handleCollapsedPointerDown}
          onPointerMove={handleCollapsedPointerMove}
          onPointerUp={handleCollapsedPointerUp}
          onPointerCancel={releaseCollapsedPointer}
          onClick={handleCollapsedClick}
          className={`inline-flex h-9 touch-none items-center gap-1.5 rounded-full border border-[rgba(116,103,80,0.45)] bg-[rgba(5,11,22,0.84)] px-3 text-[0.72rem] font-semibold text-[#f3ead8] shadow-[0_10px_28px_rgba(0,0,0,0.38)] backdrop-blur-md transition-colors hover:border-[rgba(215,188,146,0.72)] hover:bg-[rgba(18,26,41,0.9)] sm:text-xs ${isCollapsedDragging ? "cursor-grabbing" : "cursor-grab"}`}
          style={{
            transform: `translate3d(${collapsedOffset.x}px, ${collapsedOffset.y}px, 0)`,
          }}
          aria-expanded="false"
          aria-label="拖拽移动选择入口，点击展开选择"
        >
          <MousePointer2 className="h-3.5 w-3.5" />
          选择
        </button>
      </div>
    );
  }

  return (
    <div className="flex w-full">
      <div className="game-choices flex-1 rounded-[1.1rem] border border-[rgba(116,103,80,0.42)] bg-[rgba(5,11,22,0.86)] px-1.5 py-2 shadow-[0_18px_48px_rgba(0,0,0,0.42)] backdrop-blur-md">
        <div className="flex items-center justify-between gap-2 px-0.5 mb-0.5">
          <button
            type="button"
            onClick={onToggleCollapsed}
            className="inline-flex h-8 items-center gap-1.5 rounded-full px-2 text-[0.68rem] font-medium text-[#d9cbb1] transition-colors hover:bg-[rgba(188,169,124,0.12)] hover:text-[#f3ead8] sm:text-xs"
            aria-expanded="true"
          >
            <ChevronDown className="h-3.5 w-3.5" />
            收起
          </button>
          {showAutoChoiceToggle ? (
            <button
              type="button"
              role="switch"
              aria-checked={autoChoiceEnabled}
              onClick={() => onAutoChoiceToggle?.(!autoChoiceEnabled)}
              className="inline-flex h-8 items-center gap-2 rounded-full border border-[rgba(116,103,80,0.48)] bg-[rgba(18,26,41,0.72)] px-3 text-[0.68rem] font-medium text-[#d9cbb1] transition-colors hover:border-[rgba(215,188,146,0.72)] hover:text-[#f3ead8] sm:text-xs"
              title="开发测试：自动选择第一个可用选项"
            >
              <Bot className="h-3.5 w-3.5" />
              <span>自动选择</span>
              <span
                className={[
                  "relative inline-flex h-4 w-7 shrink-0 items-center rounded-full border transition-colors",
                  autoChoiceEnabled
                    ? "border-[#d1b78d] bg-[#d1b78d]/85"
                    : "border-[#746750]/70 bg-[#1d283b]",
                ].join(" ")}
              >
                <span
                  className={[
                    "h-3 w-3 rounded-full bg-[#f7efe2] transition-transform",
                    autoChoiceEnabled ? "translate-x-3.5" : "translate-x-0.5",
                  ].join(" ")}
                />
              </span>
            </button>
          ) : (
            <span />
          )}
        </div>

        {!activeObsession ? (
          <div className="scrollbar-none max-h-[24dvh] touch-pan-y space-y-1 overflow-y-auto pr-0.5 py-0.5">
            {hasChoices ? (
              choices.map((choice) => (
                <div key={choice.id} className="space-y-1.5">
                  <div className="grid grid-cols-[minmax(0,1fr)_2.5rem] items-center gap-1.5">
                    <button
                      onClick={() => void onChoiceClick(choice)}
                      disabled={isChoiceInteractionDisabled || choice.disabled}
                      className="akashic-choice h-10 text-[#f3ead8] disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      <div className="flex min-h-7 items-center text-left">
                        <div className="w-full text-sm font-semibold leading-5 sm:text-[0.95rem]">
                          {choice.text}
                        </div>
                      </div>
                    </button>

                    <button
                      type="button"
                      onClick={(event) => void onPreview(choice, event)}
                      disabled={
                        isChoiceInteractionDisabled ||
                        (remainingIntuitionPoints <= 0 && !previews[choice.id])
                      }
                      className="akashic-icon-btn h-10 min-h-10 w-10 self-auto disabled:cursor-not-allowed disabled:opacity-50"
                      title={
                        previews[choice.id]
                          ? "再次查看记录碎片"
                          : remainingIntuitionPoints > 0
                            ? "消耗 1 点直觉，查看记录碎片"
                            : "本轮直觉已用尽"
                      }
                    >
                      <Eye className="h-4 w-4" />
                    </button>
                  </div>
                </div>
              ))
            ) : (
              <button
                type="button"
                onClick={() => void onContinue()}
                disabled={isChoiceInteractionDisabled}
                className="akashic-choice h-10 w-full text-[#f3ead8] disabled:cursor-not-allowed disabled:opacity-50"
              >
                <div className="flex min-h-7 items-center justify-center text-sm font-semibold leading-5 sm:text-[0.95rem]">
                  继续回响
                </div>
              </button>
            )}
          </div>
        ) : null}

        {activeObsession ? (
          <div className="space-y-2 rounded-[0.95rem] border border-red-400/30 bg-red-950/12 px-2 py-2.5">
            <textarea
              ref={obsessionInputRef}
              value={obsessionInput}
              onChange={(event) => onObsessionInputChange(event.target.value)}
              onKeyDown={(event) => {
                if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
                  event.preventDefault();
                  void submitObsessionAction();
                }
              }}
              disabled={isChoiceInteractionDisabled}
              placeholder="写下想强行写入记录的行动"
              className="min-h-24 w-full resize-none rounded-[0.85rem] border border-red-300/25 bg-[rgba(16,8,14,0.72)] px-3 py-2 text-sm leading-5 text-[#f7efe2] outline-none transition-colors placeholder:text-red-100/35 focus:border-red-300/45 disabled:cursor-not-allowed disabled:opacity-60"
            />

            <div className="flex items-center justify-between gap-2">
              <p className="text-[0.68rem] leading-4 text-red-100/65 sm:text-[0.72rem]"></p>
              <SecondaryButton
                type="button"
                onClick={() => void submitObsessionAction()}
                disabled={isObsessionSubmitDisabled}
                className="min-h-0 px-3 py-1.5 text-[0.72rem] leading-4 text-red-100 disabled:cursor-not-allowed disabled:opacity-60 sm:text-xs"
              >
                写入执念
              </SecondaryButton>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
};

export default ChoicePanel;
