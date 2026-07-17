import { describe, expect, it } from 'vitest';
import {
  isActiveTranscriptTurn,
  mergeOlderTranscriptPage,
  mergeSessionPoll,
  shouldRetainActiveTurn,
} from './state';
import type { MonitorTurn, PollSnapshot, SessionInfo, TranscriptPage } from './types';

function turn(turnId: string): MonitorTurn {
  return {
    turnId,
    turnIndex: Number(turnId.slice(-1)),
    kind: 'user_dialog',
    status: 'completed',
    timestamp: 1,
    startTime: 1,
    userMessage: { id: `user-${turnId}`, content: turnId, timestamp: 1 },
    rounds: [],
  };
}

describe('LAN monitor read-only state projection', () => {
  it('prepends older turns without duplicating overlap cursors', () => {
    const current: TranscriptPage = {
      sessionId: 'session-1',
      turns: [turn('turn-2'), turn('turn-3')],
      totalTurnCount: 3,
      hasMore: true,
      nextBeforeTurnId: 'turn-2',
    };
    const older: TranscriptPage = {
      sessionId: 'session-1',
      turns: [turn('turn-1'), turn('turn-2')],
      totalTurnCount: 3,
      hasMore: false,
    };

    expect(mergeOlderTranscriptPage(current, older).turns.map(item => item.turnId)).toEqual([
      'turn-1',
      'turn-2',
      'turn-3',
    ]);
  });

  it('merges live state only into the selected session snapshot', () => {
    const sessions: SessionInfo[] = [
      {
        session_id: 'session-1',
        name: 'Old title',
        agent_type: 'agentic',
        created_at: '1',
        updated_at: '1',
        message_count: 2,
      },
      {
        session_id: 'session-2',
        name: 'Other',
        agent_type: 'agentic',
        created_at: '1',
        updated_at: '1',
        message_count: 1,
      },
    ];
    const snapshot: PollSnapshot = {
      version: 7,
      changed: true,
      sessionState: 'processing',
      title: 'Live title',
      activeTurn: {
        turnId: 'turn-live',
        status: 'active',
        roundIndex: 1,
        text: '',
        thinking: '',
        tools: [],
        items: [],
      },
      transcriptChanged: false,
    };

    const merged = mergeSessionPoll(sessions, 'session-1', snapshot);
    expect(merged[0]).toMatchObject({
      name: 'Live title',
      state: 'processing',
      active_turn_id: 'turn-live',
    });
    expect(merged[1]).toBe(sessions[1]);
  });

  it('renders a live snapshot in place of the matching persisted turn', () => {
    const activeTurn = {
      turnId: 'turn-2',
      status: 'active',
      roundIndex: 0,
      text: 'streaming',
      thinking: '',
      tools: [],
      items: [],
    };

    expect(isActiveTranscriptTurn('turn-2', activeTurn)).toBe(true);
    expect(isActiveTranscriptTurn('turn-1', activeTurn)).toBe(false);
  });

  it('keeps live output during partial persistence and clears it after completion', () => {
    const snapshot: PollSnapshot = {
      version: 8,
      changed: true,
      activeTurn: {
        turnId: 'turn-live',
        status: 'active',
        roundIndex: 1,
        text: 'partial',
        thinking: '',
        tools: [],
        items: [],
      },
      transcriptChanged: true,
    };

    expect(shouldRetainActiveTurn(snapshot)).toBe(true);
    expect(shouldRetainActiveTurn({
      ...snapshot,
      activeTurn: { ...snapshot.activeTurn!, status: 'completed' },
    })).toBe(false);
  });
});
