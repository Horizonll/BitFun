import type { ActiveTurnSnapshot, ChatMessage, ChatMessageItem, PollResponse } from './RemoteSessionManager';

const HOLD_MS = 5000;

/** Retries after turn end — desktop may persist the final tokens a beat late. */
export const TURN_END_REFRESH_DELAYS_MS = [0, 600, 1500, 3500] as const;

let holdDeadlineMs = 0;

/** Clear held completed-turn state (session switch / unmount). */
export function resetActiveTurnHold(): void {
  holdDeadlineMs = 0;
}

function itemPlainText(items: ChatMessageItem[] | undefined): string {
  if (!items?.length) return '';
  const chunks: string[] = [];
  const walk = (list: ChatMessageItem[]) => {
    for (const item of list) {
      if ((item.type === 'text' || item.type === 'thinking') && item.content) {
        chunks.push(item.content);
      }
      if (item.subItems?.length) walk(item.subItems);
    }
  };
  walk(items);
  return chunks.join('\n');
}

/** Best-effort plain text from a held / streaming turn for catch-up checks. */
function heldTurnPlainText(turn: ActiveTurnSnapshot | null | undefined): string {
  if (!turn) return '';
  const top = `${turn.text || ''}\n${turn.thinking || ''}`.trim();
  if (top) return top;
  return itemPlainText(turn.items).trim();
}

function lastAssistantPlainText(messages: ChatMessage[]): string {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message.role !== 'assistant') continue;
    const fromItems = itemPlainText(message.items);
    return `${message.content || ''}\n${message.thinking || ''}\n${fromItems}`.trim();
  }
  return '';
}

/**
 * Apply poll active_turn without dropping the last streaming tokens before the
 * persisted assistant message arrives.
 *
 * Desktop may clear active_turn one poll before new_messages contains the
 * final assistant content. Holding a completed snapshot bridges that gap.
 * Do not clear the hold just because new_messages arrived — that message is
 * often still truncated mid-persist.
 */
export function resolvePolledActiveTurn(
  previous: ActiveTurnSnapshot | null,
  resp: PollResponse,
  nowMs: number = Date.now(),
): ActiveTurnSnapshot | null {
  const next = resp.active_turn ?? null;
  if (next) {
    holdDeadlineMs = 0;
    return next;
  }

  if (previous?.status === 'active') {
    holdDeadlineMs = nowMs + HOLD_MS;
    return { ...previous, status: 'completed' };
  }

  if (previous?.status === 'completed' && holdDeadlineMs > nowMs) {
    return previous;
  }

  holdDeadlineMs = 0;
  return null;
}

/** True when an active turn just ended and we should pull a fresh transcript. */
export function shouldRefreshMessagesAfterTurn(
  previous: ActiveTurnSnapshot | null,
  resolved: ActiveTurnSnapshot | null,
): boolean {
  return previous?.status === 'active' && resolved?.status !== 'active';
}

/**
 * Clear the completed hold once persisted messages have caught up to (or
 * surpassed) the held stream text. Empty held text clears as soon as any
 * assistant message exists.
 */
export function shouldClearHeldTurnAfterRefresh(
  held: ActiveTurnSnapshot | null,
  messages: ChatMessage[],
): boolean {
  if (!held || held.status === 'active') return false;
  const heldText = heldTurnPlainText(held);
  const persisted = lastAssistantPlainText(messages);
  if (!persisted) return false;
  if (!heldText) return true;
  return persisted.length >= heldText.length;
}
