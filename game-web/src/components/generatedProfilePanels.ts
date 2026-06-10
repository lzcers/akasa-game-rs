import type { GeneratedProfiles } from "../lib/api";

export interface GeneratedProfilePanel {
  key: string;
  title: string;
  text: string;
  className: string;
}

export function generatedProfilePanels(
  profiles: GeneratedProfiles,
): GeneratedProfilePanel[] {
  return [
    {
      key: "world",
      title: "阿卡夏显影出的世界记录",
      text: profiles.world,
      className: "border-[#5b6f96]/30 bg-[#0f1624]/80 text-[#c7d5f2]",
    },
    {
      key: "protagonist",
      title: "阿卡夏显影出的角色记录",
      text: profiles.protagonist,
      className: "border-[#6f5f96]/30 bg-[#151325]/80 text-[#d8d0f2]",
    },
    {
      key: "beats",
      title: "第一条即将回响的剧情线索",
      text: profiles.keyStoryBeats,
      className: "border-[#8a7755]/30 bg-[#17120f]/80 text-[#efe4cd]/88",
    },
  ];
}
