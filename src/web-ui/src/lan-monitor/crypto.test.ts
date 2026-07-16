import { describe, expect, it } from 'vitest';
import { decrypt, deriveSharedKey, encrypt, generateKeyPair } from './crypto';

describe('LAN monitor browser crypto', () => {
  it('derives the same key with X25519 and encrypts with AES-GCM', () => {
    const alice = generateKeyPair();
    const bob = generateKeyPair();
    const aliceKey = deriveSharedKey(alice, bob.publicKey);
    const bobKey = deriveSharedKey(bob, alice.publicKey);
    const payload = encrypt(aliceKey, 'LAN monitor test');

    expect(Array.from(aliceKey)).toEqual(Array.from(bobKey));
    expect(decrypt(bobKey, payload.data, payload.nonce)).toBe('LAN monitor test');
  });
});
