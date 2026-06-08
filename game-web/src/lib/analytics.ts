import { readSessionIdFromSearch, isCloneShareSearch } from './appRoutes';

type AnalyticsProperties = Record<string, unknown>;

interface AttributionSnapshot {
  utmSource: string | null;
  utmMedium: string | null;
  utmCampaign: string | null;
  referrerDomain: string | null;
  sourceSessionId: string | null;
}

interface AnalyticsEventPayload {
  id: string;
  eventName: string;
  anonymousUserId: string;
  clientSessionId: string;
  gameSessionId: string | null;
  sourceSessionId: string | null;
  occurredAt: string;
  app: string;
  appVersion: string | null;
  path: string;
  referrerDomain: string | null;
  utmSource: string | null;
  utmMedium: string | null;
  utmCampaign: string | null;
  deviceType: string;
  platform: string;
  properties: AnalyticsProperties;
}

const API_ORIGIN = import.meta.env.PROD ? 'https://game.akasa.fun' : '';
const ANALYTICS_ENDPOINT = `${API_ORIGIN}/api/analytics/events`;
const APP_NAME = 'game-web';
const APP_VERSION = typeof import.meta.env.VITE_APP_VERSION === 'string'
  ? import.meta.env.VITE_APP_VERSION
  : null;
const ANONYMOUS_USER_STORAGE_KEY = 'akasa:analytics-anonymous-user-id';
const FIRST_ATTRIBUTION_STORAGE_KEY = 'akasa:analytics-first-attribution';
const LAST_ATTRIBUTION_STORAGE_KEY = 'akasa:analytics-last-attribution';

let clientSessionId: string | null = null;
let gameSessionId: string | null = null;
let sourceSessionId: string | null = null;
let lastAttribution: AttributionSnapshot | null = null;
let queue: AnalyticsEventPayload[] = [];
let flushTimer: number | null = null;

function canUseBrowserStorage() {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined';
}

function createId(prefix: string) {
  const cryptoApi = globalThis.crypto;
  if (typeof cryptoApi?.randomUUID === 'function') {
    return `${prefix}-${cryptoApi.randomUUID().replace(/-/g, '')}`;
  }
  return `${prefix}-${Date.now().toString(16)}${Math.random().toString(16).slice(2)}`;
}

function getAnonymousUserId() {
  if (!canUseBrowserStorage()) {
    return createId('anon');
  }

  const existing = window.localStorage.getItem(ANONYMOUS_USER_STORAGE_KEY);
  if (existing) {
    return existing;
  }

  const created = createId('anon');
  window.localStorage.setItem(ANONYMOUS_USER_STORAGE_KEY, created);
  return created;
}

function getClientSessionId() {
  if (!clientSessionId) {
    clientSessionId = createId('visit');
  }
  return clientSessionId;
}

function readSearchParam(search: string, key: string) {
  return new URLSearchParams(search).get(key)?.trim() || null;
}

function referrerDomain() {
  if (!document.referrer) {
    return null;
  }

  try {
    return new URL(document.referrer).hostname || null;
  } catch {
    return null;
  }
}

function readAttributionFromLocation(): AttributionSnapshot {
  const search = window.location.search;
  const clonedSourceSessionId = isCloneShareSearch(search)
    ? readSessionIdFromSearch(search)
    : null;

  return {
    utmSource: readSearchParam(search, 'utm_source'),
    utmMedium: readSearchParam(search, 'utm_medium'),
    utmCampaign: readSearchParam(search, 'utm_campaign'),
    referrerDomain: referrerDomain(),
    sourceSessionId: clonedSourceSessionId,
  };
}

function hasAttributionSignal(attribution: AttributionSnapshot) {
  return Boolean(
    attribution.utmSource
      || attribution.utmMedium
      || attribution.utmCampaign
      || attribution.referrerDomain
      || attribution.sourceSessionId,
  );
}

function writeStoredJson(key: string, value: unknown) {
  if (!canUseBrowserStorage()) {
    return;
  }

  window.localStorage.setItem(key, JSON.stringify(value));
}

