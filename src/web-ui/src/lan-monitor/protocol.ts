import type { LanMonitorResponse } from './types';

export interface LanMonitorCommandEnvelope {
  cmd: 'lan_monitor';
  request: Record<string, unknown>;
  _protocol_version: 1;
  _issued_at_ms: number;
  _request_id: string;
}

export function createLanMonitorCommandEnvelope(
  request: Record<string, unknown>,
  requestId: string,
  issuedAtMs: number,
): LanMonitorCommandEnvelope {
  return {
    cmd: 'lan_monitor',
    request,
    _protocol_version: 1,
    _issued_at_ms: issuedAtMs,
    _request_id: requestId,
  };
}

export function hasMatchingLanMonitorRequestId(
  response: LanMonitorResponse & { _request_id?: string },
  requestId: string,
): boolean {
  return response._request_id === requestId;
}
