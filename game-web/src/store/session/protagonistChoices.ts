import type {
  Choice,
  TaskView,
} from '../../lib/api';
import { taskContent } from './taskContent';

interface StreamedProtagonistOption {
  title?: string;
  action?: string;
  motivation_and_risk?: string;
  motivationAndRisk?: string;
}

function toChoiceFromStreamOption(option: StreamedProtagonistOption, index: number): Choice {
  const action = option.action?.trim() || '';
  return {
    id: `choice-${index + 1}`,
    text: option.title?.trim() || action || `行动 ${index + 1}`,
    action,
    motivationAndRisk: option.motivationAndRisk?.trim() || option.motivation_and_risk?.trim(),
    disabled: false,
  };
}

function parseProtagonistChoicesPayload(raw: string): Choice[] | null {
  try {
    const parsed = JSON.parse(raw) as { options?: StreamedProtagonistOption[] };
    return (parsed.options ?? []).map(toChoiceFromStreamOption);
  } catch {
    return null;
  }
}

function extractCompletedJsonObjects(raw: string): string[] {
  const objects: string[] = [];
  let depth = 0;
  let startIndex = -1;
  let isInString = false;
  let isEscaping = false;

  for (let index = 0; index < raw.length; index += 1) {
    const char = raw[index];

    if (isEscaping) {
      isEscaping = false;
      continue;
    }

    if (char === '\\' && isInString) {
      isEscaping = true;
      continue;
    }

    if (char === '"') {
      isInString = !isInString;
      continue;
    }

    if (isInString) {
      continue;
    }

    if (char === '{') {
      if (depth === 0) {
        startIndex = index;
      }
      depth += 1;
      continue;
    }

    if (char === '}') {
      depth -= 1;
      if (depth === 0 && startIndex >= 0) {
        objects.push(raw.slice(startIndex, index + 1));
        startIndex = -1;
      }
    }
  }

  return objects;
}

function parseStreamingProtagonistChoices(raw: string): Choice[] | null {
  const parsed = parseProtagonistChoicesPayload(raw);
  if (parsed) {
    return parsed;
  }

  const optionsMatch = raw.match(/"options"\s*:\s*\[/);
  if (!optionsMatch) {
    return null;
  }

  const optionSection = raw.slice((optionsMatch.index ?? 0) + optionsMatch[0].length);
  const optionObjects = extractCompletedJsonObjects(optionSection);
  if (optionObjects.length === 0) {
    return null;
  }

  const options = optionObjects.flatMap((item) => {
    try {
      return [JSON.parse(item) as StreamedProtagonistOption];
    } catch {
      return [];
    }
  });

  return options.map(toChoiceFromStreamOption);
}

export function protagonistActionChoices(task: TaskView): Choice[] | null {
  if (task.kind !== 'protagonist_action') {
    return null;
  }

  const raw = taskContent(task);
  if (!raw?.trim()) {
    return null;
  }

  return parseStreamingProtagonistChoices(raw);
}

export function protagonistActionText(task: TaskView): string | null {
  const raw = taskContent(task);
  if (task.kind !== 'protagonist_action' || !raw?.trim()) {
    return null;
  }

  try {
    const parsedChoices = parseStreamingProtagonistChoices(raw);
    if (parsedChoices) {
      if (parsedChoices.length === 0) {
        return '前路暂时未显，请稍等记录展开。';
      }
      return parsedChoices.map((choice) => choice.text).join(' / ');
    }

    const parsed = JSON.parse(raw) as { options?: StreamedProtagonistOption[] };
    const options = parsed.options ?? [];
    if (options.length === 0) {
      return '前路暂时未显，请稍等记录展开。';
    }
    return options
      .map((option, index) => option.title?.trim() || option.action?.trim() || `行动 ${index + 1}`)
      .join(' / ');
  } catch {
    return raw.trim();
  }
}
