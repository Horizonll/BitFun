import type { LanMonitorResponse } from './types';
import { translate as t } from './i18n';
import {
  createLanMonitorCommandEnvelope,
  hasMatchingLanMonitorRequestId,
} from './protocol';
import {
  decrypt,
  deriveSharedKey,
  encrypt,
  fromBase64,
  generateKeyPair,
  toBase64,
  type LanMonitorKeyPair,
} from './crypto';

interface EncryptedPayload {
  encrypted_data: string;
  nonce: string;
}

interface PairingTarget {
  roomId: string;
  desktopPublicKey: Uint8Array;
  relayUrl: string;
}

interface LanMonitorBootstrapResponse {
  room_id: string;
  public_key: string;
}

interface RandomCryptoSource {
  randomUUID?: () => string;
  getRandomValues<T extends ArrayBufferView>(array: T): T;
}

export function createSecureRandomId(cryptoSource: RandomCryptoSource = crypto): string {
  if (typeof cryptoSource.randomUUID === 'function') return cryptoSource.randomUUID();
  const bytes = cryptoSource.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hexadecimal = Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('');
  return `${hexadecimal.slice(0, 8)}-${hexadecimal.slice(8, 12)}-${hexadecimal.slice(12, 16)}-${hexadecimal.slice(16, 20)}-${hexadecimal.slice(20)}`;
}

function resolveRelayUrl(): string {
  const params = new URLSearchParams(window.location.hash.replace(/^#\/pair\?/, ''));
  const relay = params.get('relay');
  return relay
    ? relay
        .replace(/^wss:\/\//, 'https://')
        .replace(/^ws:\/\//, 'http://')
        .replace(/\/ws\/?$/, '')
        .replace(/\/$/, '')
    : window.location.origin;
}

async function resolveTarget(): Promise<PairingTarget> {
  const params = new URLSearchParams(window.location.hash.replace(/^#\/pair\?/, ''));
  let roomId = params.get('room');
  const publicKey = params.get('pk');
  const relayUrl = resolveRelayUrl();
  let desktopPublicKey = publicKey;
  if (!roomId || !desktopPublicKey) {
    const bootstrapResponse = await fetch(`${relayUrl}/api/lan-monitor/bootstrap`);
    if (!bootstrapResponse.ok) {
      throw new Error(t('invalidUrl'));
    }
    const bootstrap = (await bootstrapResponse.json()) as LanMonitorBootstrapResponse;
    roomId = bootstrap.room_id;
    desktopPublicKey = bootstrap.public_key;
  }
  if (!roomId || !desktopPublicKey) throw new Error(t('invalidUrl'));
  return {
    roomId,
    desktopPublicKey: fromBase64(desktopPublicKey),
    relayUrl,
  };
}

export class LanMonitorClient {
  private target: PairingTarget | null = null;
  private sharedKey: Uint8Array | null = null;

  async pair(pairingCode: string, installationId: string): Promise<void> {
    this.target = await resolveTarget();
    const keyPair: LanMonitorKeyPair = generateKeyPair();
    const publicKey = toBase64(keyPair.publicKey);
    this.sharedKey = deriveSharedKey(keyPair, this.target.desktopPublicKey);
    const pairResponse = await fetch(
      `${this.target.relayUrl}/api/rooms/${this.target.roomId}/pair`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          public_key: publicKey,
          device_id: installationId,
          device_name: t('deviceName'),
        }),
      },
    );
    if (!pairResponse.ok) throw new Error(t('pairingFailed', { status: pairResponse.status }));
    const challengePayload = (await pairResponse.json()) as EncryptedPayload;
    const challenge = JSON.parse(
      decrypt(this.sharedKey, challengePayload.encrypted_data, challengePayload.nonce),
    );
    const verification = encrypt(
      this.sharedKey,
      JSON.stringify({
        challenge_echo: challenge.challenge,
        device_id: installationId,
        device_name: t('deviceName'),
        mobile_install_id: installationId,
        user_id: pairingCode,
      }),
    );
    const verificationResponse = await fetch(
      `${this.target.relayUrl}/api/rooms/${this.target.roomId}/command`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          encrypted_data: verification.data,
          nonce: verification.nonce,
        }),
      },
    );
    if (!verificationResponse.ok) {
      throw new Error(t('verificationFailed', { status: verificationResponse.status }));
    }
    const responsePayload = (await verificationResponse.json()) as EncryptedPayload;
    const response = JSON.parse(
      decrypt(this.sharedKey, responsePayload.encrypted_data, responsePayload.nonce),
    );
    if (response.resp === 'error') throw new Error(response.message || t('pairingRejected'));
  }

  async command(request: Record<string, unknown>): Promise<LanMonitorResponse> {
    if (!this.sharedKey || !this.target) throw new Error(t('notPaired'));
    const requestId = createSecureRandomId();
    const payload = encrypt(
      this.sharedKey,
      JSON.stringify(createLanMonitorCommandEnvelope(request, requestId, Date.now())),
    );
    const response = await fetch(
      `${this.target.relayUrl}/api/rooms/${this.target.roomId}/command`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ encrypted_data: payload.data, nonce: payload.nonce }),
      },
    );
    if (!response.ok) throw new Error(t('commandFailed', { status: response.status }));
    const responsePayload = (await response.json()) as EncryptedPayload;
    const parsed = JSON.parse(
      decrypt(this.sharedKey, responsePayload.encrypted_data, responsePayload.nonce),
    ) as LanMonitorResponse & { _request_id?: string };
    if (parsed.resp === 'error') throw new Error(parsed.message);
    if (!hasMatchingLanMonitorRequestId(parsed, requestId)) throw new Error(t('invalidResponse'));
    return parsed;
  }
}

export function getOrCreateInstallationId(): string {
  const key = 'bitfun.lan-monitor.install-id';
  const existing = localStorage.getItem(key)?.trim();
  if (existing) return existing;
  const created = createSecureRandomId();
  localStorage.setItem(key, created);
  return created;
}
