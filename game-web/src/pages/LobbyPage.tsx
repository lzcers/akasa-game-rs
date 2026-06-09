import React, { useEffect } from "react";
import { Library, Play, TriangleAlert } from "lucide-react";
import { useNavigate } from "react-router-dom";
import {
  PageTitle,
  PrimaryButton,
  ScreenShell,
  SecondaryButton,
  SectionCard,
  StoryFrame,
  StatusPill,
} from "../components/AkashicUI";
import { appRoutes } from "../lib/appRoutes";
import { useGameUIStore } from "../store/gameUIStore";
import { clearSuppressedSessionRestore } from "../lib/sessionRestore";

const LobbyPage: React.FC = () => {
  const navigate = useNavigate();
  const resetGame = useGameUIStore((state) => state.resetGame);
  const isLoading = useGameUIStore((state) => state.isLoading);
  const error = useGameUIStore((state) => state.error);

  useEffect(() => {
    clearSuppressedSessionRestore();
  }, []);

  const handleStart = () => {
    resetGame();
    navigate(appRoutes.creation);
  };

  return (
    <ScreenShell className="items-center">
      <StoryFrame className="overflow-hidden p-5 sm:p-6 md:p-8">
        <div
          className="absolute inset-0 bg-cover bg-center bg-no-repeat opacity-28"
          style={{
            backgroundImage:
              'url("https://coresg-normal.trae.ai/api/ide/v1/text_to_image?prompt=A%20mystical%20ancient%20archive%20with%20rainy%20blue%20atmosphere%2C%20dark%20fantasy%20ui%20background%2C%20cinematic%20concept%20art&image_size=landscape_16_9")',
          }}
        />
        <div className="absolute inset-0 bg-[radial-gradient(circle_at_28%_18%,rgba(240,223,194,0.18),transparent_28%),linear-gradient(135deg,rgba(8,14,26,0.28),rgba(8,14,26,0.86)_62%,rgba(3,7,14,0.95))]" />
        <div className="relative z-10">
          <div className="mx-auto max-w-3xl space-y-6">
            <PageTitle title="阿卡夏·回响" subtitle="从记录中共鸣出你想要的世界与角色。" />
            {error ? (
              <StatusPill
                icon={TriangleAlert}
                className="border-[#7f3b3b]/50 bg-[#2a1216]/85 text-[#ffd7d7]"
                iconClassName="text-[#ff9b9b]"
              >
                {error}
              </StatusPill>
            ) : null}
            <SectionCard className="space-y-5">
              <div className="space-y-3 text-center">
                <p className="text-lg font-semibold leading-8 text-[#f6eddc] sm:text-xl">
                  写下你想要的世界与角色，记录便会回应。
                </p>
                <p className="mx-auto max-w-2xl text-sm leading-7 text-[#b9c3d4] sm:text-base">
                  阿卡夏会读取姓名、烙印、角色气质与世界种子，从记录中显影一段只属于你的开局。每次选择，都会让分支继续回响。
                </p>
              </div>
              <div className="rounded-xl border border-[#6f6655]/35 bg-[#0c1424]/56 px-4 py-3 text-center">
                <p className="text-sm font-medium leading-7 text-[#e4d7bd] sm:text-base">
                  设定唤起记录，共鸣生成世界，选择写成新的因果。
                </p>
              </div>
            </SectionCard>
            <div className="flex flex-col gap-3 sm:flex-row">
              <PrimaryButton
                onClick={handleStart}
                disabled={isLoading}
                className="flex-1"
              >
                <Play className="h-4 w-4" />
                开始共鸣
              </PrimaryButton>
              <SecondaryButton
                onClick={() => navigate(appRoutes.archives)}
                disabled={isLoading}
                className="flex-1"
              >
                <Library className="h-4 w-4" />
                续读旧日记录
              </SecondaryButton>
            </div>
          </div>
        </div>
      </StoryFrame>
    </ScreenShell>
  );
};

export default LobbyPage;
