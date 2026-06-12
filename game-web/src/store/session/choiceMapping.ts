import type {
  Choice,
  ChoiceExploration,
  PendingCharacterChoice,
} from '../../lib/api';

export function toChoiceFromSession(
  choice: PendingCharacterChoice,
  exploration?: ChoiceExploration,
): Choice {
  const motivationAndRisk = choice.option.motivationAndRisk?.trim()
    || choice.option.motivation_and_risk?.trim();

  return {
    id: choice.id,
    text: choice.option.title || choice.option.action,
    action: choice.option.action,
    motivationAndRisk,
    visited: exploration?.visited ?? false,
    disabled: false,
  };
}
