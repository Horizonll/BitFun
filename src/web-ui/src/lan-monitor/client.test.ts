import { afterEach, describe, expect, it } from 'vitest';
import { createSecureRandomId, LanMonitorClient } from './client';

const pairingStorageKey = 'bitfun.lan-monitor.pairing';
const originalLocalStorage = globalThis.localStorage;

describe('LAN monitor browser compatibility', () => {
  afterEach(() => {
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      value: originalLocalStorage,
    });
  });

  it('uses randomUUID when the browser provides it', () => {
    const value = createSecureRandomId({
      randomUUID: () => 'native-random-id',
      getRandomValues: array => array,
    });
    expect(value).toBe('native-random-id');
  });

  it('creates an RFC 4122 UUID with getRandomValues when randomUUID is unavailable', () => {
    const value = createSecureRandomId({
      getRandomValues: array => {
        const bytes = array as Uint8Array;
        bytes.forEach((_, index) => {
          bytes[index] = index;
        });
        return array;
      },
    });

    expect(value).toBe('00010203-0405-4607-8809-0a0b0c0d0e0f');
  });

  it('restores a paired client from local storage and clears it on disconnect', () => {
    const values = new Map<string, string>();
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      value: {
        getItem: (key: string) => values.get(key) ?? null,
        setItem: (key: string, value: string) => values.set(key, value),
        removeItem: (key: string) => values.delete(key),
      },
    });
    values.set(
      pairingStorageKey,
      JSON.stringify({
        version: 1,
        roomId: 'room-id',
        relayUrl: 'http://192.168.1.2:9700',
        desktopPublicKey: btoa('a'.repeat(32)),
        sharedKey: btoa('b'.repeat(32)),
      }),
    );

    const client = LanMonitorClient.restore();
    expect(client?.isPaired).toBe(true);
    expect(LanMonitorClient.hasPersistedPairing()).toBe(true);
    client?.forgetPairing();
    expect(LanMonitorClient.hasPersistedPairing()).toBe(false);
  });
});
