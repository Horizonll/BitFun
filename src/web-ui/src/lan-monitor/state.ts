import type { ActiveTurn, PollSnapshot, SessionInfo, TranscriptPage } from './types';

const FINISHED_TURN_STATUSES = new Set(['completed', 'failed', 'cancelled']);

export function isActiveTranscriptTurn(turnId: string, activeTurn: ActiveTurn | null): boolean {
  return activeTurn?.turnId === turnId;
}

export function shouldRetainActiveTurn(snapshot: PollSnapshot): boolean {
  const status = snapshot.activeTurn?.status.toLowerCase();
  return Boolean(status && !FINISHED_TURN_STATUSES.has(status));
}

export function mergeOlderTranscriptPage(
  current: TranscriptPage,
  older: TranscriptPage,
): TranscriptPage {
  const seenTurnIds = new Set<string>();
  const turns = [...older.turns, ...current.turns].filter(turn => {
    if (seenTurnIds.has(turn.turnId)) return false;
    seenTurnIds.add(turn.turnId);
    return true;
  });
  return {
    ...current,
    turns,
    totalTurnCount: Math.max(current.totalTurnCount, older.totalTurnCount),
    hasMore: older.hasMore,
    nextBeforeTurnId: older.nextBeforeTurnId,
  };
}

export function mergeSessionPoll(
  sessions: SessionInfo[],
  sessionId: string,
  snapshot: PollSnapshot,
): SessionInfo[] {
  if (!snapshot.changed) return sessions;
  return sessions.map(session =>
    session.session_id === sessionId
      ? {
          ...session,
          name: snapshot.title || session.name,
          state: snapshot.sessionState ?? session.state,
          active_turn_id: snapshot.activeTurn?.turnId ?? null,
        }
      : session,
  );
}
