import type { Choice } from '../../lib/api';

interface StreamedCharacterOption {
  title?: string;
  action?: string;
  motivation_and_risk?: string;
  motivationAndRisk?: string;
}

function toChoiceFromStreamOption(option: StreamedCharacterOption, index: number): Choice {
  const action = option.action?.trim() || '';
  return {
    id: `choice-${index + 1}`,
    text: option.title?.trim() || '',
    action,
    motivationAndRisk: option.motivationAndRisk?.trim() || option.motivation_and_risk?.trim(),
    disabled: false,
  };
}

function parseCharacterChoicesPayload(raw: string): Choice[] | null {
  try {
    const parsed = JSON.parse(raw) as { options?: StreamedCharacterOption[] };
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

function parseStreamingCharacterChoices(raw: string): Choice[] | null {
  const parsed = parseCharacterChoicesPayload(raw);
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
      return [JSON.parse(item) as StreamedCharacterOption];
    } catch {
      return [];
    }
  });

  return options.map(toChoiceFromStreamOption);
}

export function characterActionChoices(raw: string): Choice[] | null {
  if (!raw.trim()) {
    return null;
  }

  return parseStreamingCharacterChoices(raw);
}

export function characterActionText(raw: string): string | null {
  if (!raw.trim()) {
    return null;
  }

  try {
    const parsedChoices = parseStreamingCharacterChoices(raw);
    if (parsedChoices) {
      if (parsedChoices.length === 0) {
        return '前路暂时未显，请稍等记录展开。';
      }
      return parsedChoices.map((choice) => choice.text).join(' / ');
    }

    const parsed = JSON.parse(raw) as { options?: StreamedCharacterOption[] };
    const options = parsed.options ?? [];
    if (options.length === 0) {
      return '前路暂时未显，请稍等记录展开。';
    }
    return options
      .map((option) => option.title?.trim() || '')
      .join(' / ');
  } catch {
    return raw.trim();
  }
}
