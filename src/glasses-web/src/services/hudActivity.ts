import type {
  ActiveTurnSnapshot,
  ChatMessage,
  ChatMessageItem,
  RemoteToolStatus,
} from './RemoteSessionManager';

export interface HudActivity {
  /** i18n key under sessions.* */
  labelKey: string;
  preview?: string;
  busy: boolean;
}

const PREVIEW_MAX = 140;

/** Match desktop pet: keep the tail of long output, not the head. */
function truncatePreview(raw: string): string | undefined {
  const text = raw
    .replace(/```[\s\S]*?```/g, ' ')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/[#>*_\-\[\]()]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
  if (!text) return undefined;
  if (text.length <= PREVIEW_MAX) return text;
  return `…${text.slice(-(PREVIEW_MAX - 1))}`;
}

function isToolStatusBusy(status: string | undefined): boolean {
  const s = (status || '').toLowerCase();
  return (
    s === 'running'
    || s === 'pending'
    || s === 'waiting'
    || s === 'active'
    || s === 'in_progress'
  );
}

function collectItemSnippets(
  items: ChatMessageItem[] | undefined,
  out: { text: string; thinking: string; toolsBusy: boolean },
): void {
  if (!items || items.length === 0) return;

  for (let i = items.length - 1; i >= 0; i -= 1) {
    const item = items[i];
    if (item.subItems?.length) {
      collectItemSnippets(item.subItems, out);
    }
    if (item.type === 'tool' && item.tool && isToolStatusBusy(item.tool.status)) {
      out.toolsBusy = true;
    }
    if (!out.text && item.type === 'text' && item.content?.trim()) {
      out.text = item.content;
    }
    if (!out.thinking && item.type === 'thinking' && item.content?.trim()) {
      out.thinking = item.content;
    }
  }
}

/**
 * Relay clears top-level text/thinking when ordered `items` are present.
 * Reconstruct the same phase signals the desktop pet gets from ProcessingPhase.
 */
function summarizeTurn(turn: ActiveTurnSnapshot): {
  text: string;
  thinking: string;
  toolsBusy: boolean;
} {
  const out = {
    text: turn.text || '',
    thinking: turn.thinking || '',
    toolsBusy: turn.tools.some((tool: RemoteToolStatus) => isToolStatusBusy(tool.status)),
  };
  collectItemSnippets(turn.items, out);
  return out;
}

/** Latest plain output from a turn snapshot (active or just-finished), untruncated. */
export function turnOutputRaw(turn: ActiveTurnSnapshot | null | undefined): string | undefined {
  if (!turn) return undefined;
  const { text, thinking } = summarizeTurn(turn);
  const raw = (text || thinking || '').trim();
  return raw || undefined;
}

/** Pet-like activity summary from relay active_turn (+ optional last assistant text). */
export function deriveHudActivity(
  turn: ActiveTurnSnapshot | null | undefined,
  lastAssistantText?: string,
): HudActivity {
  if (turn && turn.status === 'active') {
    const { text, thinking, toolsBusy } = summarizeTurn(turn);

    if (toolsBusy) {
      return {
        labelKey: 'sessions.hudUsingTools',
        preview: truncatePreview(text || thinking || lastAssistantText || ''),
        busy: true,
      };
    }
    if (text) {
      return {
        labelKey: 'sessions.hudWriting',
        preview: truncatePreview(text),
        busy: true,
      };
    }
    if (thinking) {
      return {
        labelKey: 'sessions.hudThinking',
        preview: truncatePreview(thinking),
        busy: true,
      };
    }
    return {
      labelKey: 'sessions.hudWorking',
      preview: truncatePreview(lastAssistantText || ''),
      busy: true,
    };
  }

  // Turn just finished: prefer leftover snapshot / cached assistant text over stale history.
  const leftover = turn ? summarizeTurn(turn) : null;
  return {
    labelKey: 'sessions.hudIdle',
    preview: truncatePreview(leftover?.text || leftover?.thinking || lastAssistantText || ''),
    busy: false,
  };
}

export function lastAssistantPlainText(messages: ChatMessage[]): string | undefined {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const msg = messages[i];
    if (msg.role !== 'assistant') continue;
    if (msg.items?.length) {
      for (let j = msg.items.length - 1; j >= 0; j -= 1) {
        const item = msg.items[j];
        if (item.type === 'text' && item.content?.trim()) {
          return item.content.trim();
        }
      }
    }
    const text = (msg.content || '').trim();
    if (text) return text;
  }
  return undefined;
}
