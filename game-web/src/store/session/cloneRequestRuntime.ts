interface CloneResult {
  sessionId: string;
  isEnding: boolean;
}

let activeCloneRequest: {
  sourceSessionId: string;
  sourceRound: number | null;
  promise: Promise<CloneResult>;
} | null = null;

export function getActiveCloneRequest(
  sourceSessionId: string,
  sourceRound: number | null,
): Promise<CloneResult> | null {
  return activeCloneRequest?.sourceSessionId === sourceSessionId
    && activeCloneRequest.sourceRound === sourceRound
    ? activeCloneRequest.promise
    : null;
}

export function trackCloneRequest(
  sourceSessionId: string,
  sourceRound: number | null,
  promise: Promise<CloneResult>,
) {
  activeCloneRequest = {
    sourceSessionId,
    sourceRound,
    promise,
  };

  promise.finally(() => {
    if (
      activeCloneRequest?.sourceSessionId === sourceSessionId
      && activeCloneRequest.sourceRound === sourceRound
    ) {
      activeCloneRequest = null;
    }
  });

  return promise;
}
