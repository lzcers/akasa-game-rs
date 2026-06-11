export const STREAM_PLACEHOLDER_TEXT = '记录正在共鸣中...';

export function streamEntityLabel(entityName: string): string {
  switch (entityName) {
    case 'FateWeaver':
    case 'simulation':
      return '记录共鸣中...';
    case 'UpperNarrator':
    case 'narration':
      return '回响展开中...';
    case 'CharacterAgent':
    case 'character_action':
      return '角色抉择';
    default:
      return '记录推进';
  }
}
