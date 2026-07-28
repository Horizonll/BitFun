import React, {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import LanguageToggleButton from '../components/LanguageToggleButton';
import { useI18n } from '../i18n';
import type { RelayHttpClient } from '../services/RelayHttpClient';
import {
  RemoteSessionManager,
  SessionPoller,
  type ChatMessage,
  type PollResponse,
} from '../services/RemoteSessionManager';
import {
  deriveHudActivity,
  lastAssistantPlainText,
  turnOutputRaw,
} from '../services/hudActivity';
import {
  resetActiveTurnHold,
  resolvePolledActiveTurn,
  shouldClearHeldTurnAfterRefresh,
  shouldRefreshMessagesAfterTurn,
  TURN_END_REFRESH_DELAYS_MS,
} from '../services/activeTurnPoll';
import { useMobileStore, type ConnectionHealth } from '../services/store';
import SessionListPage from './SessionListPage';
import DevicesPage from './DevicesPage';

const ChatPage = lazy(() => import('./ChatPage'));

/** Stable empty list so Zustand selectors do not return a new [] every render. */
const EMPTY_MESSAGES: ChatMessage[] = [];

export interface VrShellProps {
  sessionMgr: RemoteSessionManager;
  client: RelayHttpClient | null;
  onDisconnect: () => void;
  onRescan?: () => void;
}

type VrChromeMode = 'hud' | 'focus';

function connectionLabel(health: ConnectionHealth, t: (key: string) => string): string {
  switch (health) {
    case 'connected':
      return t('sessions.connectionConnected');
    case 'checking':
      return t('sessions.connectionChecking');
    case 'unreachable':
      return t('sessions.connectionUnreachable');
    default:
      return t('sessions.connectionUnpaired');
  }
}

/**
 * Optical-AR shell: fixed left/right edge rails, clear center.
 * When chat is collapsed (hud), the right panel shows pet-like turn status
 * (label + output preview) — not the desktop pet itself.
 */
const VrShell: React.FC<VrShellProps> = ({ sessionMgr, client, onDisconnect, onRescan }) => {
  const { t } = useI18n();
  const connectionHealth = useMobileStore((s) => s.connectionHealth);
  const activeTurn = useMobileStore((s) => s.activeTurn);
  const controlTarget = useMobileStore((s) => s.controlTarget);
  const appendNewMessages = useMobileStore((s) => s.appendNewMessages);
  const updateSessionName = useMobileStore((s) => s.updateSessionName);
  const setActiveTurn = useMobileStore((s) => s.setActiveTurn);

  const [mode, setMode] = useState<VrChromeMode>('hud');
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [activeSessionName, setActiveSessionName] = useState('Session');
  const [chatAutoFocus, setChatAutoFocus] = useState(false);
  const [showDisconnectConfirm, setShowDisconnectConfirm] = useState(false);
  const [overlay, setOverlay] = useState<'devices' | null>(null);
  /** Hide all HUD chrome so optical FoV stays unobstructed. */
  const [clearView, setClearView] = useState(false);
  // Raw latest output: stream first, then refresh when new messages persist.
  const [cachedTurnPreview, setCachedTurnPreview] = useState<string | undefined>();
  const messageCountRef = useRef(0);

  const sessionMessages = useMobileStore((s) => (
    activeSessionId
      ? (s.messagesBySession[activeSessionId] ?? EMPTY_MESSAGES)
      : EMPTY_MESSAGES
  ));

  const sessionsHostRef = useRef<HTMLDivElement | null>(null);
  const controlsHostRef = useRef<HTMLDivElement | null>(null);
  const hudPollerRef = useRef<SessionPoller | null>(null);
  const vrHosts = useMemo(
    () => ({ sessions: sessionsHostRef, controls: controlsHostRef }),
    [],
  );

  useEffect(() => {
    setCachedTurnPreview(undefined);
    messageCountRef.current = 0;
  }, [activeSessionId]);

  useEffect(() => {
    const raw = turnOutputRaw(activeTurn);
    if (raw) {
      setCachedTurnPreview((prev) => (prev === raw ? prev : raw));
    }
  }, [activeTurn]);

  useEffect(() => {
    const count = sessionMessages.length;
    if (count <= messageCountRef.current) {
      messageCountRef.current = count;
      return;
    }
    messageCountRef.current = count;
    const latest = lastAssistantPlainText(sessionMessages);
    if (latest) {
      setCachedTurnPreview((prev) => (prev === latest ? prev : latest));
    }
  }, [sessionMessages]);

  // Keep a lightweight poller while chat is collapsed so status stays live.
  useEffect(() => {
    hudPollerRef.current?.stop();
    hudPollerRef.current = null;

    if (mode !== 'hud' || !activeSessionId) return;

    const sessionId = activeSessionId;
    const knownCount = useMobileStore.getState().getMessages(sessionId).length;
    const turnEndRefreshTimers: number[] = [];
    const poller = new SessionPoller(sessionMgr, sessionId, (resp: PollResponse) => {
      if (resp.new_messages && resp.new_messages.length > 0) {
        appendNewMessages(sessionId, resp.new_messages);
      }
      if (resp.title) {
        setActiveSessionName(resp.title);
        updateSessionName(sessionId, resp.title);
      }
      const previousTurn = useMobileStore.getState().activeTurn;
      const resolvedTurn = resolvePolledActiveTurn(previousTurn, resp);
      setActiveTurn(resolvedTurn);

      if (shouldRefreshMessagesAfterTurn(previousTurn, resolvedTurn)) {
        const held = resolvedTurn?.status === 'completed' ? resolvedTurn : previousTurn;
        for (const timerId of turnEndRefreshTimers) {
          window.clearTimeout(timerId);
        }
        turnEndRefreshTimers.length = 0;
        const refresh = (forceClear: boolean) => {
          sessionMgr.getSessionMessages(sessionId, 200).then((fresh) => {
            useMobileStore.getState().setMessages(sessionId, fresh.messages);
            const current = useMobileStore.getState().activeTurn;
            if (!current || current.status === 'active') return;
            if (forceClear || shouldClearHeldTurnAfterRefresh(held, fresh.messages)) {
              resetActiveTurnHold();
              setActiveTurn(null);
            }
          }).catch(() => {});
        };
        for (let i = 0; i < TURN_END_REFRESH_DELAYS_MS.length; i += 1) {
          const delay = TURN_END_REFRESH_DELAYS_MS[i];
          const forceClear = i === TURN_END_REFRESH_DELAYS_MS.length - 1;
          if (delay === 0) refresh(forceClear);
          else turnEndRefreshTimers.push(window.setTimeout(() => refresh(forceClear), delay));
        }
      }
    });
    poller.start(knownCount);
    hudPollerRef.current = poller;

    return () => {
      for (const timerId of turnEndRefreshTimers) {
        window.clearTimeout(timerId);
      }
      turnEndRefreshTimers.length = 0;
      poller.stop();
      if (hudPollerRef.current === poller) hudPollerRef.current = null;
    };
  }, [
    activeSessionId,
    appendNewMessages,
    mode,
    sessionMgr,
    setActiveTurn,
    updateSessionName,
  ]);

  const hudActivity = useMemo(() => {
    if (!activeSessionId) return null;
    // Prefer cached stream/persist preview so idle does not fall back to round-1 text
    // while the latest assistant message is still in flight.
    return deriveHudActivity(
      activeTurn,
      cachedTurnPreview || lastAssistantPlainText(sessionMessages),
    );
  }, [activeSessionId, activeTurn, cachedTurnPreview, sessionMessages]);

  const handleCollapseToHud = useCallback(() => {
    setMode('hud');
    setChatAutoFocus(false);
  }, []);

  const handleEnterClearView = useCallback(() => {
    setMode('hud');
    setChatAutoFocus(false);
    setOverlay(null);
    setShowDisconnectConfirm(false);
    setClearView(true);
  }, []);

  const handleRestoreView = useCallback(() => {
    setClearView(false);
  }, []);

  // Left rail toggles focus: tap current session again to collapse; tap any
  // session (or create new) to open center chat. No right-rail expand/collapse.
  const handleSelectSession = useCallback(
    (sessionId: string, sessionName?: string, isNew?: boolean) => {
      if (clearView) return;
      if (!isNew && sessionId === activeSessionId && mode === 'focus') {
        handleCollapseToHud();
        return;
      }
      setActiveSessionId(sessionId);
      setActiveSessionName(sessionName || 'Session');
      setChatAutoFocus(!!isNew);
      setMode('focus');
    },
    [activeSessionId, clearView, handleCollapseToHud, mode],
  );

  return (
    <div className={`vr-shell${clearView ? ' vr-shell--clear' : ''}`}>
      <aside
        className="vr-shell__left"
        aria-label={t('sessions.sessionHistory')}
        hidden={clearView}
        aria-hidden={clearView}
      >
        <div
          ref={sessionsHostRef}
          className="vr-shell__host vr-shell__host--sessions"
        />
      </aside>

      <aside
        className="vr-shell__right"
        aria-label={t('sessions.connectionConnected')}
        hidden={clearView}
        aria-hidden={clearView}
      >
        <div className="vr-shell__host vr-shell__host--controls">
          <div className="vr-shell__status">
            <span
              className={`vr-shell__health-dot vr-shell__health-dot--${connectionHealth}`}
              aria-hidden="true"
            />
            <span className="vr-shell__health-text">
              {connectionLabel(connectionHealth, t)}
            </span>
          </div>

          <div ref={controlsHostRef} className="vr-shell__controls-slot" />

          {activeSessionId && hudActivity && (
            <div
              className={`vr-shell__session-card${hudActivity.busy ? ' is-busy' : ' is-idle'}`}
            >
              <span className="vr-shell__session-phase">{t(hudActivity.labelKey)}</span>
              {hudActivity.preview && (
                <p className="vr-shell__session-preview">{hudActivity.preview}</p>
              )}
            </div>
          )}

          <div className="vr-shell__right-actions">
            <button
              type="button"
              className="vr-shell__icon-btn"
              onClick={handleEnterClearView}
              aria-label={t('sessions.clearView')}
              title={t('sessions.clearView')}
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
                <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" />
                <circle cx="12" cy="12" r="3" />
                <line x1="4" y1="4" x2="20" y2="20" />
              </svg>
            </button>
            {client && (
              <button
                type="button"
                className={`vr-shell__icon-btn${controlTarget && !controlTarget.isHome ? ' is-remote' : ''}`}
                onClick={() => setOverlay('devices')}
                title={t('devices.title')}
                aria-label={t('devices.title')}
              >
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
                  <line x1="8" y1="21" x2="16" y2="21" />
                  <line x1="12" y1="17" x2="12" y2="21" />
                </svg>
              </button>
            )}
            <LanguageToggleButton className="vr-shell__icon-btn" />
            <button
              type="button"
              className="vr-shell__icon-btn"
              onClick={() => setShowDisconnectConfirm(true)}
              aria-label={t('sessions.disconnect')}
              title={t('sessions.disconnect')}
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
                <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
                <polyline points="16 17 21 12 16 7" />
                <line x1="21" y1="12" x2="9" y2="12" />
              </svg>
            </button>
          </div>
        </div>
      </aside>

      <SessionListPage
        sessionMgr={sessionMgr}
        selectedSessionId={activeSessionId}
        onSelectSession={handleSelectSession}
        vrHosts={vrHosts}
      />

      {clearView && (
        <button
          type="button"
          className="vr-shell__restore-btn"
          onClick={handleRestoreView}
          aria-label={t('sessions.restoreView')}
          title={t('sessions.restoreView')}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
            <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
        </button>
      )}

      {!clearView && mode === 'focus' && activeSessionId && (
        <main
          className="vr-shell__focus"
          aria-label={t('sessions.continueSession')}
        >
          <Suspense fallback={<div className="spinner" aria-hidden="true" />}>
            <ChatPage
              sessionMgr={sessionMgr}
              sessionId={activeSessionId}
              sessionName={activeSessionName}
              onBack={handleCollapseToHud}
              autoFocus={chatAutoFocus}
              layout="vr"
            />
          </Suspense>
        </main>
      )}

      {!clearView && overlay === 'devices' && client && (
        <div className="vr-shell__overlay" role="dialog" aria-modal="true">
          <div className="vr-shell__overlay-panel">
            <DevicesPage client={client} onBack={() => setOverlay(null)} />
          </div>
          <button
            type="button"
            className="vr-shell__overlay-backdrop"
            aria-label={t('common.back')}
            onClick={() => setOverlay(null)}
          />
        </div>
      )}

      {!clearView && showDisconnectConfirm && (
        <div
          className="vr-shell__overlay"
          role="alertdialog"
          aria-modal="true"
          aria-labelledby="vr-disconnect-title"
        >
          <div className="vr-shell__overlay-panel vr-shell__confirm-panel">
            <h3 id="vr-disconnect-title">{t('sessions.disconnect')}</h3>
            <p>{t('sessions.disconnectConfirm')}</p>
            <div className="vr-shell__confirm-actions">
              <button
                type="button"
                className="vr-shell__confirm-btn vr-shell__confirm-btn--muted"
                onClick={() => setShowDisconnectConfirm(false)}
              >
                {t('common.cancel')}
              </button>
              {onRescan && (
                <button
                  type="button"
                  className="vr-shell__confirm-btn vr-shell__confirm-btn--muted"
                  onClick={() => {
                    setShowDisconnectConfirm(false);
                    onRescan();
                  }}
                >
                  {t('sessions.rescan')}
                </button>
              )}
              <button
                type="button"
                className="vr-shell__confirm-btn"
                onClick={() => {
                  setShowDisconnectConfirm(false);
                  onDisconnect();
                }}
              >
                {t('sessions.disconnect')}
              </button>
            </div>
          </div>
          <button
            type="button"
            className="vr-shell__overlay-backdrop"
            aria-label={t('common.cancel')}
            onClick={() => setShowDisconnectConfirm(false)}
          />
        </div>
      )}
    </div>
  );
};

export default VrShell;
