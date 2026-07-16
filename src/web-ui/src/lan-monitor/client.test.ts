import { describe, expect, it } from 'vitest';
import { createSecureRandomId } from './client';

describe('LAN monitor browser compatibility', () => {
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
});
