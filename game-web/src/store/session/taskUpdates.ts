import type {
  TaskUpdatedEvent,
  TaskView,
} from '../../lib/api';

export function applyTaskUpdate(tasks: Map<string, TaskView>, update: TaskUpdatedEvent): TaskView {
  const existingTask = tasks.get(update.entity);
  const shouldResetTask =
    update.status === 'pending'
    || !existingTask
    || existingTask.kind !== update.kind;
  const currentTask = shouldResetTask ? {
    entity: update.entity,
    kind: update.kind,
    status: 'pending',
    attempts: 1,
    maxAttempts: 1,
    lastError: null,
    chunks: [],
    output: null,
    error: null,
  } : existingTask;

  const nextTask: TaskView = {
    ...currentTask,
    kind: update.kind,
    status: update.status,
    chunks: [...currentTask.chunks],
  };

  if (update.chunk != null) {
    nextTask.chunks.push(update.chunk);
  }

  if (update.error !== undefined) {
    nextTask.error = update.error;
    nextTask.lastError = update.error;
  }

  if (update.status === 'done') {
    nextTask.error = null;
    nextTask.lastError = null;
  }

  tasks.set(update.entity, nextTask);
  return nextTask;
}
