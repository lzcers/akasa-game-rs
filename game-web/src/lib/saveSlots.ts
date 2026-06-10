export interface StoredSaveSlot {
  slotId: string;
  sessionId: string;
  title: string;
  createdAt: string;
  updatedAt: string;
}

const SAVE_SLOTS_STORAGE_KEY = 'akashic-save-slots';
const SAVE_ARCHIVE_STORAGE_KEY_PREFIX = 'akashic-save-archive:';

function canUseLocalStorage() {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

export function createStoredSaveSlotId() {
  const cryptoApi = globalThis.crypto;
  if (typeof cryptoApi?.randomUUID === 'function') {
    return `slot-${cryptoApi.randomUUID().replace(/-/g, '')}`;
  }

  if (typeof cryptoApi?.getRandomValues === 'function') {
    const randomBytes = new Uint8Array(16);
    cryptoApi.getRandomValues(randomBytes);
    const randomToken = Array.from(randomBytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
    return `slot-${randomToken}`;
  }

  return `slot-${Date.now().toString(16)}${Math.random().toString(16).slice(2)}`;
}

export function readStoredSaveSlots(): StoredSaveSlot[] {
  if (!canUseLocalStorage()) {
    return [];
  }

  const raw = window.localStorage.getItem(SAVE_SLOTS_STORAGE_KEY);
  if (!raw) {
    return [];
  }

  try {
    const parsed = JSON.parse(raw) as StoredSaveSlot[];
    if (!Array.isArray(parsed)) {
      return [];
    }
    return parsed
      .filter((item) => item && typeof item.slotId === 'string')
      .sort((left, right) => right.updatedAt.localeCompare(left.updatedAt));
  } catch {
    return [];
  }
}

export function upsertStoredSaveSlot(slot: StoredSaveSlot) {
  if (!canUseLocalStorage()) {
    return;
  }

  const nextSlots = [
    slot,
    ...readStoredSaveSlots().filter((item) => item.slotId !== slot.slotId),
  ];
  window.localStorage.setItem(SAVE_SLOTS_STORAGE_KEY, JSON.stringify(nextSlots));
}

export function writeStoredSaveArchive(slotId: string, compressedArchive: string) {
  if (!canUseLocalStorage()) {
    return;
  }

  window.localStorage.setItem(
    `${SAVE_ARCHIVE_STORAGE_KEY_PREFIX}${slotId}`,
    compressedArchive,
  );
}

export function readStoredSaveArchive(slotId: string): string | null {
  if (!canUseLocalStorage()) {
    return null;
  }

  const raw = window.localStorage.getItem(`${SAVE_ARCHIVE_STORAGE_KEY_PREFIX}${slotId}`);
  if (!raw) {
    return null;
  }

  try {
    return raw;
  } catch {
    return null;
  }
}

export function removeStoredSaveArchive(slotId: string) {
  if (!canUseLocalStorage()) {
    return;
  }

  window.localStorage.removeItem(`${SAVE_ARCHIVE_STORAGE_KEY_PREFIX}${slotId}`);
}

export function removeStoredSaveSlot(slotId: string) {
  if (!canUseLocalStorage()) {
    return;
  }

  const nextSlots = readStoredSaveSlots().filter((item) => item.slotId !== slotId);
  window.localStorage.setItem(SAVE_SLOTS_STORAGE_KEY, JSON.stringify(nextSlots));
  removeStoredSaveArchive(slotId);
}
