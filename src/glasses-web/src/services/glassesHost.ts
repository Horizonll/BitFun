/** Android WebView host bridge injected as `window.BitFunGlassesHost`. */

export type GlassesVoiceEventType = 'started' | 'result' | 'error' | 'ended' | 'info';

export type GlassesVoiceEventHandler = (
  type: GlassesVoiceEventType,
  payload?: string | null,
) => void;

interface BitFunGlassesHostBridge {
  rescanQr: () => void;
  exitApp?: () => void;
  isVoiceInputAvailable?: () => boolean | string;
  startVoiceInput?: () => void;
  stopVoiceInput?: () => void;
  getSharedInstallId?: () => string;
  getSharedLanguage?: () => string;
  putSharedLanguage?: (language: string) => void;
  takePendingAsrPcm?: (requestId: string) => string;
  completeDesktopAsr?: (requestId: string, text: string) => void;
  failDesktopAsr?: (requestId: string, error: string) => void;
}

export type DesktopAsrRequestHandler = (requestId: string) => void;

declare global {
  interface Window {
    BitFunGlassesHost?: BitFunGlassesHostBridge;
    __bitfunOnVoiceEvent?: GlassesVoiceEventHandler;
    __bitfunOnDesktopAsrRequest?: DesktopAsrRequestHandler;
    /** Glasses chat: temple single-click toggles voice capture. */
    __bitfunVoiceToggle?: () => void;
  }
}

function coerceBridgeBoolean(value: unknown): boolean {
  if (typeof value === 'boolean') return value;
  if (typeof value === 'number') return value !== 0;
  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase();
    return normalized === 'true' || normalized === '1';
  }
  return false;
}

/** Ask the native host to reopen the QR scanner. Returns false when unavailable. */
export function requestRescanQr(): boolean {
  const host = window.BitFunGlassesHost;
  if (!host || typeof host.rescanQr !== 'function') {
    return false;
  }
  try {
    host.rescanQr();
    return true;
  } catch {
    return false;
  }
}

/** Ask the native host to finish the activity (leave the app). */
export function requestExitApp(): boolean {
  const host = window.BitFunGlassesHost;
  if (!host || typeof host.exitApp !== 'function') {
    return false;
  }
  try {
    host.exitApp();
    return true;
  } catch {
    return false;
  }
}

/**
 * True when the native host exposes a voice-input entrypoint.
 * Do not trust OEM capability probes for button visibility — they often
 * return false even when the system recognizer activity can still launch.
 */
export function isVoiceInputAvailable(): boolean {
  const host = window.BitFunGlassesHost;
  if (!host) return false;
  return typeof host.startVoiceInput === 'function';
}

/** Optional native capability probe used only for diagnostics / toasts. */
export function probeNativeVoiceCapability(): boolean | null {
  const host = window.BitFunGlassesHost;
  if (!host || typeof host.isVoiceInputAvailable !== 'function') return null;
  try {
    return coerceBridgeBoolean(host.isVoiceInputAvailable());
  } catch {
    return null;
  }
}

/** Start native speech recognition. Returns false when unavailable. */
export function startVoiceInput(): boolean {
  const host = window.BitFunGlassesHost;
  if (!host || typeof host.startVoiceInput !== 'function') {
    return false;
  }
  try {
    host.startVoiceInput();
    return true;
  } catch {
    return false;
  }
}

/** Stop / dismiss the active native speech recognition session when possible. */
export function stopVoiceInput(): boolean {
  const host = window.BitFunGlassesHost;
  if (!host || typeof host.stopVoiceInput !== 'function') {
    return false;
  }
  try {
    host.stopVoiceInput();
    return true;
  } catch {
    return false;
  }
}

/** Shared install id across activity restarts (native SharedPreferences). */
export function getSharedInstallId(): string | null {
  const host = window.BitFunGlassesHost;
  if (!host || typeof host.getSharedInstallId !== 'function') return null;
  try {
    const value = host.getSharedInstallId()?.trim();
    return value || null;
  } catch {
    return null;
  }
}

export function getSharedLanguage(): string | null {
  const host = window.BitFunGlassesHost;
  if (!host || typeof host.getSharedLanguage !== 'function') return null;
  try {
    const value = host.getSharedLanguage()?.trim();
    return value || null;
  } catch {
    return null;
  }
}

export function putSharedLanguage(language: string): boolean {
  const host = window.BitFunGlassesHost;
  if (!host || typeof host.putSharedLanguage !== 'function') return false;
  try {
    host.putSharedLanguage(language);
    return true;
  } catch {
    return false;
  }
}

/**
 * Subscribe to native voice events. Replaces any previous handler.
 * Returns an unsubscribe function that clears the callback when it still owns it.
 */
export function subscribeVoiceInput(handler: GlassesVoiceEventHandler): () => void {
  window.__bitfunOnVoiceEvent = handler;
  return () => {
    if (window.__bitfunOnVoiceEvent === handler) {
      delete window.__bitfunOnVoiceEvent;
    }
  };
}

/** Subscribe to native "please transcribe this PCM on desktop" requests. */
export function subscribeDesktopAsrRequest(handler: DesktopAsrRequestHandler): () => void {
  window.__bitfunOnDesktopAsrRequest = handler;
  return () => {
    if (window.__bitfunOnDesktopAsrRequest === handler) {
      delete window.__bitfunOnDesktopAsrRequest;
    }
  };
}

export function takePendingAsrPcm(requestId: string): string | null {
  const host = window.BitFunGlassesHost;
  if (!host || typeof host.takePendingAsrPcm !== 'function') return null;
  try {
    const value = host.takePendingAsrPcm(requestId)?.trim();
    return value || null;
  } catch {
    return null;
  }
}

export function completeDesktopAsr(requestId: string, text: string): boolean {
  const host = window.BitFunGlassesHost;
  if (!host || typeof host.completeDesktopAsr !== 'function') return false;
  try {
    host.completeDesktopAsr(requestId, text);
    return true;
  } catch {
    return false;
  }
}

export function failDesktopAsr(requestId: string, error: string): boolean {
  const host = window.BitFunGlassesHost;
  if (!host || typeof host.failDesktopAsr !== 'function') return false;
  try {
    host.failDesktopAsr(requestId, error);
    return true;
  } catch {
    return false;
  }
}

/** Append recognized speech into the composer without forcing trailing spaces twice. */
export function appendVoiceTranscript(current: string, transcript: string): string {
  const text = transcript.trim();
  if (!text) return current;
  const base = current.trimEnd();
  if (!base) return text;
  const needsSpace = !/[\s\n]$/.test(base) && !/^[,.!?，。！？、；：]/.test(text);
  return needsSpace ? `${base} ${text}` : `${base}${text}`;
}
