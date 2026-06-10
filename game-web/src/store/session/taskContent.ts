import type { TaskView } from '../../lib/api';

export const STREAM_PLACEHOLDER_TEXT = '记录正在共鸣中...';

export function taskLabel(kind: string): string {
  switch (kind) {
    case 'simulation':
    case 'fate_planning':
      return '记录共鸣中...';
    case 'narration':
      return '回响展开中...';
    case 'protagonist_action':
      return '角色抉择';
    default:
      return '记录推进';
  }
}

export function taskContent(task: TaskView): string | null {
  if (task.status === 'done' && task.output != null) {
    return task.output;
  }

  if (task.chunks.length > 0) {
    return task.chunks.join('');
  }

  return task.output;
}

export function taskText(task: TaskView): string | null {
  const text = taskContent(task);
  if (!text?.trim()) {
    return null;
  }
  if (task.kind === 'narration') {
    return text;
  }
  return null;
}

export function taskRawContent(task: TaskView | null | undefined): string {
  return task ? taskContent(task) ?? '' : '';
}
