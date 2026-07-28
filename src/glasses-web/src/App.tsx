import React, { useCallback, useEffect, useRef, useState } from 'react';
import PairingPage from './pages/PairingPage';
import VrShell from './pages/VrShell';
import { ErrorBoundary } from './components/ErrorBoundary';
import { I18nProvider, useI18n } from './i18n';
import { RelayHttpClient } from './services/RelayHttpClient';
import { RemoteSessionManager } from './services/RemoteSessionManager';
import { reconcileDelegatedAccountOwner } from './services/delegatedAccountOwner';
import { ThemeProvider } from './theme';
import { useConnectionHealth } from './hooks/useConnectionHealth';
import { requestRescanQr } from './services/glassesHost';
import { useMobileStore } from './services/store';
import './styles/index.scss';

const AppContent: React.FC = () => {
  const { t } = useI18n();
  const [paired, setPaired] = useState(false);
  const connectionHealth = useMobileStore((state) => state.connectionHealth);
  const clientRef = useRef<RelayHttpClient | null>(null);
  const delegatedOwnerUnlistenRef = useRef<(() => void) | null>(null);
  const sessionMgrRef = useRef<RemoteSessionManager | null>(null);
  const [sessionMgr, setSessionMgr] = useState<RemoteSessionManager | null>(null);

  useEffect(() => {
    document.documentElement.classList.add('vr-ui');
    document.body.classList.add('vr-ui');
    return () => {
      document.documentElement.classList.remove('vr-ui');
      document.body.classList.remove('vr-ui');
    };
  }, []);

  useConnectionHealth(sessionMgr);

  useEffect(() => {
    const handleLinkClick = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      const link = target.closest('a') as HTMLAnchorElement | null;
      if (link?.href) {
        const href = link.href;
        if (href.startsWith('http://') || href.startsWith('https://')) {
          e.preventDefault();
          e.stopPropagation();
          window.open(href, '_blank', 'noopener,noreferrer');
        }
      }
    };
    document.addEventListener('click', handleLinkClick, true);
    return () => document.removeEventListener('click', handleLinkClick, true);
  }, []);

  const handlePaired = useCallback(
    (client: RelayHttpClient, nextSessionMgr: RemoteSessionManager) => {
      delegatedOwnerUnlistenRef.current?.();
      clientRef.current = client;
      delegatedOwnerUnlistenRef.current = client.onDelegatedAccountOwnerChange((change) => {
        if (clientRef.current !== client) return;
        reconcileDelegatedAccountOwner(change);
      }, { emitCurrent: true });
      sessionMgrRef.current = nextSessionMgr;
      setSessionMgr(nextSessionMgr);
      setPaired(true);
    },
    [],
  );

  const handleDisconnect = useCallback(() => {
    delegatedOwnerUnlistenRef.current?.();
    delegatedOwnerUnlistenRef.current = null;
    clientRef.current?.resetConnectionIdentity();
    clientRef.current = null;
    sessionMgrRef.current = null;
    setSessionMgr(null);
    localStorage.removeItem('bitfun.mobile.user_id');
    useMobileStore.getState().resetConnectionState();
    setPaired(false);
  }, []);

  const handleRescan = useCallback(() => {
    // Native host opens the camera scanner and tears down this WebView.
    // Without a host bridge, fall back to the in-page pairing flow.
    if (!requestRescanQr()) {
      handleDisconnect();
    }
  }, [handleDisconnect]);

  useEffect(() => () => {
    delegatedOwnerUnlistenRef.current?.();
    delegatedOwnerUnlistenRef.current = null;
  }, []);

  return (
    <div className="mobile-app mobile-app--vr">
      {connectionHealth === 'unreachable' && paired && (
        <div className="mobile-reconnect-banner" role="alert">
          <span className="mobile-reconnect-spinner" />
          <span>{t('sessions.reconnecting')}</span>
          <button type="button" onClick={handleDisconnect}>
            {t('sessions.repair')}
          </button>
          <button type="button" onClick={handleRescan}>
            {t('sessions.rescan')}
          </button>
        </div>
      )}
      {!paired && <PairingPage onPaired={handlePaired} />}
      {paired && sessionMgrRef.current && (
        <VrShell
          sessionMgr={sessionMgrRef.current}
          client={clientRef.current}
          onDisconnect={handleDisconnect}
          onRescan={handleRescan}
        />
      )}
    </div>
  );
};

const App: React.FC = () => (
  <ThemeProvider>
    <ErrorBoundary>
      <I18nProvider>
        <AppContent />
      </I18nProvider>
    </ErrorBoundary>
  </ThemeProvider>
);

export default App;
