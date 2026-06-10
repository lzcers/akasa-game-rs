let startupStageTimer: number | null = null;
let startupFlowRunId = 0;

export function startStartupFlow(): number {
  startupFlowRunId += 1;
  return startupFlowRunId;
}

export function currentStartupFlowRunId(): number {
  return startupFlowRunId;
}

export function invalidateStartupFlow() {
  startupFlowRunId += 1;
}

export function isStartupFlowCurrent(runId: number): boolean {
  return runId === startupFlowRunId;
}

export function clearStartupStageTimer() {
  if (startupStageTimer !== null) {
    window.clearTimeout(startupStageTimer);
    startupStageTimer = null;
  }
}

export function scheduleStartupStageProgress(onProgress: () => void) {
  clearStartupStageTimer();
  startupStageTimer = window.setTimeout(onProgress, 1400);
}

export function sleep(ms: number) {
  return new Promise<void>((resolve) => {
    window.setTimeout(resolve, ms);
  });
}

export function waitForNextPaint() {
  return new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => resolve());
  });
}
