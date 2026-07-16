import React, { useCallback, useEffect, useRef, useState } from 'react';
import { MarkdownRenderer } from '@/component-library';
import { getOrCreateInstallationId, LanMonitorClient } from './client';
import { formatTimestamp, translate as t } from './i18n';
import { mergeOlderTranscriptPage, mergeSessionPoll } from './state';
import type {
  ActiveTurn,
  MonitorItem,
  MonitorTurn,
  PollSnapshot,
  SessionInfo,
  ToolItem,
  TranscriptPage,
  WorkspaceFacts,
} from './types';

type Screen = 'pairing' | 'monitor';

interface ExpandedResult {
  text: string;
  cursor: number;
  hasMore: boolean;
  loading: boolean;
}

const delay = (milliseconds: number) =>
  new Promise<void>(resolve => window.setTimeout(resolve, milliseconds));

function formatValue(value: unknown): string {
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value ?? '');
  }
}

function ItemShell({ item, children }: { item: MonitorItem; children: React.ReactNode }) {
  const isSubagent = Boolean(item.subagentSessionId || item.parentTaskToolId);
  return (
    <div className={`lan-monitor__item ${isSubagent ? 'lan-monitor__item--subagent' : ''}`}>
      {isSubagent && <div className="lan-monitor__subagent-label">{t('subagent')}</div>}
      {children}
    </div>
  );
}

