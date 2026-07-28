/** Android WebView host bridge injected as `window.BitFunGlassesHost`. */

export type GlassesVoiceEventType = 'started' | 'result' | 'error' | 'ended';

export type GlassesVoiceEventHandler = (
  type: GlassesVoiceEventType,
  payload?: string | null,
) => void;

interface BitFunGlassesHostBridge {
  rescanQr: () => void;
  isVoiceInputAvailable?: () => boolean | string;
  startVoiceInput?: () => void;
  stopVoiceInput?: () => void;
}

declare global {
  interface Window {
    BitFunGlassesHost?: BitFunGlassesHostBridge;
    __bitfunOnVoiceEvent?: GlassesVoiceEventHandler;
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

/** Append recognized speech into the composer without forcing trailing spaces twice. */
export function appendVoiceTranscript(current: string, transcript: string): string {
  const text = transcript.trim();
  if (!text) return current;
  const base = current.trimEnd();
  if (!base) return text;
  const needsSpace = !/[\s\n]$/.test(base) && !/^[,.!?，。！？、；：]/.test(text);
  return needsSpace ? `${base} ${text}` : `${base}${text}`;
}
