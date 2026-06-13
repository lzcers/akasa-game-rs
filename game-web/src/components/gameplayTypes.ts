import type { Choice } from "../lib/api";

export interface NarrationRoundEntry {
  nodeId: string | null;
  round: number;
  title: string;
  narrationText: string;
  narrationStatus: 'pending' | 'running' | 'done' | 'error' | null;
  choices: Choice[];
  selectedChoiceText: string | null;
  selectedChoiceAction: string | null;
  isAwaitingNarration: boolean;
}
