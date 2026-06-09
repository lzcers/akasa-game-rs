let suppressedSessionRestoreId: string | null = null;

export function suppressSessionRestore(sessionId: string | null | undefined) {
  suppressedSessionRestoreId = sessionId?.trim() || null;
}

export function isSessionRestoreSuppressed(sessionId: string | null): boolean {
  return Boolean(sessionId && suppressedSessionRestoreId === sessionId);
}

export function clearSuppressedSessionRestore() {
  suppressedSessionRestoreId = null;
}
