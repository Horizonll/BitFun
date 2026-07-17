import React, { useCallback, useEffect, useRef, useState } from 'react';
import { MarkdownRenderer } from '@/component-library';
import {
  clearPersistedPairing,
  getOrCreateInstallationId,
  LanMonitorClient,
} from './client';
import { formatTimestamp, translate as t } from './i18n';
import {
  isActiveTranscriptTurn,
  mergeOlderTranscriptPage,
  mergeSessionPoll,
  shouldRetainActiveTurn,
} from './state';
import type {
  ActiveTurn,
  MonitorItem,
  PollSnapshot,
  SessionInfo,
  ToolItem,
  TranscriptPage,
  WorkspaceFacts,
} from './types';

type Screen = 'pairing' | 'monitor';

const delay = (milliseconds: number) =>
  new Promise<void>(resolve => window.setTimeout(resolve, milliseconds));

const TOOL_SUMMARY_KEYS = [
  'search_term',
  'search_query',
  'query',
  'q',
  'url',
  'description',
  'prompt',
  'command',
  'cmd',
  'path',
  'file_path',
  'filePath',
  'pattern',
  'search_pattern',
  'task',
];

function compactText(value: string, maxLength = 120): string {
  const normalized = value.replace(/\s+/g, ' ').trim();
  return normalized.length > maxLength ? `${normalized.slice(0, maxLength - 1)}…` : normalized;
}

function readToolSummaryValue(input: unknown): string {
  if (typeof input === 'string') return compactText(input);
  if (typeof input === 'number' || typeof input === 'boolean') return String(input);
  if (Array.isArray(input)) return `${input.length} items`;
  if (!input || typeof input !== 'object') return '';
  const values = input as Record<string, unknown>;
  for (const key of TOOL_SUMMARY_KEYS) {
    const value = values[key];
    if (typeof value === 'string' && value.trim()) return compactText(value);
  }
  const firstString = Object.values(values).find(value => typeof value === 'string' && value.trim());
  return typeof firstString === 'string' ? compactText(firstString) : '';
}

function toolSummary(
  name: string,
  input: unknown,
  subagentModelDisplayName: string | undefined,
  translate: typeof t,
): { label: string; detail: string } {
  const normalizedName = name.toLowerCase().replace(/[-_\s]/g, '');
  const inputRecord = input && typeof input === 'object' && !Array.isArray(input)
    ? input as Record<string, unknown>
    : null;
  const subagentType = inputRecord
    ? [inputRecord.subagent_type, inputRecord.subagentType, inputRecord.agent_type, inputRecord.agentType]
      .find(value => typeof value === 'string' && value.trim()) as string | undefined
    : undefined;
  const isSubagentTask = normalizedName === 'task' || normalizedName.includes('subagent');
  if (isSubagentTask) {
    return {
      label: compactText(subagentType || subagentModelDisplayName || translate('toolSummary.subagent'), 48),
      detail: readToolSummaryValue(input),
    };
  }
  if (normalizedName.includes('websearch') || normalizedName === 'search') {
    return { label: translate('toolSummary.webSearch'), detail: readToolSummaryValue(input) };
  }
  if (normalizedName.includes('webfetch') || normalizedName === 'fetch') {
    return { label: translate('toolSummary.webFetch'), detail: readToolSummaryValue(input) };
  }
  if (normalizedName.includes('exec') || normalizedName.includes('terminal') || normalizedName === 'shell') {
    return { label: translate('toolSummary.command'), detail: readToolSummaryValue(input) };
  }
  return { label: compactText(name, 48), detail: readToolSummaryValue(input) };
}

function toolStatusLabel(status: string, translate: typeof t): string {
  const key = status.toLowerCase();
  const statusKeys: Record<string, string> = {
    queued: 'toolStatus.queued',
    pending: 'toolStatus.pending',
    pending_confirmation: 'toolStatus.pendingConfirmation',
    preparing: 'toolStatus.preparing',
    running: 'toolStatus.running',
    streaming: 'toolStatus.running',
    completed: 'toolStatus.completed',
    cancelled: 'toolStatus.cancelled',
    rejected: 'toolStatus.rejected',
    failed: 'toolStatus.error',
    error: 'toolStatus.error',
  };
  return translate(statusKeys[key] ?? 'toolStatus.running');
}

