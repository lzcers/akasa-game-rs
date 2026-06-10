import React from 'react';
import { Clock3, Hourglass, Sparkles } from 'lucide-react';
import { StatusPill } from './AkashicUI';

interface GameplayHeaderProps {
  currentRound: number;
  currentScene: string;
  isLoading: boolean;
  statusLabel?: string;
}

const GameplayHeader: React.FC<GameplayHeaderProps> = ({
  currentRound,
  currentScene,
  isLoading,
  statusLabel,
}) => {
  const isFatePlanningScene = !statusLabel
    && isLoading
    && (currentScene.includes('记录共鸣') || currentScene.includes('命运编织'));
  const sceneStatusLabel = statusLabel ?? currentScene;

  return (
    <div className="flex flex-wrap gap-1 justify-between">
      <StatusPill icon={Clock3} className="px-2.5 py-1 text-[0.7rem] sm:text-xs">第 {currentRound} 轮</StatusPill>
      <StatusPill
        icon={statusLabel ? Sparkles : isFatePlanningScene ? Hourglass : null}
        iconClassName={isFatePlanningScene ? 'h-3 w-3 animate-spin' : undefined}
        className="px-2.5 py-1 text-[0.7rem] sm:text-xs"
      >
        {sceneStatusLabel}
      </StatusPill>
    </div>
  );
};

export default GameplayHeader;