export default function LanMonitorApp() {
  const clientRef = useRef<LanMonitorClient | null>(null);
  const knownTurnCountRef = useRef(0);
  const [screen, setScreen] = useState<Screen>('pairing');
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
  const [expandedResults, setExpandedResults] = useState<Record<string, ExpandedResult>>({});

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

  const handlePair = useCallback(async () => {
    if (!/^\d{6}$/.test(pairingCode.trim())) return;
    setPairing(true);
    setError(null);
    try {
      const client = new LanMonitorClient();
      await client.pair(pairingCode.trim(), getOrCreateInstallationId());
      clientRef.current = client;
      sessionStorage.setItem('bitfun.lan-monitor.pairing-code', pairingCode.trim());
      const workspaceResponse = await client.command({ action: 'get_workspace_info' });
      if (workspaceResponse.resp === 'lan_monitor_workspace') {
        setWorkspace(workspaceResponse.workspace);
      }
      setScreen('monitor');
      await refreshSessions();
    } catch (pairingError) {
      setError(pairingError instanceof Error ? pairingError.message : String(pairingError));
    } finally {
      setPairing(false);
    }
  }, [pairingCode, refreshSessions]);

  useEffect(() => {
    const savedCode = sessionStorage.getItem('bitfun.lan-monitor.pairing-code');
    if (savedCode && /^\d{6}$/.test(savedCode)) setPairingCode(savedCode);
  }, []);

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
            await loadTranscript(selectedSessionId);
            await refreshSessions();
          }
        } catch {
          if (cancelled) return;
          consecutiveFailures += 1;
          if (consecutiveFailures >= 5) {
            clientRef.current = null;
            sessionStorage.removeItem('bitfun.lan-monitor.pairing-code');
            setScreen('pairing');
            setError(t('connectionLost'));
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
  }, [command, loadTranscript, refreshSessions, screen, selectedSessionId]);

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

  const loadToolResult = async (turn: MonitorTurn, tool: ToolItem) => {
    if (!selectedSessionId || !tool.resultRef) return;
    const key = `${turn.turnId}:${tool.id}`;
    const existing = expandedResults[key];
    const cursor = existing?.cursor ?? 0;
    setExpandedResults(current => ({
      ...current,
      [key]: { text: existing?.text ?? '', cursor, hasMore: true, loading: true },
    }));
    try {
      const response = await command({
        action: 'get_tool_result_chunk',
        session_id: selectedSessionId,
        turn_id: turn.turnId,
        tool_id: tool.id,
        result_ref: tool.resultRef,
        cursor,
        limit: 65536,
      });
      if (response.resp !== 'lan_monitor_tool_result') return;
      setExpandedResults(current => ({
        ...current,
        [key]: {
          text: `${current[key]?.text ?? ''}${response.result.chunk}`,
          cursor: response.result.nextCursor ?? cursor + response.result.chunk.length,
          hasMore: response.result.hasMore,
          loading: false,
        },
      }));
    } catch (resultError) {
      setExpandedResults(current => ({
        ...current,
        [key]: { ...(current[key] ?? { text: '', cursor, hasMore: true }), loading: false },
      }));
      setError(resultError instanceof Error ? resultError.message : String(resultError));
    }
  };

  const control = async (request: Record<string, unknown>) => {
    try {
      await command(request);
      setError(null);
    } catch (controlError) {
      setError(controlError instanceof Error ? controlError.message : String(controlError));
    }
  };

  const renderTool = (turn: MonitorTurn, tool: ToolItem) => {
    const key = `${turn.turnId}:${tool.id}`;
    const expanded = expandedResults[key];
    const resultText = expanded ? expanded.text : formatValue(tool.result);
    return (
      <ItemShell key={tool.id} item={tool}>
        <section className={`lan-monitor__tool lan-monitor__tool--${tool.status}`}>
          <header>
            <strong>{tool.name}</strong>
            <span>{tool.status}</span>
            {tool.durationMs != null && (
              <span>{t('duration', { milliseconds: tool.durationMs })}</span>
            )}
          </header>
          <details>
            <summary>{t('toolInput')}</summary>
            <pre>{formatValue(tool.input)}</pre>
          </details>
          {tool.result !== undefined && (
            <details open>
              <summary>{t('toolOutput')}</summary>
              <pre>{resultText}</pre>
            </details>
          )}
          {tool.result === undefined && !tool.error && (
            <div className="lan-monitor__waiting-result">{t('waitingResult')}</div>
          )}
          {tool.error && (
            <div className="lan-monitor__tool-error">
              <strong>{t('toolError')}</strong>
              <pre>{tool.error}</pre>
            </div>
          )}
          {tool.resultTruncated && tool.resultRef && (expanded?.hasMore ?? true) && (
            <button disabled={expanded?.loading} onClick={() => void loadToolResult(turn, tool)}>
              {t('loadMoreResult')}
            </button>
          )}
        </section>
      </ItemShell>
    );
  };

  const renderItem = (turn: MonitorTurn, item: MonitorItem) => {
    if (item.type === 'tool') return renderTool(turn, item);
    if (item.type === 'thinking') {
      return (
        <ItemShell key={item.id} item={item}>
          <details className="lan-monitor__thinking" open={!item.isCollapsed}>
            <summary>{t('thinking')}</summary>
            <MarkdownRenderer content={item.content} />
          </details>
        </ItemShell>
      );
    }
    return (
      <ItemShell key={item.id} item={item}>
        <div className="lan-monitor__assistant-message">
          {item.isMarkdown ? (
            <MarkdownRenderer content={item.content} />
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
        <span className="lan-monitor__live-dot" />
        <strong>{t('live')}</strong>
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
        if (item.type === 'thinking') {
          return (
            <details key={`thinking-${index}`} className="lan-monitor__thinking">
              <summary>{t('thinking')}</summary>
              <MarkdownRenderer content={item.content ?? ''} isStreaming />
            </details>
          );
        }
        if (item.type === 'text') {
          return <MarkdownRenderer key={`text-${index}`} content={item.content ?? ''} isStreaming />;
        }
        const tool = turn.tools.find(candidate => candidate.id === item.tool?.id);
        if (!tool) return null;
        return (
          <section key={tool.id} className="lan-monitor__tool">
            <header>
              <strong>{tool.name}</strong>
              <span>{tool.status}</span>
            </header>
            {tool.input !== undefined && <pre>{formatValue(tool.input)}</pre>}
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
          </section>
        );
      })}
      {turn.items.length === 0 && (
        <>
          {turn.thinking && (
            <details className="lan-monitor__thinking">
              <summary>{t('thinking')}</summary>
              <MarkdownRenderer content={turn.thinking} isStreaming />
            </details>
          )}
          {turn.text && <MarkdownRenderer content={turn.text} isStreaming />}
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
          {reconnecting && <span className="lan-monitor__reconnecting">{t('reconnecting')}</span>}
        </header>
        {error && <div className="lan-monitor__error">{error}</div>}
        <div className="lan-monitor__timeline">
          {transcript?.hasMore && (
            <button className="lan-monitor__load-older" onClick={() => void loadOlder()}>
              {t('loadOlder')}
            </button>
          )}
          {transcript?.turns.map(turn => (
            <article key={turn.turnId} className="lan-monitor__turn">
              <section className="lan-monitor__user-message">
                <header>
                  <span>{formatTimestamp(turn.userMessage.timestamp)}</span>
                  <span>{turn.status}</span>
                </header>
                <MarkdownRenderer content={turn.userMessage.content} />
                {turn.userMessage.images?.map(image => (
                  <img key={image.name} src={image.data_url} alt={image.name} />
                ))}
              </section>
              {turn.rounds.map(round => (
                <section key={round.id} className="lan-monitor__round">
                  {round.items.map(item => renderItem(turn, item))}
                </section>
              ))}
            </article>
          ))}
          {activeTurn && renderActiveTurn(activeTurn)}
          {loadingTranscript && <div className="lan-monitor__loading">{t('loading')}</div>}
          {!selectedSessionId && <div className="lan-monitor__empty">{t('selectSession')}</div>}
        </div>
      </section>
    </main>
  );
}
