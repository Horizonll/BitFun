import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useI18n } from '../i18n';
import { RelayHttpClient } from '../services/RelayHttpClient';
import { RemoteSessionManager } from '../services/RemoteSessionManager';
import {
  getSharedInstallId,
  requestRescanQr,
} from '../services/glassesHost';
import { useMobileStore } from '../services/store';
import logoIcon from '../assets/Logo-ICON.png';

interface PairingPageProps {
  onPaired: (client: RelayHttpClient, sessionMgr: RemoteSessionManager) => void;
}

/** Fixed pair identity for glasses MVP (no username form). */
export const GLASSES_USER_ID = 'BitFun Glasses';

const MOBILE_INSTALL_ID_KEY = 'bitfun.mobile.install_id';
const MOBILE_USER_ID_KEY = 'bitfun.mobile.user_id';

function normalizeRelayUrl(value: string): string | null {
  try {
    const normalized = value
      .replace(/^wss:\/\//, 'https://')
      .replace(/^ws:\/\//, 'http://')
      .replace(/\/ws\/?$/, '')
      .replace(/\/$/, '');
    const url = new URL(normalized);
    if (!['http:', 'https:'].includes(url.protocol)
      || !url.hostname
      || url.username
      || url.password
      || url.search
      || url.hash) {
      return null;
    }
    return url.toString().replace(/\/$/, '');
  } catch {
    return null;
  }
}

function validPairingSecret(room: string | null, publicKey: string | null): boolean {
  return !!room
    && room.length <= 128
    && /^[A-Za-z0-9_-]+$/.test(room)
    && !['_store', 'page-data', 'pages'].includes(room)
    && !!publicKey
    && publicKey.length <= 512
    && /^[A-Za-z0-9+/=_-]+$/.test(publicKey);
}

function generateInstallId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `glasses-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function getOrCreateInstallId(): string {
  const shared = getSharedInstallId();
  if (shared) {
    localStorage.setItem(MOBILE_INSTALL_ID_KEY, shared);
    return shared;
  }
  const existing = localStorage.getItem(MOBILE_INSTALL_ID_KEY)?.trim();
  if (existing) return existing;
  const created = generateInstallId();
  localStorage.setItem(MOBILE_INSTALL_ID_KEY, created);
  return created;
}

function resolvePairingTarget(): {
  room: string | null;
  pk: string | null;
  httpBaseUrl: string;
  accountAuth: boolean;
} {
  const hash = window.location.hash;
  const params = new URLSearchParams(hash.replace(/^#\/pair\?/, ''));
  const room = params.get('room');
  const pk = params.get('pk');
  const relayParam = params.get('relay');
  const accountAuth = params.get('auth') === 'account';

  if (relayParam) {
    return {
      room,
      pk,
      httpBaseUrl: normalizeRelayUrl(relayParam) ?? '',
      accountAuth,
    };
  }

  const origin = window.location.origin;
  const pathname = window.location.pathname
    .replace(/\/[^/]*$/, '')
    .replace(/\/r\/[^/]*$/, '');
  return {
    room,
    pk,
    httpBaseUrl: origin + pathname,
    accountAuth,
  };
}

function applyInitialSyncToStore(initialSync: any): void {
  const store = useMobileStore.getState();
  // Glasses companion is Expert/pro only — ignore assistant pair payloads.
  store.setPairedDisplayMode('pro');
  store.setCurrentAssistant(null);
  if (
    initialSync?.has_workspace
    && initialSync.workspace_kind !== 'assistant'
    && initialSync.path
  ) {
    store.setCurrentWorkspace({
      has_workspace: true,
      path: initialSync.path,
      project_name: initialSync.project_name,
      git_branch: initialSync.git_branch,
      workspace_kind: initialSync.workspace_kind,
      assistant_id: initialSync.assistant_id,
      remote_connection_id: initialSync.remote_connection_id,
      remote_ssh_host: initialSync.remote_ssh_host,
    });
  } else {
    store.setCurrentWorkspace(null);
  }
  if (initialSync?.sessions) {
    store.setSessions(initialSync.sessions);
  }
}

async function maybeRequestDelegatedIdentity(
  client: RelayHttpClient,
  isCurrentAttempt: () => boolean,
): Promise<void> {
  try {
    const delegated = await Promise.race<boolean>([
      client.requestDelegatedIdentity(),
      new Promise<boolean>((resolve) => {
        window.setTimeout(() => resolve(false), 10_000);
      }),
    ]);
    if (!isCurrentAttempt()) return;
    const homeDeviceId = client.homeDeviceId;
    if (delegated && homeDeviceId) {
      useMobileStore.getState().setControlTarget({
        deviceId: homeDeviceId,
        deviceName: null,
        isHome: true,
      });
      const accountEpoch = client.delegatedAccountEpoch;
      const target = client.getControlTargetSnapshot();
      void client
        .listDevices()
        .then((devices) => {
          if (
            client.delegatedAccountEpoch !== accountEpoch
            || !client.isControlTargetCurrent(target)
            || client.pairedDeviceId !== homeDeviceId
          ) return;
          const home = devices.find((d) => d.device_id === homeDeviceId);
          if (home) {
            useMobileStore.getState().setControlTarget({
              deviceId: homeDeviceId,
              deviceName: home.device_name,
              isHome: true,
            });
          }
        })
        .catch(() => {
          // Device name resolution is cosmetic; ignore failures.
        });
    }
  } catch {
    // Single-device pairing without delegation is normal.
  }
}

const PairingPage: React.FC<PairingPageProps> = ({ onPaired }) => {
  const { t } = useI18n();
  const {
    connectionStatus,
    setConnectionStatus,
    setError,
    error,
    setAuthenticatedUserId,
  } = useMobileStore();
  const [retryToken, setRetryToken] = useState(0);
  const pairAttemptGenerationRef = useRef(0);
  const onPairedRef = useRef(onPaired);
  onPairedRef.current = onPaired;

  const pairingTarget = useMemo(() => resolvePairingTarget(), []);

  const finishPaired = useCallback(
    async (
      client: RelayHttpClient,
      initialSync: any,
      isCurrentAttempt: () => boolean,
    ) => {
      setConnectionStatus('paired');
      localStorage.setItem(MOBILE_USER_ID_KEY, GLASSES_USER_ID);
      setAuthenticatedUserId(initialSync?.authenticated_user_id ?? GLASSES_USER_ID);
      applyInitialSyncToStore(initialSync);
      await maybeRequestDelegatedIdentity(client, isCurrentAttempt);
      if (!isCurrentAttempt()) return;
      onPairedRef.current(client, new RemoteSessionManager(client));
    },
    [setAuthenticatedUserId, setConnectionStatus],
  );

  const attemptPair = useCallback(async () => {
    const attemptGeneration = ++pairAttemptGenerationRef.current;
    const isCurrentAttempt = () => pairAttemptGenerationRef.current === attemptGeneration;

    const roomId = pairingTarget.room;
    const desktopPublicKey = pairingTarget.pk;
    const currentInstallId = getOrCreateInstallId();

    if (pairingTarget.accountAuth) {
      if (!isCurrentAttempt()) return;
      setError(t('pairing.accountAuthUnsupported'));
      setConnectionStatus('error');
      return;
    }

    if (!roomId
      || !desktopPublicKey
      || !validPairingSecret(roomId, desktopPublicKey)
      || !pairingTarget.httpBaseUrl) {
      if (!isCurrentAttempt()) return;
      setError(t('pairing.invalidQrCode'));
      setConnectionStatus('error');
      return;
    }

    setError(null);
    setConnectionStatus('pairing');

    const client = new RelayHttpClient(pairingTarget.httpBaseUrl, roomId);

    try {
      const initialSync = await client.pair(desktopPublicKey, {
        userId: GLASSES_USER_ID,
        mobileInstallId: currentInstallId,
      });
      if (!isCurrentAttempt()) return;
      await finishPaired(client, initialSync, isCurrentAttempt);
    } catch (e: unknown) {
      if (!isCurrentAttempt()) return;
      const rawErrorMessage = e instanceof Error ? e.message : '';
      const errorMessage = rawErrorMessage.includes('timed out')
        ? t('pairing.requestTimedOut')
        : rawErrorMessage.includes('HTTP 404')
          ? t('pairing.qrExpired')
          : rawErrorMessage.includes('HTTP 429')
            ? t('pairing.rateLimited')
            : rawErrorMessage.includes('HTTP 503') || rawErrorMessage.includes('HTTP 504')
              ? t('pairing.relayUnavailable')
              : rawErrorMessage || t('pairing.pairingFailed');
      setError(errorMessage);
      setConnectionStatus('error');
    }
  }, [
    finishPaired,
    pairingTarget.accountAuth,
    pairingTarget.httpBaseUrl,
    pairingTarget.pk,
    pairingTarget.room,
    setConnectionStatus,
    setError,
    t,
  ]);

  useEffect(() => {
    void attemptPair();
    return () => {
      pairAttemptGenerationRef.current += 1;
    };
  }, [attemptPair, retryToken]);

  const stateLabels: Record<string, string> = {
    idle: t('pairing.connectingAndPairing'),
    pairing: t('pairing.connectingAndPairing'),
    paired: t('pairing.pairedLoadingSessions'),
    error: t('pairing.connectionError'),
  };
  return (
    <div className="pairing-page pairing-page--vr">
      <div className="pairing-page__card">
        <img src={logoIcon} alt="BitFun" className="pairing-page__logo" />
        <div className="pairing-page__brand">{t('pairing.title')}</div>
        <div className="pairing-page__subtitle">{t('pairing.subtitle')}</div>

        <div className="pairing-page__state">
          {stateLabels[connectionStatus] || connectionStatus}
        </div>

        {connectionStatus === 'error' && (
          <div className="pairing-page__form">
            <p className="pairing-page__note">{t('pairing.glassesAutoPairNote')}</p>
            <button
              className="pairing-page__retry"
              type="button"
              data-rayneo-focus
              onClick={() => {
                setError(null);
                setConnectionStatus('pairing');
                setRetryToken((n) => n + 1);
              }}
            >
              {t('pairing.retry')}
            </button>
            <button
              className="pairing-page__retry pairing-page__retry--secondary"
              type="button"
              data-rayneo-focus
              onClick={() => {
                if (!requestRescanQr()) {
                  setError(t('pairing.qrExpired'));
                }
              }}
            >
              {t('pairing.rescan')}
            </button>
          </div>
        )}

        {error && <div className="pairing-page__error">{error}</div>}
      </div>
    </div>
  );
};

export default PairingPage;
