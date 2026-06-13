import type { BranchExploration, Choice } from "../lib/api";

export interface NarrationRoundEntry {
  nodeId: string | null;
  round: number;
  title: string;
  narrationText: string;
  narrationStatus: 'pending' | 'running' | 'done' | 'error' | null;
  choices: Choice[];
  branchExplorations: BranchExploration[];
  selectedChoiceText: string | null;
  selectedChoiceAction: string | null;
  isAwaitingNarration: boolean;
}
