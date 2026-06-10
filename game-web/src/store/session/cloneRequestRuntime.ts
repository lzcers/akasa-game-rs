interface CloneResult {
  sessionId: string;
  isEnding: boolean;
}

let activeCloneRequest: {
  sourceSessionId: string;
  promise: Promise<CloneResult>;
} | null = null;

export function getActiveCloneRequest(sourceSessionId: string): Promise<CloneResult> | null {
  return activeCloneRequest?.sourceSessionId === sourceSessionId
    ? activeCloneRequest.promise
    : null;
}

export function trackCloneRequest(sourceSessionId: string, promise: Promise<CloneResult>) {
  activeCloneRequest = {
    sourceSessionId,
    promise,
  };

  promise.finally(() => {
    if (activeCloneRequest?.sourceSessionId === sourceSessionId) {
      activeCloneRequest = null;
    }
  });

  return promise;
}