function formatToolDuration(durationMs: number, translate: typeof t): string {
  if (durationMs < 1000) return translate('duration', { milliseconds: Math.round(durationMs) });
  const totalSeconds = Math.floor(durationMs / 1000);
  if (totalSeconds < 60) return translate('durationSeconds', { seconds: totalSeconds });
  return translate('durationMinutes', {
    minutes: Math.floor(totalSeconds / 60),
    seconds: totalSeconds % 60,
  });
}

function ItemShell({ children }: { children: React.ReactNode }) {
  return <div className="lan-monitor__item">{children}</div>;
}

function DarkMarkdownRenderer(props: React.ComponentProps<typeof MarkdownRenderer>) {
  return <MarkdownRenderer {...props} initializeTheme={false} />;
}

export default function LanMonitorApp() {
  const clientRef = useRef<LanMonitorClient | null>(null);
  const knownTurnCountRef = useRef(0);
  const [screen, setScreen] = useState<Screen>(() =>
    LanMonitorClient.hasPersistedPairing() ? 'monitor' : 'pairing',
  );
  const [pairingCode, setPairingCode] = useState('');
  const [pairing, setPairing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reconnecting, setReconnecting] = useState(false);
  const [workspace, setWorkspace] = useState<WorkspaceFacts | null>(null);
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [transcript, setTranscript] = useState<TranscriptPage | null>(null);
  const [activeTurn, setActiveTurn] = useState<ActiveTurn | null>(null);
  const [loadingTranscript, setLoadingTranscript] = useState(false);
  const [clockMs, setClockMs] = useState(Date.now());
  const timelineRef = useRef<HTMLDivElement | null>(null);
  const followOutputRef = useRef(true);
  const [isFollowingOutput, setIsFollowingOutput] = useState(true);

  const command = useCallback(async (request: Record<string, unknown>) => {
    const client = clientRef.current;
    if (!client) throw new Error(t('connectionLost'));
    return client.command(request);
  }, []);

  const refreshSessions = useCallback(async () => {
    const response = await command({ action: 'list_sessions', limit: 100, offset: 0 });
    if (response.resp !== 'lan_monitor_sessions') return;
    setSessions(response.sessions);
    setSelectedSessionId(current => {
      if (current && response.sessions.some(session => session.session_id === current)) {
        return current;
      }
      return response.sessions[0]?.session_id ?? null;
    });
  }, [command]);

  const enterMonitor = useCallback(
    async (client: LanMonitorClient) => {
      clientRef.current = client;
      const workspaceResponse = await client.command({ action: 'get_workspace_info' });
      if (workspaceResponse.resp === 'lan_monitor_workspace') {
        setWorkspace(workspaceResponse.workspace);
      }
      setScreen('monitor');
      await refreshSessions();
    },
    [refreshSessions],
  );

  const leaveMonitor = useCallback((message: string | null = null) => {
    clientRef.current?.forgetPairing();
    clientRef.current = null;
    clearPersistedPairing();
    sessionStorage.removeItem('bitfun.lan-monitor.pairing-code');
    setScreen('pairing');
    setPairingCode('');
    setWorkspace(null);
    setSessions([]);
    setSelectedSessionId(null);
    setTranscript(null);
    setActiveTurn(null);
    setError(message);
  }, []);

  const loadTranscript = useCallback(
    async (sessionId: string) => {
      setLoadingTranscript(true);
      try {
        const response = await command({
          action: 'get_transcript_page',
          session_id: sessionId,
          limit: 50,
        });
        if (response.resp === 'lan_monitor_transcript') {
          knownTurnCountRef.current = response.page.totalTurnCount;
          setTranscript(response.page);
        }
      } finally {
        setLoadingTranscript(false);
      }
    },
    [command],
  );

  const handleTimelineScroll = useCallback(() => {
    const timeline = timelineRef.current;
    if (!timeline) return;
    const distanceFromBottom = timeline.scrollHeight - timeline.scrollTop - timeline.clientHeight;
    const shouldFollow = distanceFromBottom <= 32;
    followOutputRef.current = shouldFollow;
    setIsFollowingOutput(current => (current === shouldFollow ? current : shouldFollow));
  }, []);

  const scrollToLatest = useCallback((behavior: ScrollBehavior = 'smooth') => {
    const timeline = timelineRef.current;
    if (!timeline) return;
    followOutputRef.current = true;
    setIsFollowingOutput(true);
    timeline.scrollTo({ top: timeline.scrollHeight, behavior });
  }, []);

  useEffect(() => {
    followOutputRef.current = true;
    setIsFollowingOutput(true);
  }, [selectedSessionId]);

  useEffect(() => {
    if (!followOutputRef.current) return;
    const frame = window.requestAnimationFrame(() => {
      const timeline = timelineRef.current;
      if (timeline) timeline.scrollTo({ top: timeline.scrollHeight, behavior: 'auto' });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [activeTurn, transcript]);

  useEffect(() => {
    const tracksElapsedTime = activeTurn?.tools.some(tool =>
      tool.startMs != null && ['pending', 'preparing', 'running', 'streaming'].includes(tool.status),
    );
    if (!tracksElapsedTime) return;
    setClockMs(Date.now());
    const timer = window.setInterval(() => setClockMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [activeTurn]);

  const handlePair = useCallback(async () => {
    if (!/^\d{6}$/.test(pairingCode.trim())) return;
    setPairing(true);
    setError(null);
    try {
      const client = new LanMonitorClient();
      await client.pair(pairingCode.trim(), getOrCreateInstallationId());
      await enterMonitor(client);
    } catch (pairingError) {
      setError(pairingError instanceof Error ? pairingError.message : String(pairingError));
    } finally {
      setPairing(false);
    }
  }, [enterMonitor, pairingCode]);

  useEffect(() => {
    const client = LanMonitorClient.restore();
    if (!client) {
      setScreen('pairing');
      return;
    }
    setPairing(true);
    void enterMonitor(client).catch(restoreError => {
      leaveMonitor(restoreError instanceof Error ? restoreError.message : String(restoreError));
    }).finally(() => setPairing(false));
  }, [enterMonitor, leaveMonitor]);

  useEffect(() => {
    if (screen !== 'monitor' || !selectedSessionId) {
      setTranscript(null);
      setActiveTurn(null);
      return;
    }
    void loadTranscript(selectedSessionId).catch(loadError => {
      setError(loadError instanceof Error ? loadError.message : String(loadError));
    });
  }, [loadTranscript, screen, selectedSessionId]);

  useEffect(() => {
    if (screen !== 'monitor' || !selectedSessionId) return;
    let cancelled = false;
    let version = 0;
    let consecutiveFailures = 0;

    const poll = async () => {
      while (!cancelled) {
        try {
          const response = await command({
            action: 'poll_session',
            session_id: selectedSessionId,
            since_version: version,
            known_turn_count: knownTurnCountRef.current,
          });
          if (cancelled || response.resp !== 'lan_monitor_poll') continue;
          const snapshot: PollSnapshot = response.snapshot;
          version = snapshot.version;
          consecutiveFailures = 0;
          setReconnecting(false);
          if (snapshot.changed) {
            setActiveTurn(snapshot.activeTurn ?? null);
            setSessions(current => mergeSessionPoll(current, selectedSessionId, snapshot));
          }
          if (snapshot.transcriptChanged) {
            if (snapshot.totalTurnCount != null) {
              knownTurnCountRef.current = snapshot.totalTurnCount;
            }
            const refreshTurnId = snapshot.activeTurn?.turnId;
            const retainActiveTurn = shouldRetainActiveTurn(snapshot);
            void loadTranscript(selectedSessionId).then(() => {
              if (!cancelled && !retainActiveTurn && refreshTurnId) {
                setActiveTurn(current => current?.turnId === refreshTurnId ? null : current);
              }
            }).catch(loadError => {
              if (!cancelled) {
                setError(loadError instanceof Error ? loadError.message : String(loadError));
              }
            });
          }
        } catch {
          if (cancelled) return;
          consecutiveFailures += 1;
          if (consecutiveFailures >= 5) {
            leaveMonitor(t('connectionLost'));
            return;
          }
          setReconnecting(true);
          await delay(2000);
        }
      }
    };
    void poll();
    return () => {
      cancelled = true;
    };
  }, [command, leaveMonitor, loadTranscript, screen, selectedSessionId]);

  const loadOlder = async () => {
    if (!selectedSessionId || !transcript?.nextBeforeTurnId) return;
    const response = await command({
      action: 'get_transcript_page',
      session_id: selectedSessionId,
      limit: 50,
      before_turn_id: transcript.nextBeforeTurnId,
    });
    if (response.resp !== 'lan_monitor_transcript') return;
    setTranscript(current =>
      current ? mergeOlderTranscriptPage(current, response.page) : response.page,
    );
  };

  const control = async (request: Record<string, unknown>) => {
    try {
      await command(request);
      setError(null);
    } catch (controlError) {
      setError(controlError instanceof Error ? controlError.message : String(controlError));
    }
  };

  const renderTool = (tool: ToolItem) => {
    const summary = toolSummary(
      tool.name,
      tool.input,
      tool.subagentModelDisplayName,
      t,
    );
    const status = tool.error || tool.success === false ? 'error' : tool.status;
    return (
      <ItemShell key={tool.id}>
        <section className={`lan-monitor__tool lan-monitor__tool--${status}`}>
          <header>
            <strong>{summary.label}</strong>
            {summary.detail && <span className="lan-monitor__tool-summary">{summary.detail}</span>}
            <span className="lan-monitor__tool-status">{toolStatusLabel(status, t)}</span>
            {tool.durationMs != null && (
              <span>{formatToolDuration(tool.durationMs, t)}</span>
            )}
          </header>
        </section>
      </ItemShell>
    );
  };

  const renderItem = (item: MonitorItem) => {
    if (item.parentTaskToolId || (item.subagentSessionId && item.type !== 'tool')) return null;
    if (item.type === 'tool') return renderTool(item);
    if (item.type === 'thinking') {
      return (
        <ItemShell key={item.id}>
          <details className="lan-monitor__thinking" open={!item.isCollapsed}>
            <summary>{t('thinking')}</summary>
            <DarkMarkdownRenderer content={item.content} />
          </details>
        </ItemShell>
      );
    }
    return (
      <ItemShell key={item.id}>
        <div className="lan-monitor__assistant-message">
          {item.isMarkdown ? (
            <DarkMarkdownRenderer content={item.content} />
          ) : (
            <pre>{item.content}</pre>
          )}
        </div>
      </ItemShell>
    );
  };

  const renderActiveTurn = (turn: ActiveTurn) => (
    <section className="lan-monitor__active-turn">
      <header>
        <span>{turn.status}</span>
        <button
          onClick={() =>
            void control({
              action: 'cancel_task',
              session_id: selectedSessionId,
              turn_id: turn.turnId,
            })
          }
        >
          {t('cancelTask')}
        </button>
      </header>
      {turn.items.map((item, index) => {
        if (item.is_subagent) return null;
        if (item.type === 'thinking') {
          return (
            <details key={`thinking-${index}`} className="lan-monitor__thinking">
              <summary>{t('thinking')}</summary>
              <DarkMarkdownRenderer content={item.content ?? ''} isStreaming />
            </details>
          );
        }
        if (item.type === 'text') {
          return <DarkMarkdownRenderer key={`text-${index}`} content={item.content ?? ''} isStreaming />;
        }
        const tool = turn.tools.find(candidate => candidate.id === item.tool?.id);
        if (!tool) return null;
        const summary = toolSummary(tool.name, tool.input, undefined, t);
        const durationMs = tool.durationMs ?? (
          tool.startMs != null ? Math.max(0, clockMs - tool.startMs) : undefined
        );
        return (
          <section key={tool.id} className={`lan-monitor__tool lan-monitor__tool--${tool.status}`}>
            <header>
              <strong>{summary.label}</strong>
              {summary.detail && <span className="lan-monitor__tool-summary">{summary.detail}</span>}
              <span className="lan-monitor__tool-status">{toolStatusLabel(tool.status, t)}</span>
              {durationMs != null && (
                <span>{formatToolDuration(durationMs, t)}</span>
              )}
              {tool.status === 'pending_confirmation' && selectedSessionId && (
                <div className="lan-monitor__tool-actions">
                  <button
                    onClick={() =>
                      void control({
                        action: 'confirm_tool',
                        session_id: selectedSessionId,
                        tool_id: tool.id,
                      })
                    }
                  >
                    {t('confirmTool')}
                  </button>
                  <button
                    onClick={() =>
                      void control({
                        action: 'reject_tool',
                        session_id: selectedSessionId,
                        tool_id: tool.id,
                      })
                    }
                  >
                    {t('rejectTool')}
                  </button>
                </div>
              )}
            </header>
          </section>
        );
      })}
      {turn.items.length === 0 && (
        <>
          {turn.thinking && (
            <details className="lan-monitor__thinking">
              <summary>{t('thinking')}</summary>
              <DarkMarkdownRenderer content={turn.thinking} isStreaming />
            </details>
          )}
          {turn.text && <DarkMarkdownRenderer content={turn.text} isStreaming />}
        </>
      )}
    </section>
  );

  if (screen === 'pairing') {
    return (
      <main className="lan-monitor-pairing">
        <section>
          <img src="/Logo-ICON-128.png" alt="BitFun" />
          <h1>{t('title')}</h1>
          <p>{t('subtitle')}</p>
          <label>
            <span>{t('pairingCode')}</span>
            <input
              value={pairingCode}
              onChange={event => setPairingCode(event.target.value.replace(/\D/g, '').slice(0, 6))}
              placeholder={t('pairingPlaceholder')}
              inputMode="numeric"
              autoFocus
            />
          </label>
          <button disabled={pairing || pairingCode.length !== 6} onClick={() => void handlePair()}>
            {pairing ? t('connecting') : t('connect')}
          </button>
          {error && <div className="lan-monitor__error">{error}</div>}
        </section>
      </main>
    );
  }

  const selectedSession = sessions.find(session => session.session_id === selectedSessionId);
  const activeTurnIsInTranscript = Boolean(
    activeTurn && transcript?.turns.some(turn => turn.turnId === activeTurn.turnId),
  );
  return (
    <main className="lan-monitor">
      <aside className="lan-monitor__sidebar">
        <div className="lan-monitor__brand">
          <img src="/Logo-ICON-128.png" alt="BitFun" />
          <div>
            <strong>{t('title')}</strong>
            <span>{workspace?.name ?? t('workspace')}</span>
          </div>
        </div>
        <h2>{t('sessions')}</h2>
        <div className="lan-monitor__session-list">
          {sessions.map(session => (
            <button
              key={session.session_id}
              className={session.session_id === selectedSessionId ? 'is-active' : ''}
              onClick={() => setSelectedSessionId(session.session_id)}
            >
              <strong>{session.name}</strong>
              <span>
                {session.state ?? session.agent_type}
                {session.queue_depth ? ` · ${t('queueDepth', { count: session.queue_depth })}` : ''}
              </span>
            </button>
          ))}
          {sessions.length === 0 && <p>{t('noSessions')}</p>}
        </div>
      </aside>
      <section className="lan-monitor__content">
        <header className="lan-monitor__topbar">
          <div>
            <strong>{selectedSession?.name ?? t('selectSession')}</strong>
            <span>{selectedSession?.agent_type}</span>
          </div>
          <div className="lan-monitor__topbar-actions">
            {reconnecting && <span className="lan-monitor__reconnecting">{t('reconnecting')}</span>}
            <button className="lan-monitor__disconnect" onClick={() => leaveMonitor()}>
              {t('disconnect')}
            </button>
          </div>
        </header>
        {error && <div className="lan-monitor__error">{error}</div>}
        <div className="lan-monitor__timeline-shell">
          <div
            ref={timelineRef}
            className="lan-monitor__timeline"
            onScroll={handleTimelineScroll}
          >
            {transcript?.hasMore && (
              <button className="lan-monitor__load-older" onClick={() => void loadOlder()}>
                {t('loadOlder')}
              </button>
            )}
            {transcript?.turns.map(turn => {
              const liveTurn = isActiveTranscriptTurn(turn.turnId, activeTurn) ? activeTurn : null;
              return (
                <article key={turn.turnId} className="lan-monitor__turn">
                  <section className="lan-monitor__user-message">
                    <header>
                      <span>{formatTimestamp(turn.userMessage.timestamp)}</span>
                      <span>{turn.status}</span>
                    </header>
                    <DarkMarkdownRenderer content={turn.userMessage.content} />
                    {turn.userMessage.images?.map(image => (
                      <img key={image.name} src={image.data_url} alt={image.name} />
                    ))}
                  </section>
                  {liveTurn ? renderActiveTurn(liveTurn) : turn.rounds.map(round => (
                    <section key={round.id} className="lan-monitor__round">
                      {round.items.map(item => renderItem(item))}
                    </section>
                  ))}
                </article>
              );
            })}
            {activeTurn && !activeTurnIsInTranscript && renderActiveTurn(activeTurn)}
            {loadingTranscript && <div className="lan-monitor__loading">{t('loading')}</div>}
            {!selectedSessionId && <div className="lan-monitor__empty">{t('selectSession')}</div>}
          </div>
          {!isFollowingOutput && (
            <button
              className="lan-monitor__scroll-latest"
              onClick={() => scrollToLatest()}
              aria-label={t('scrollToLatest')}
              title={t('scrollToLatest')}
            >
              ↓
            </button>
          )}
        </div>
      </section>
    </main>
  );
}
