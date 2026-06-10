import type {
  Choice,
  PendingProtagonistChoice,
} from '../../lib/api';

export function toChoiceFromSession(choice: PendingProtagonistChoice): Choice {
  const motivationAndRisk = choice.option.motivationAndRisk?.trim()
    || choice.option.motivation_and_risk?.trim();

  return {
    id: choice.id,
    text: choice.option.title || choice.option.action,
    action: choice.option.action,
    motivationAndRisk,
    disabled: false,
  };
}
