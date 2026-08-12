import React, {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import type { RelayHttpClient } from '../services/RelayHttpClient';
import { RemoteSessionManager } from '../services/RemoteSessionManager';
import { requestExitApp } from '../services/glassesHost';
import { installTempleFocus } from '../services/templeFocus';
import SessionListPage from './SessionListPage';

const ChatPage = lazy(() => import('./ChatPage'));

type GlassesAppPage = 'sessions' | 'chat';

const TEMPLE_SCROLL_STEP_PX = 140;

function resolvePageScrollContainer(page: GlassesAppPage): HTMLElement | null {
  if (page === 'chat') {
    return document.querySelector<HTMLElement>('.chat-page__messages');
  }
  if (page === 'sessions') {
    return document.querySelector<HTMLElement>('.session-list__page-scroll');
  }
  return null;
}

export interface VrShellProps {
  sessionMgr: RemoteSessionManager;
  /** Kept for App wiring compatibility; unused after devices entry removed. */
  client?: RelayHttpClient | null;
}

/**
 * Glasses page-app shell (left SPA only).
 * Right eye is a native PixelCopy clone in MobileWebActivity — no secondary SPA
 * and no UI-mirror publish loop on this path.
 */
const VrShell: React.FC<VrShellProps> = ({ sessionMgr }) => {
  const [page, setPage] = useState<GlassesAppPage>('sessions');
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [activeSessionName, setActiveSessionName] = useState('Session');
  const [chatAutoFocus, setChatAutoFocus] = useState(false);
  const pageRef = useRef<GlassesAppPage>(page);
  pageRef.current = page;
  const backToSessionsRef = useRef<() => void>(() => undefined);

  useEffect(() => {
    return installTempleFocus({
      onVerticalSlide: (direction) => {
        const el = resolvePageScrollContainer(pageRef.current);
        if (!el) return false;
        const maxScroll = Math.max(0, el.scrollHeight - el.clientHeight);
        if (maxScroll <= 0) return false;
        el.scrollTop = Math.max(
          0,
          Math.min(maxScroll, el.scrollTop + direction * TEMPLE_SCROLL_STEP_PX),
        );
        return true;
      },
      onClick: () => {
        if (pageRef.current !== 'chat') return false;
        window.__bitfunVoiceToggle?.();
        return true;
      },
      onDoubleClick: () => {
        if (pageRef.current === 'chat') {
          backToSessionsRef.current();
          return;
        }
        if (pageRef.current === 'sessions') {
          requestExitApp();
        }
      },
    });
  }, []);

  const handleSelectSession = useCallback(
    (sessionId: string, sessionName?: string, isNew?: boolean) => {
      setActiveSessionId(sessionId);
      setActiveSessionName(sessionName || 'Session');
      setChatAutoFocus(!!isNew);
      setPage('chat');
    },
    [],
  );

  const handleBackToSessions = useCallback(() => {
    setPage('sessions');
    setChatAutoFocus(false);
  }, []);
  backToSessionsRef.current = handleBackToSessions;

  return (
    <div className="glasses-shell">
      {page === 'sessions' && (
        <div className="glasses-shell__page glasses-shell__page--sessions-minimal">
          <div className="glasses-shell__body">
            <SessionListPage
              sessionMgr={sessionMgr}
              selectedSessionId={activeSessionId}
              onSelectSession={handleSelectSession}
            />
          </div>
        </div>
      )}

      {page === 'chat' && activeSessionId && (
        <div className="glasses-shell__page glasses-shell__page--chat">
          <Suspense fallback={null}>
            <ChatPage
              sessionMgr={sessionMgr}
              sessionId={activeSessionId}
              sessionName={activeSessionName}
              onBack={handleBackToSessions}
              autoFocus={chatAutoFocus}
              layout="stack"
              enableVoice
              glassesUi
            />
          </Suspense>
        </div>
      )}
    </div>
  );
};

export default VrShell;