function readStoredJson<T>(key: string): T | null {
  if (!canUseBrowserStorage()) {
    return null;
  }

  const raw = window.localStorage.getItem(key);
  if (!raw) {
    return null;
  }

  try {
    return JSON.parse(raw) as T;
  } catch {
    return null;
  }
}

function currentAttribution() {
  if (lastAttribution) {
    return lastAttribution;
  }

  return readStoredJson<AttributionSnapshot>(LAST_ATTRIBUTION_STORAGE_KEY) ?? {
    utmSource: null,
    utmMedium: null,
    utmCampaign: null,
    referrerDomain: null,
    sourceSessionId: null,
  };
}

function currentPath() {
  return `${window.location.pathname}${window.location.search}`;
}

function deviceType() {
  return window.matchMedia('(pointer: coarse)').matches ? 'mobile' : 'desktop';
}

function scheduleFlush() {
  if (flushTimer !== null) {
    return;
  }

  flushTimer = window.setTimeout(() => {
    flushTimer = null;
    void flushAnalytics();
  }, 1200);
}

function sendEvents(events: AnalyticsEventPayload[]) {
  const body = JSON.stringify({ events });
  const blob = new Blob([body], { type: 'application/json' });

  if (navigator.sendBeacon?.(ANALYTICS_ENDPOINT, blob)) {
    return Promise.resolve();
  }

  return fetch(ANALYTICS_ENDPOINT, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body,
    keepalive: true,
  }).then(() => undefined);
}

export function captureAttribution() {
  const attribution = readAttributionFromLocation();
  const previousFirst = readStoredJson<AttributionSnapshot>(FIRST_ATTRIBUTION_STORAGE_KEY);
  const nextAttribution = hasAttributionSignal(attribution)
    ? attribution
    : currentAttribution();

  if (!previousFirst && hasAttributionSignal(nextAttribution)) {
    writeStoredJson(FIRST_ATTRIBUTION_STORAGE_KEY, nextAttribution);
  }

  if (hasAttributionSignal(nextAttribution)) {
    writeStoredJson(LAST_ATTRIBUTION_STORAGE_KEY, nextAttribution);
  }

  lastAttribution = nextAttribution;
  sourceSessionId = nextAttribution.sourceSessionId;
}

export function setAnalyticsGameSessionId(nextGameSessionId: string | null) {
  gameSessionId = nextGameSessionId;
}

export function getAnalyticsSourceSessionId() {
  return sourceSessionId;
}

export function track(eventName: string, properties: AnalyticsProperties = {}) {
  if (typeof window === 'undefined') {
    return;
  }

  const attribution = currentAttribution();
  const eventSourceSessionId = typeof properties.sourceSessionId === 'string'
    ? properties.sourceSessionId
    : sourceSessionId ?? attribution.sourceSessionId;

  queue.push({
    id: createId('evt'),
    eventName,
    anonymousUserId: getAnonymousUserId(),
    clientSessionId: getClientSessionId(),
    gameSessionId,
    sourceSessionId: eventSourceSessionId,
    occurredAt: new Date().toISOString(),
    app: APP_NAME,
    appVersion: APP_VERSION,
    path: currentPath(),
    referrerDomain: attribution.referrerDomain,
    utmSource: attribution.utmSource,
    utmMedium: attribution.utmMedium,
    utmCampaign: attribution.utmCampaign,
    deviceType: deviceType(),
    platform: navigator.platform || 'unknown',
    properties,
  });
  scheduleFlush();
}

export async function flushAnalytics() {
  if (queue.length === 0) {
    return;
  }

  const batch = queue;
  queue = [];

  try {
    await sendEvents(batch);
  } catch {
    // Analytics must never interrupt gameplay.
  }
}

if (typeof window !== 'undefined') {
  window.addEventListener('pagehide', () => {
    if (flushTimer !== null) {
      window.clearTimeout(flushTimer);
      flushTimer = null;
    }
    void flushAnalytics();
  });
}
