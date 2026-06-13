interface CloneResult {
  sessionId: string;
  isEnding: boolean;
}

let activeCloneRequest: {
  sourceSessionId: string;
  sourceNodeId: string | null;
  promise: Promise<CloneResult>;
} | null = null;

export function getActiveCloneRequest(
  sourceSessionId: string,
  sourceNodeId: string | null,
): Promise<CloneResult> | null {
  return activeCloneRequest?.sourceSessionId === sourceSessionId
    && activeCloneRequest.sourceNodeId === sourceNodeId
    ? activeCloneRequest.promise
    : null;
}

export function trackCloneRequest(
  sourceSessionId: string,
  sourceNodeId: string | null,
  promise: Promise<CloneResult>,
) {
  activeCloneRequest = {
    sourceSessionId,
    sourceNodeId,
    promise,
  };

  promise.finally(() => {
    if (
      activeCloneRequest?.sourceSessionId === sourceSessionId
      && activeCloneRequest.sourceNodeId === sourceNodeId
    ) {
      activeCloneRequest = null;
    }
  });

  return promise;
}
