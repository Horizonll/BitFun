import { describe, expect, it } from 'vitest';
import {
  createLanMonitorCommandEnvelope,
  hasMatchingLanMonitorRequestId,
} from './protocol';

describe('LAN monitor encrypted command protocol', () => {
  it('adds the replay-protection fields without changing the monitor request', () => {
    const request = { action: 'poll_session', session_id: 'session-1', since_version: 4 };
    const envelope = createLanMonitorCommandEnvelope(request, 'request-0001', 123_456);

    expect(envelope).toEqual({
      cmd: 'lan_monitor',
      request,
      _protocol_version: 1,
      _issued_at_ms: 123_456,
      _request_id: 'request-0001',
    });
  });

  it('accepts only the response correlated to the encrypted request', () => {
    expect(
      hasMatchingLanMonitorRequestId(
        { resp: 'pong', _request_id: 'request-0001' },
        'request-0001',
      ),
    ).toBe(true);
    expect(
      hasMatchingLanMonitorRequestId(
        { resp: 'pong', _request_id: 'request-0002' },
        'request-0001',
      ),
    ).toBe(false);
  });
});
