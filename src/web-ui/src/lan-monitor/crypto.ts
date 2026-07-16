import { gcm } from '@noble/ciphers/aes.js';
import { x25519 } from '@noble/curves/ed25519.js';

const NONCE_LENGTH = 12;

export interface LanMonitorKeyPair {
  publicKey: Uint8Array;
  privateKey: Uint8Array;
}

export function toBase64(value: Uint8Array): string {
  let binary = '';
  for (const byte of value) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function fromBase64(value: string): Uint8Array {
  const binary = atob(value);
  return Uint8Array.from(binary, character => character.charCodeAt(0));
}

function randomBytes(length: number): Uint8Array {
  const source = globalThis.crypto ?? (globalThis as typeof globalThis & { msCrypto?: Crypto }).msCrypto;
  if (!source?.getRandomValues) {
    throw new Error('This browser does not provide secure random values');
  }
  return source.getRandomValues(new Uint8Array(length));
}

export function generateKeyPair(): LanMonitorKeyPair {
  const privateKey = randomBytes(32);
  return {
    privateKey,
    publicKey: new Uint8Array(x25519.getPublicKey(privateKey)),
  };
}

export function deriveSharedKey(
  keyPair: LanMonitorKeyPair,
  peerPublicKey: Uint8Array,
): Uint8Array {
  return new Uint8Array(x25519.getSharedSecret(keyPair.privateKey, peerPublicKey));
}

export function encrypt(
  key: Uint8Array,
  plaintext: string,
): { data: string; nonce: string } {
  const nonce = randomBytes(NONCE_LENGTH);
  const ciphertext = gcm(key, nonce).encrypt(new TextEncoder().encode(plaintext));
  return { data: toBase64(ciphertext), nonce: toBase64(nonce) };
}

export function decrypt(key: Uint8Array, data: string, nonce: string): string {
  const plaintext = gcm(key, fromBase64(nonce)).decrypt(fromBase64(data));
  return new TextDecoder().decode(plaintext);
}
