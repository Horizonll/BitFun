import React, {
  useEffect,
  useLayoutEffect,
  useRef,
  useCallback,
  useState,
  type RefObject,
} from 'react';
import { createPortal } from 'react-dom';
import { useControlTargetEpoch } from '../hooks/useControlTargetEpoch';
import { useI18n } from '../i18n';
import {
  isRemoteControlTargetChangedError,
  RemoteSessionManager,
  type RecentWorkspaceEntry,
  type SessionInfo,
} from '../services/RemoteSessionManager';
import { useMobileStore } from '../services/store';

/** Fixed visible sessions on glasses rails — no scroll / load-more. */
const VR_SESSION_CAP = 6;
/** Max workspace rows in the left-rail tree (sessions fill remaining slots). */
const VR_WORKSPACE_CAP = 3;
/** Total left-rail rows (workspace + nested session) under Expert mode. */
const VR_LEFT_ROW_CAP = 6;

type DisplayMode = 'pro' | 'assistant';

type WorkspaceListEntry = {
  path: string;
  name: string;
  last_opened: string;
  workspace_kind?: 'normal' | 'assistant' | 'remote';
  remote_connection_id?: string;
  remote_ssh_host?: string;
};

export interface SessionListVrHosts {
  sessions: RefObject<HTMLElement | null>;
  controls: RefObject<HTMLElement | null>;
}

interface SessionListPageProps {
  sessionMgr: RemoteSessionManager;
  onSelectSession: (sessionId: string, sessionName?: string, isNew?: boolean) => void;
  /** Highlight the open chat session in VR shell. */
  selectedSessionId?: string | null;
  /**
   * Sessions and controls are portaled into the left/right HUD hosts
   * (single data owner, no duplicate loaders).
   */
  vrHosts: SessionListVrHosts;
}

type SessionListTargetOwner = {
  sessionMgr: RemoteSessionManager;
  epoch: number;
  active: boolean;
};

/**
 * Resolve the epoch owned by one render. The explicit renderedEpoch check is
 * what prevents an old timer/poll closure from borrowing a newer mutable ref
 * owner during the render-to-passive-cleanup window.
 */
export function captureSessionListOwnerEpoch(
  owner: SessionListTargetOwner,
  sessionMgr: RemoteSessionManager,
  renderedEpoch: number,
): number | null {
  if (
    !owner.active
    || owner.sessionMgr !== sessionMgr
    || owner.epoch !== renderedEpoch
    || sessionMgr.controlTargetEpoch !== renderedEpoch
  ) return null;
  return renderedEpoch;
}

function formatTime(
  unixStr: string,
  formatDate: (date: Date | number, options?: Intl.DateTimeFormatOptions) => string,
  t: (key: string, params?: Record<string, string | number>) => string,
): string {
  const ts = parseInt(unixStr, 10);
  if (!ts || isNaN(ts)) return '';
  const date = new Date(ts * 1000);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 1) return t('common.justNow');
  if (diffMin < 60) return t('common.minutesAgo', { count: diffMin });
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return t('common.hoursAgo', { count: diffHr });
  const diffDay = Math.floor(diffHr / 24);
  if (diffDay < 7) return t('common.daysAgo', { count: diffDay });
  return formatDate(date);
}

function agentLabel(agentType: string, t: (key: string) => string): string {
  switch (agentType) {
    case 'code':
    case 'agentic':
      return t('sessions.agentCode');
    case 'cowork':
    case 'Cowork':
      return t('sessions.agentCowork');
    case 'claw':
    case 'Claw':
      return t('shared.agents.claw');
    default:
      return agentType || t('sessions.agentDefault');
  }
}

function isCoworkAgent(agentType: string): boolean {
  return agentType === 'cowork' || agentType === 'Cowork';
}

function isClawAgent(agentType: string): boolean {
  return agentType === 'claw' || agentType === 'Claw';
}

/** Pick first workspace suitable for Expert mode (exclude Claw assistant roots when kind is known). */
function pickFirstProWorkspace(list: RecentWorkspaceEntry[]): RecentWorkspaceEntry | undefined {
  return filterProWorkspaces(list)[0];
}

/** Expert-mode workspace roots only (exclude assistant kind when the host reports it). */
function filterProWorkspaces(list: RecentWorkspaceEntry[]): RecentWorkspaceEntry[] {
  if (list.length === 0) return [];
  const anyKind = list.some((w) => w.workspace_kind != null);
  if (anyKind) {
    return list.filter((w) => w.workspace_kind !== 'assistant');
  }
  return list;
}

function workspaceIdentityKey(workspace: {
  path?: string | null;
  remote_connection_id?: string | null;
  remote_ssh_host?: string | null;
}): string {
  return [
    workspace.remote_connection_id ?? 'local',
    workspace.remote_ssh_host ?? '',
    workspace.path ?? '',
  ].join(':');
}

function sameWorkspaceIdentity(
  a: {
    path?: string | null;
    remote_connection_id?: string | null;
    remote_ssh_host?: string | null;
  } | null | undefined,
  b: {
    path?: string | null;
    remote_connection_id?: string | null;
    remote_ssh_host?: string | null;
  } | null | undefined,
): boolean {
  if (!a?.path || !b?.path) return false;
  return workspaceIdentityKey(a) === workspaceIdentityKey(b);
}

/**
 * Keep the current workspace visible in the capped left-rail tree, prefer it first,
 * then fill remaining slots from recent pro workspaces.
 */
function visibleProWorkspaces(
  list: RecentWorkspaceEntry[],
  current: {
    path?: string | null;
    project_name?: string | null;
    workspace_kind?: 'normal' | 'assistant' | 'remote';
    remote_connection_id?: string | null;
    remote_ssh_host?: string | null;
  } | null,
  cap: number,
): WorkspaceListEntry[] {
  const pro = filterProWorkspaces(list);
  const mapped: WorkspaceListEntry[] = pro.map((w) => ({
    path: w.path,
    name: w.name,
    last_opened: w.last_opened,
    workspace_kind: w.workspace_kind,
    remote_connection_id: w.remote_connection_id,
    remote_ssh_host: w.remote_ssh_host,
  }));

  if (!current?.path) return mapped.slice(0, cap);

  const currentEntry: WorkspaceListEntry = {
    path: current.path,
    name: current.project_name || current.path,
    last_opened: '',
    workspace_kind: current.workspace_kind,
    remote_connection_id: current.remote_connection_id ?? undefined,
    remote_ssh_host: current.remote_ssh_host ?? undefined,
  };
  const others = mapped.filter((w) => !sameWorkspaceIdentity(w, currentEntry));
  const fromList = mapped.find((w) => sameWorkspaceIdentity(w, currentEntry));
  return [fromList ?? currentEntry, ...others].slice(0, cap);
}

function SessionTypeIcon({ agentType }: { agentType: string }) {
  if (isCoworkAgent(agentType)) {
    return (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
        <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
        <circle cx="9" cy="7" r="4" />
        <path d="M22 21v-2a4 4 0 0 0-3-3.87" />
        <path d="M16 3.13a4 4 0 0 1 0 7.75" />
      </svg>
    );
  }

  if (isClawAgent(agentType)) {
    return (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
        <rect width="20" height="14" x="2" y="5" rx="2" />
        <path d="M2 10h20" />
      </svg>
    );
  }

  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
    </svg>
  );
}

/* Mode Selection Icons */
const ProModeIcon = () => (
  <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="4 17 10 11 4 5" />
    <line x1="12" y1="19" x2="20" y2="19" />
  </svg>
);

const AssistantModeIcon = () => (
  <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 8V4H8" />
    <rect width="16" height="12" x="4" y="8" rx="2" />
    <path d="M2 14h2" />
    <path d="M20 14h2" />
    <path d="M15 13v2" />
    <path d="M9 13v2" />
  </svg>
);

const WorkspaceIcon = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.93a2 2 0 0 1 1.66.9l.82 1.2a2 2 0 0 0 1.66.9H18a2 2 0 0 1 2 2v2"/>
  </svg>
);

const SessionListPage: React.FC<SessionListPageProps> = ({
  sessionMgr,
  onSelectSession,
  selectedSessionId = null,
  vrHosts,
}) => {
  const listPageSize = VR_SESSION_CAP;
  const { t, formatDate } = useI18n();
  const {
    sessions,
    setSessions,
    setError,
    currentWorkspace,
    setCurrentWorkspace,
    currentAssistant,
    setCurrentAssistant,
    setPairedDisplayMode,
  } = useMobileStore();
  const [creating, setCreating] = useState(false);
  const [loading, setLoading] = useState(false);
  const [targetInitializing, setTargetInitializing] = useState(true);
  const targetInitializingRef = useRef(true);
  const [displayMode, setDisplayMode] = useState<DisplayMode>(() => {
    const hint = useMobileStore.getState().pairedDisplayMode;
    if (hint === 'assistant' || hint === 'pro') return hint;
    return 'pro';
  });

  const [assistantList, setAssistantList] = useState<Array<{ path: string; name: string; assistant_id?: string }>>([]);
  const [showAssistantPicker, setShowAssistantPicker] = useState(false);
  const [workspaceList, setWorkspaceList] = useState<WorkspaceListEntry[]>([]);

  // Glasses HUD has no session search box; list APIs still accept an empty query.
  const searchQuery = '';
  const [menuSession, setMenuSession] = useState<SessionInfo | null>(null);
  const [renameTarget, setRenameTarget] = useState<SessionInfo | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [deleteConfirmTarget, setDeleteConfirmTarget] = useState<SessionInfo | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [actionToast, setActionToast] = useState<string | null>(null);

  const longPressTimerRef = useRef<ReturnType<typeof setTimeout>>();
  const longPressPosRef = useRef({ x: 0, y: 0 });
  const longPressTriggeredRef = useRef(false);
  const toastTimerRef = useRef<ReturnType<typeof setTimeout>>();
  const controlTargetEpoch = useControlTargetEpoch(sessionMgr);
  const sessionListOwnerRef = useRef({
    sessionMgr,
    epoch: controlTargetEpoch,
    active: true,
  });
  if (
    sessionListOwnerRef.current.sessionMgr !== sessionMgr
    || sessionListOwnerRef.current.epoch !== controlTargetEpoch
  ) {
    sessionListOwnerRef.current = {
      sessionMgr,
      epoch: controlTargetEpoch,
      active: true,
    };
  }

  const captureSessionListEpoch = useCallback((): number | null => {
    return captureSessionListOwnerEpoch(
      sessionListOwnerRef.current,
      sessionMgr,
      controlTargetEpoch,
    );
  }, [controlTargetEpoch, sessionMgr]);

  const isSessionListCurrent = useCallback((epoch: number | null): boolean => {
    const owner = sessionListOwnerRef.current;
    return epoch !== null
      && owner.active
      && owner.sessionMgr === sessionMgr
      && owner.epoch === epoch
      && sessionMgr.controlTargetEpoch === epoch;
  }, [controlTargetEpoch, sessionMgr]);

  // ── Long-press context menu ─────────────────────────────────────
  const clearLongPressTimer = () => {
    if (longPressTimerRef.current) {
      clearTimeout(longPressTimerRef.current);
      longPressTimerRef.current = undefined;
    }
  };

  const handleSessionTouchStart = useCallback((s: SessionInfo, e: React.TouchEvent) => {
    if (deleting || renaming) return;
    clearLongPressTimer();
    longPressTriggeredRef.current = false;
    longPressPosRef.current = { x: e.touches[0].clientX, y: e.touches[0].clientY };
    longPressTimerRef.current = setTimeout(() => {
      longPressTriggeredRef.current = true;
      setMenuSession(s);
      longPressTimerRef.current = undefined;
    }, 500);
  }, [deleting, renaming]);

  const handleSessionTouchMove = useCallback((e: React.TouchEvent) => {
    const dx = Math.abs(e.touches[0].clientX - longPressPosRef.current.x);
    const dy = Math.abs(e.touches[0].clientY - longPressPosRef.current.y);
    if (dx > 10 || dy > 10) {
      clearLongPressTimer();
    }
  }, []);

  const handleSessionTouchEnd = useCallback(() => {
    clearLongPressTimer();
  }, []);

  const handleSessionClick = useCallback((s: SessionInfo, e: React.MouseEvent) => {
    if (longPressTriggeredRef.current) {
      e.preventDefault();
      e.stopPropagation();
      longPressTriggeredRef.current = false;
      return;
    }
    onSelectSession(s.session_id, s.name);
  }, [onSelectSession]);

  // ── Session actions ─────────────────────────────────────────────
  const showToast = useCallback((msg: string) => {
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    setActionToast(msg);
    toastTimerRef.current = setTimeout(() => setActionToast(null), 2500);
  }, []);

  // Cleanup timers on unmount
  useEffect(() => {
    return () => {
      clearLongPressTimer();
      if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    };
  }, []);

  const handleRename = useCallback(async () => {
    if (!renameTarget || !renameValue.trim()) return;
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return;
    setRenaming(true);
    try {
      await sessionMgr.renameSession(renameTarget.session_id, renameValue.trim());
      if (!isSessionListCurrent(targetEpoch)) return;
      useMobileStore.getState().updateSessionName(renameTarget.session_id, renameValue.trim());
      setRenameTarget(null);
      setMenuSession(null);
    } catch (e: any) {
      if (isSessionListCurrent(targetEpoch) && !isRemoteControlTargetChangedError(e)) {
        showToast(e.message || t('sessions.renameFailed'));
      }
    } finally {
      if (isSessionListCurrent(targetEpoch)) setRenaming(false);
    }
  }, [captureSessionListEpoch, isSessionListCurrent, renameTarget, renameValue, sessionMgr, showToast, t]);

  const handleDelete = useCallback(async () => {
    if (!deleteConfirmTarget) return;
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return;
    setDeleting(true);
    try {
      await sessionMgr.deleteSession(deleteConfirmTarget.session_id);
      if (!isSessionListCurrent(targetEpoch)) return;
      useMobileStore.getState().removeSession(deleteConfirmTarget.session_id);
      setDeleteConfirmTarget(null);
      setMenuSession(null);
      showToast(t('sessions.deleted'));
    } catch (e: any) {
      if (isSessionListCurrent(targetEpoch) && !isRemoteControlTargetChangedError(e)) {
        showToast(e.message || t('sessions.deleteFailed'));
      }
    } finally {
      if (isSessionListCurrent(targetEpoch)) setDeleting(false);
    }
  }, [captureSessionListEpoch, deleteConfirmTarget, isSessionListCurrent, sessionMgr, showToast, t]);

  const offsetRef = useRef(0);
  const listRequestSeqRef = useRef(0);
  const initLoadedPathRef = useRef<string | undefined>(undefined);

  const committedSessionListTargetRef = useRef({ sessionMgr, epoch: controlTargetEpoch });
  useLayoutEffect(() => {
    const previous = committedSessionListTargetRef.current;
    const targetChanged = previous.sessionMgr !== sessionMgr
      || previous.epoch !== controlTargetEpoch;
    const owner = sessionListOwnerRef.current;
    owner.active = owner.sessionMgr === sessionMgr
      && owner.epoch === controlTargetEpoch
      && sessionMgr.controlTargetEpoch === controlTargetEpoch;
    if (targetChanged) {
      targetInitializingRef.current = true;
      setTargetInitializing(true);
      listRequestSeqRef.current += 1;
      setLoading(false);
      clearLongPressTimer();
      longPressTriggeredRef.current = false;
      if (toastTimerRef.current) {
        clearTimeout(toastTimerRef.current);
        toastTimerRef.current = undefined;
      }
      setCreating(false);
      setRenaming(false);
      setDeleting(false);
      setAssistantList([]);
      setWorkspaceList([]);
      setShowAssistantPicker(false);
      setMenuSession(null);
      setRenameTarget(null);
      setRenameValue('');
      setDeleteConfirmTarget(null);
      setActionToast(null);
      setDisplayMode('pro');
      setSessions([]);
      setCurrentWorkspace(null);
      setCurrentAssistant(null);
      setPairedDisplayMode(null);
      setError(null);
      offsetRef.current = 0;
      initLoadedPathRef.current = undefined;
    }
    committedSessionListTargetRef.current = { sessionMgr, epoch: controlTargetEpoch };
    return () => {
      owner.active = false;
      listRequestSeqRef.current += 1;
    };
  }, [
    controlTargetEpoch,
    sessionMgr,
    setCurrentAssistant,
    setCurrentWorkspace,
    setError,
    setPairedDisplayMode,
    setSessions,
  ]);

  // Load assistant list when entering assistant mode
  const loadAssistantList = useCallback(async () => {
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return undefined;
    try {
      const assistants = await sessionMgr.listAssistants();
      if (!isSessionListCurrent(targetEpoch)) return undefined;
      setAssistantList(assistants);
      // Set default assistant if none selected
      if (!currentAssistant && assistants.length > 0) {
        const defaultAssistant = assistants.find(a => !a.assistant_id) || assistants[0];
        setCurrentAssistant(defaultAssistant);
        return defaultAssistant.path;
      }
      return currentAssistant?.path;
    } catch (e: any) {
      if (isSessionListCurrent(targetEpoch) && !isRemoteControlTargetChangedError(e)) {
        setError(e.message);
      }
      return undefined;
    }
  }, [captureSessionListEpoch, currentAssistant, isSessionListCurrent, sessionMgr, setCurrentAssistant, setError]);

  const loadFirstPage = useCallback(async (
    workspacePath: string | undefined,
    query = '',
    identity?: { remoteConnectionId?: string; remoteSshHost?: string },
  ) => {
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return;
    const requestSeq = ++listRequestSeqRef.current;
    setLoading(true);
    offsetRef.current = 0;
    try {
      const resp = await sessionMgr.listSessions(
        workspacePath,
        listPageSize,
        0,
        query,
        identity,
      );
      if (
        requestSeq !== listRequestSeqRef.current
        || !isSessionListCurrent(targetEpoch)
      ) return;
      setSessions(resp.sessions);
      offsetRef.current = resp.sessions.length;
    } catch (e: any) {
      if (
        requestSeq !== listRequestSeqRef.current
        || !isSessionListCurrent(targetEpoch)
      ) return;
      if (!isRemoteControlTargetChangedError(e)) setError(e.message);
    } finally {
      if (
        requestSeq === listRequestSeqRef.current
        && isSessionListCurrent(targetEpoch)
      ) {
        setLoading(false);
      }
    }
  }, [captureSessionListEpoch, isSessionListCurrent, listPageSize, sessionMgr, setError, setSessions]);

  // Load workspace list for Expert-mode left-rail tree
  const loadWorkspaceList = useCallback(async () => {
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return;
    try {
      const workspaces = await sessionMgr.listRecentWorkspaces();
      if (!isSessionListCurrent(targetEpoch)) return;
      setWorkspaceList(workspaces);
    } catch (e: any) {
      if (isSessionListCurrent(targetEpoch) && !isRemoteControlTargetChangedError(e)) {
        setError(e.message);
      }
    }
  }, [captureSessionListEpoch, isSessionListCurrent, sessionMgr, setError]);

  const handleSelectWorkspace = useCallback(async (workspace: {
    path: string;
    name: string;
    remote_connection_id?: string;
    remote_ssh_host?: string;
  }) => {
    if (targetInitializingRef.current) return;
    if (sameWorkspaceIdentity(currentWorkspace, workspace)) return;
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return;
    try {
      const result = await sessionMgr.setWorkspace(workspace.path, {
        remoteConnectionId: workspace.remote_connection_id,
        remoteSshHost: workspace.remote_ssh_host,
      });
      if (!isSessionListCurrent(targetEpoch)) return;
      if (result.success) {
        const path = result.path || workspace.path;
        const remoteConnectionId =
          result.remote_connection_id ?? workspace.remote_connection_id;
        const remoteSshHost = result.remote_ssh_host ?? workspace.remote_ssh_host;
        const identity = { remoteConnectionId, remoteSshHost };
        setCurrentWorkspace({
          has_workspace: true,
          path,
          project_name: result.project_name || workspace.name,
          workspace_kind: remoteConnectionId || remoteSshHost
            ? 'remote'
            : undefined,
          remote_connection_id: remoteConnectionId,
          remote_ssh_host: remoteSshHost,
        });
        loadFirstPage(path, searchQuery, identity);
      } else {
        setError(result.error || 'Failed to set workspace');
      }
    } catch (e: any) {
      if (isSessionListCurrent(targetEpoch) && !isRemoteControlTargetChangedError(e)) {
        setError(e.message);
      }
    }
  }, [
    captureSessionListEpoch,
    currentWorkspace,
    isSessionListCurrent,
    loadFirstPage,
    searchQuery,
    sessionMgr,
    setCurrentWorkspace,
    setError,
  ]);

  const trySelectFirstProWorkspace = useCallback(async (): Promise<boolean> => {
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return false;
    try {
      const list = await sessionMgr.listRecentWorkspaces();
      if (!isSessionListCurrent(targetEpoch)) return false;
      const candidate = pickFirstProWorkspace(list);
      if (!candidate) return false;
      const result = await sessionMgr.setWorkspace(candidate.path, {
        remoteConnectionId: candidate.remote_connection_id,
        remoteSshHost: candidate.remote_ssh_host,
      });
      if (!isSessionListCurrent(targetEpoch)) return false;
      if (result.success) {
        const path = result.path || candidate.path;
        const remoteConnectionId =
          result.remote_connection_id ?? candidate.remote_connection_id;
        const remoteSshHost = result.remote_ssh_host ?? candidate.remote_ssh_host;
        const identity = { remoteConnectionId, remoteSshHost };
        setCurrentWorkspace({
          has_workspace: true,
          path,
          project_name: result.project_name || candidate.name,
          workspace_kind: remoteConnectionId || remoteSshHost
            ? 'remote'
            : candidate.workspace_kind,
          remote_connection_id: remoteConnectionId,
          remote_ssh_host: remoteSshHost,
        });
        setWorkspaceList(list);
        await loadFirstPage(path, searchQuery, identity);
        return isSessionListCurrent(targetEpoch);
      }
      setError(result.error || t('workspace.failedToSetWorkspace'));
      return false;
    } catch (e: any) {
      if (isSessionListCurrent(targetEpoch) && !isRemoteControlTargetChangedError(e)) {
        setError(e.message);
      }
      return false;
    }
  }, [captureSessionListEpoch, isSessionListCurrent, loadFirstPage, searchQuery, sessionMgr, setCurrentWorkspace, setError, t]);

  useEffect(() => {
    let cancelled = false;
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return;
    const isInitCurrent = () => (
      !cancelled && isSessionListCurrent(targetEpoch)
    );
    const init = async () => {
      try {
        const info = await sessionMgr.getWorkspaceInfo();
        if (!isInitCurrent()) return;
        if (info.workspace_kind === 'assistant' && info.path) {
          setCurrentAssistant({
            path: info.path,
            name: info.project_name ?? 'Claw',
            assistant_id: info.assistant_id,
          });
          setCurrentWorkspace(null);
          setDisplayMode('assistant');
          initLoadedPathRef.current = info.path;
          await loadFirstPage(info.path);
        } else {
          setDisplayMode('pro');
          const ws = info.has_workspace ? info : null;
          setCurrentWorkspace(ws);
          if (ws?.path) {
            initLoadedPathRef.current = ws.path;
            await loadWorkspaceList();
            if (!isInitCurrent()) return;
            await loadFirstPage(ws.path, '', {
              remoteConnectionId: ws.remote_connection_id,
              remoteSshHost: ws.remote_ssh_host,
            });
          } else {
            await trySelectFirstProWorkspace();
          }
        }
      } catch (e: any) {
        if (isInitCurrent() && !isRemoteControlTargetChangedError(e)) setError(e.message);
      } finally {
        if (isInitCurrent()) {
          setPairedDisplayMode(null);
          setLoading(false);
          targetInitializingRef.current = false;
          setTargetInitializing(false);
        }
      }
    };
    init();
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [controlTargetEpoch]);

  const refreshData = useCallback(async () => {
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return;
    const requestSeq = ++listRequestSeqRef.current;
    try {
      if (displayMode === 'pro') {
        const info = await sessionMgr.getWorkspaceInfo();
        if (
          requestSeq !== listRequestSeqRef.current
          || !isSessionListCurrent(targetEpoch)
        ) return;
        if (info.workspace_kind === 'assistant') {
          setCurrentWorkspace(null);
          setSessions([]);
          offsetRef.current = 0;
          return;
        }
        const ws = info.has_workspace ? info : null;
        setCurrentWorkspace(ws);
        try {
          const workspaces = await sessionMgr.listRecentWorkspaces();
          if (
            requestSeq !== listRequestSeqRef.current
            || !isSessionListCurrent(targetEpoch)
          ) return;
          setWorkspaceList(workspaces);
        } catch { /* keep prior workspace list */ }
        const resp = await sessionMgr.listSessions(ws?.path, listPageSize, 0, searchQuery, {
          remoteConnectionId: ws?.remote_connection_id,
          remoteSshHost: ws?.remote_ssh_host,
        });
        if (
          requestSeq !== listRequestSeqRef.current
          || !isSessionListCurrent(targetEpoch)
        ) return;
        setSessions(resp.sessions);
        offsetRef.current = resp.sessions.length;
      } else {
        const resp = await sessionMgr.listSessions(currentAssistant?.path, listPageSize, 0, searchQuery);
        if (
          requestSeq !== listRequestSeqRef.current
          || !isSessionListCurrent(targetEpoch)
        ) return;
        setSessions(resp.sessions);
        offsetRef.current = resp.sessions.length;
      }
    } catch { /* ignore */ }
    finally {
      if (
        requestSeq === listRequestSeqRef.current
        && isSessionListCurrent(targetEpoch)
      ) {
        setLoading(false);
      }
    }
  }, [captureSessionListEpoch, currentAssistant?.path, displayMode, isSessionListCurrent, listPageSize, searchQuery, sessionMgr, setCurrentWorkspace, setSessions]);

  useEffect(() => {
    const poll = setInterval(refreshData, 10000);
    return () => clearInterval(poll);
  }, [refreshData]);

  useEffect(() => {
    const workspacePath = displayMode === 'assistant' ? currentAssistant?.path : currentWorkspace?.path;
    if (!workspacePath) return;
    // Skip the redundant first load when init() already loaded this path —
    // otherwise the state change from init() triggers a second loadFirstPage
    // 250 ms later, causing an extra network round-trip and a loading flicker.
    if (initLoadedPathRef.current === workspacePath) {
      initLoadedPathRef.current = undefined;
      return;
    }
    const identity = displayMode === 'assistant'
      ? undefined
      : {
          remoteConnectionId: currentWorkspace?.remote_connection_id,
          remoteSshHost: currentWorkspace?.remote_ssh_host,
        };
    const timer = setTimeout(() => {
      loadFirstPage(workspacePath, searchQuery, identity);
    }, 250);
    return () => clearTimeout(timer);
  }, [
    currentAssistant?.path,
    currentWorkspace?.path,
    currentWorkspace?.remote_connection_id,
    currentWorkspace?.remote_ssh_host,
    displayMode,
    loadFirstPage,
    searchQuery,
  ]);

  const handleCreate = useCallback(async (agentType: string) => {
    if (creating || targetInitializingRef.current) return;
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return;
    setCreating(true);
    try {
      // For assistant mode (Claw), use currentAssistant.path
      // For pro mode (Code/Cowork), use currentWorkspace.path
      const workspacePath = displayMode === 'assistant' ? currentAssistant?.path : currentWorkspace?.path;
      const identity = displayMode === 'assistant'
        ? undefined
        : {
            remoteConnectionId: currentWorkspace?.remote_connection_id,
            remoteSshHost: currentWorkspace?.remote_ssh_host,
      };
      const id = await sessionMgr.createSession(agentType, undefined, workspacePath, identity);
      if (!isSessionListCurrent(targetEpoch)) return;
      await loadFirstPage(workspacePath, searchQuery, identity);
      if (!isSessionListCurrent(targetEpoch)) return;
      const label = isClawAgent(agentType)
        ? t('sessions.remoteClawSession')
        : isCoworkAgent(agentType)
          ? t('sessions.remoteCoworkSession')
          : t('sessions.remoteCodeSession');
      onSelectSession(id, label, true);
    } catch (e: any) {
      if (isSessionListCurrent(targetEpoch) && !isRemoteControlTargetChangedError(e)) {
        setError(e.message);
      }
    } finally {
      if (isSessionListCurrent(targetEpoch)) setCreating(false);
    }
  }, [
    creating,
    captureSessionListEpoch,
    currentWorkspace?.path,
    currentWorkspace?.remote_connection_id,
    currentWorkspace?.remote_ssh_host,
    currentAssistant?.path,
    displayMode,
    isSessionListCurrent,
    loadFirstPage,
    onSelectSession,
    searchQuery,
    sessionMgr,
    setError,
    t,
  ]);

  const handleSelectMode = useCallback(async (mode: DisplayMode) => {
    if (targetInitializingRef.current) return;
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return;
    setDisplayMode(mode);
    setShowAssistantPicker(false);
    if (mode === 'assistant') {
      const assistantPath = await loadAssistantList();
      if (!isSessionListCurrent(targetEpoch)) return;
      loadFirstPage(assistantPath, searchQuery);
    } else {
      await loadWorkspaceList();
      if (!isSessionListCurrent(targetEpoch)) return;
      if (currentWorkspace?.path) {
        await loadFirstPage(currentWorkspace.path, searchQuery, {
          remoteConnectionId: currentWorkspace.remote_connection_id,
          remoteSshHost: currentWorkspace.remote_ssh_host,
        });
      } else {
        await trySelectFirstProWorkspace();
      }
    }
  }, [
    captureSessionListEpoch,
    currentWorkspace?.path,
    currentWorkspace?.remote_connection_id,
    currentWorkspace?.remote_ssh_host,
    isSessionListCurrent,
    loadAssistantList,
    loadFirstPage,
    loadWorkspaceList,
    searchQuery,
    trySelectFirstProWorkspace,
  ]);

  const handleSelectAssistant = useCallback(async (assistant: { path: string; name: string; assistant_id?: string }) => {
    if (targetInitializingRef.current) return;
    const targetEpoch = captureSessionListEpoch();
    if (targetEpoch === null) return;
    try {
      await sessionMgr.setAssistant(assistant.path);
      if (!isSessionListCurrent(targetEpoch)) return;
      setCurrentAssistant(assistant);
      setShowAssistantPicker(false);
      loadFirstPage(assistant.path, searchQuery);
    } catch (e: any) {
      if (isSessionListCurrent(targetEpoch) && !isRemoteControlTargetChangedError(e)) {
        setError(e.message);
      }
    }
  }, [captureSessionListEpoch, isSessionListCurrent, loadFirstPage, searchQuery, sessionMgr, setCurrentAssistant, setError]);

  const assistantDisplayName = currentAssistant?.name || t('shared.agents.default');
  const isProMode = displayMode === 'pro';
  const leftWorkspaces = isProMode
    ? visibleProWorkspaces(workspaceList, currentWorkspace, VR_WORKSPACE_CAP)
    : [];
  const nestedSessionCap = isProMode
    ? Math.max(1, VR_LEFT_ROW_CAP - leftWorkspaces.length)
    : VR_SESSION_CAP;
  const visibleSessions = sessions.slice(0, nestedSessionCap);

  const renderSessionCard = (s: SessionInfo, nested = false) => (
    <div
      key={s.session_id}
      className={[
        'session-list__item',
        nested ? 'session-list__item--nested' : '',
        menuSession?.session_id === s.session_id ? 'session-list__item--active' : '',
        selectedSessionId === s.session_id ? 'session-list__item--selected' : '',
      ].filter(Boolean).join(' ')}
      onClick={(e) => handleSessionClick(s, e)}
      onTouchStart={(e) => handleSessionTouchStart(s, e)}
      onTouchMove={handleSessionTouchMove}
      onTouchEnd={handleSessionTouchEnd}
      onTouchCancel={handleSessionTouchEnd}
      onContextMenu={(e) => { e.preventDefault(); setMenuSession(s); }}
    >
      <div className={`session-list__item-icon session-list__item-icon--${s.agent_type}`}>
        <SessionTypeIcon agentType={s.agent_type} />
      </div>
      <div className="session-list__item-body">
        <div className="session-list__item-top">
          <div className="session-list__item-name">{s.name || t('sessions.untitledSession')}</div>
          <span className={`session-list__agent-badge session-list__agent-badge--${s.agent_type}`}>
            {agentLabel(s.agent_type, t)}
          </span>
        </div>
        <div className="session-list__item-time">{formatTime(s.updated_at, formatDate, t)}</div>
      </div>
    </div>
  );

  // Re-render after HUD hosts mount so portals can attach.
  const [, setVrHostTick] = useState(0);
  useLayoutEffect(() => {
    setVrHostTick((n) => n + 1);
  }, [vrHosts, vrHosts.sessions.current, vrHosts.controls.current]);

  const sessionsPane = (
    <div className="session-list__vr-sessions">
      <div className="session-list__section-head">
        <div className="session-list__section-title">
          {isProMode ? t('shared.features.workspace') : t('sessions.sessionHistory')}
        </div>
        <div className="session-list__section-meta">
          {t('common.itemCount', {
            count: isProMode ? leftWorkspaces.length : visibleSessions.length,
          })}
        </div>
      </div>
      {isProMode ? (
        <>
          {(loading || targetInitializing) && leftWorkspaces.length === 0 && (
            <div className="session-list__empty">{t('sessions.loadingSessions')}</div>
          )}
          {!loading && !targetInitializing && leftWorkspaces.length === 0 && (
            <div className="session-list__empty">{t('sessions.noWorkspaces')}</div>
          )}
          <div className="session-list__tree">
            {leftWorkspaces.map((workspace, index) => {
              const expanded = sameWorkspaceIdentity(currentWorkspace, workspace);
              const itemKey = workspaceIdentityKey(workspace) || String(index);
              return (
                <div key={itemKey} className="session-list__workspace-group">
                  <button
                    type="button"
                    className={[
                      'session-list__workspace-row',
                      expanded ? 'session-list__workspace-row--selected' : '',
                    ].filter(Boolean).join(' ')}
                    onClick={() => handleSelectWorkspace(workspace)}
                    disabled={targetInitializing}
                    title={workspace.name}
                  >
                    <span className="session-list__workspace-row-icon">
                      <WorkspaceIcon />
                    </span>
                    <span className="session-list__workspace-row-name">{workspace.name}</span>
                  </button>
                  {expanded && (
                    <div className="session-list__workspace-sessions">
                      {(loading || targetInitializing) && visibleSessions.length === 0 && (
                        <div className="session-list__empty session-list__empty--nested">
                          {t('sessions.loadingSessions')}
                        </div>
                      )}
                      {!loading && !targetInitializing && visibleSessions.length === 0 && (
                        <div className="session-list__empty session-list__empty--nested">
                          {t('sessions.noSessions')}
                        </div>
                      )}
                      {visibleSessions.map((s) => renderSessionCard(s, true))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </>
      ) : (
        <>
          {(loading || targetInitializing) && visibleSessions.length === 0 && (
            <div className="session-list__empty">{t('sessions.loadingSessions')}</div>
          )}
          {!loading && !targetInitializing && visibleSessions.length === 0 && (
            <div className="session-list__empty">{t('sessions.noSessions')}</div>
          )}
          <div className="session-list__cards">
            {visibleSessions.map((s) => renderSessionCard(s))}
          </div>
        </>
      )}
    </div>
  );

  const controlsPane = (
    <div className="session-list__vr-controls">
      <div className="session-list__mode-toggle">
        <button
          className={`session-list__mode-toggle-btn ${isProMode ? 'is-active' : ''}`}
          onClick={() => handleSelectMode('pro')}
          disabled={targetInitializing}
        >
          <ProModeIcon />
          <span>{t('shared.modes.expert')}</span>
        </button>
        <button
          className={`session-list__mode-toggle-btn ${!isProMode ? 'is-active' : ''}`}
          onClick={() => handleSelectMode('assistant')}
          disabled={targetInitializing}
        >
          <AssistantModeIcon />
          <span>{t('shared.modes.assistant')}</span>
        </button>
      </div>

      {!isProMode && (
        <div
          className="session-list__assistant-bar"
          onClick={() => {
            if (targetInitializingRef.current) return;
            loadAssistantList();
            setShowAssistantPicker(true);
          }}
        >
          <span className="session-list__assistant-icon">
            <AssistantModeIcon />
          </span>
          <div className="session-list__assistant-copy">
            <span className="session-list__assistant-label">{t('sessions.assistant')}</span>
            <span className="session-list__assistant-name">{assistantDisplayName}</span>
          </div>
        </div>
      )}

      <div className="session-list__create-row session-list__create-row--compact">
        {isProMode ? (
          <>
            <button
              className="session-list__create-btn session-list__create-btn--code"
              onClick={() => handleCreate('code')}
              disabled={creating || targetInitializing || !currentWorkspace}
            >
              <div className="session-list__create-icon">
                <SessionTypeIcon agentType="code" />
              </div>
              <span className="session-list__create-title">{t('shared.agents.code')}</span>
            </button>
            <button
              className="session-list__create-btn session-list__create-btn--cowork"
              onClick={() => handleCreate('cowork')}
              disabled={creating || targetInitializing || !currentWorkspace}
            >
              <div className="session-list__create-icon">
                <SessionTypeIcon agentType="cowork" />
              </div>
              <span className="session-list__create-title">{t('shared.agents.cowork')}</span>
            </button>
          </>
        ) : (
          <button
            className="session-list__create-btn session-list__create-btn--claw"
            onClick={() => handleCreate('claw')}
            disabled={creating || targetInitializing}
          >
            <div className="session-list__create-icon">
              <SessionTypeIcon agentType="claw" />
            </div>
            <span className="session-list__create-title">{t('sessions.clawSession')}</span>
          </button>
        )}
      </div>
    </div>
  );

  const pickerOverlays = (
    <>
      {showAssistantPicker && (
        <div className="session-list__picker-overlay" onClick={() => setShowAssistantPicker(false)}>
          <div className="session-list__picker-modal" onClick={(e) => e.stopPropagation()}>
            <div className="session-list__picker-header">
              <h3>{t('sessions.selectAssistant')}</h3>
              <button className="session-list__picker-close" onClick={() => setShowAssistantPicker(false)}>
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
              </button>
            </div>
            <div className="session-list__picker-list">
              {assistantList.map((assistant, index) => (
                <button
                  key={assistant.path || index}
                  className={`session-list__picker-item ${currentAssistant?.path === assistant.path ? 'is-selected' : ''}`}
                  onClick={() => handleSelectAssistant(assistant)}
                >
                  <span className="session-list__picker-item-icon">
                    <AssistantModeIcon />
                  </span>
                  <span className="session-list__picker-item-name">{assistant.name}</span>
                  {currentAssistant?.path === assistant.path && (
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                  )}
                </button>
              ))}
            </div>
          </div>
        </div>
      )}
    </>
  );

  const sessionsHost = vrHosts.sessions.current;
  const controlsHost = vrHosts.controls.current;
  return (
    <>
      {sessionsHost ? createPortal(sessionsPane, sessionsHost) : null}
      {controlsHost ? createPortal(controlsPane, controlsHost) : null}
      {pickerOverlays}
      {/* Context Menu Bottom Sheet */}
      {menuSession && !renameTarget && !deleteConfirmTarget && (
        <div className="session-list__menu-overlay" onClick={() => setMenuSession(null)}>
          <div className="session-list__menu-sheet" onClick={(e) => e.stopPropagation()}>
            <div className="session-list__menu-handle" />
            <div className="session-list__menu-title">
              {menuSession.name || t('sessions.untitledSession')}
            </div>
            <div className="session-list__menu-actions">
              <button
                className="session-list__menu-btn"
                onClick={() => {
                  setRenameTarget(menuSession);
                  setRenameValue(menuSession.name || '');
                }}
              >
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
                  <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
                </svg>
                <span>{t('sessions.renameSession')}</span>
              </button>
              <button
                className="session-list__menu-btn session-list__menu-btn--danger"
                onClick={() => setDeleteConfirmTarget(menuSession)}
              >
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="3 6 5 6 21 6" />
                  <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                </svg>
                <span>{t('sessions.deleteSession')}</span>
              </button>
            </div>
            <button className="session-list__menu-cancel" onClick={() => setMenuSession(null)}>
              {t('sessions.cancel')}
            </button>
          </div>
        </div>
      )}

      {/* Rename Modal */}
      {renameTarget && (
        <div className="session-list__picker-overlay" onClick={() => !renaming && setRenameTarget(null)}>
          <div className="session-list__rename-modal" onClick={(e) => e.stopPropagation()}>
            <h3 className="session-list__rename-title">{t('sessions.renameTitle')}</h3>
            <input
              className="session-list__rename-input"
              type="text"
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              placeholder={t('sessions.sessionNamePlaceholder')}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleRename();
                if (e.key === 'Escape') setRenameTarget(null);
              }}
            />
            <div className="session-list__rename-actions">
              <button
                className="session-list__rename-btn session-list__rename-btn--cancel"
                onClick={() => setRenameTarget(null)}
                disabled={renaming}
              >
                {t('sessions.cancel')}
              </button>
              <button
                className="session-list__rename-btn session-list__rename-btn--save"
                onClick={handleRename}
                disabled={renaming || !renameValue.trim()}
              >
                {renaming ? '...' : t('sessions.save')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Delete Confirmation */}
      {deleteConfirmTarget && (
        <div className="session-list__picker-overlay" onClick={() => !deleting && setDeleteConfirmTarget(null)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') setDeleteConfirmTarget(null);
            if (e.key === 'Enter' && !deleting) handleDelete();
          }}>
          <div className="session-list__confirm-modal" onClick={(e) => e.stopPropagation()}>
            <div className="session-list__confirm-icon">
              <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="10" />
                <line x1="12" y1="8" x2="12" y2="12" />
                <line x1="12" y1="16" x2="12.01" y2="16" />
              </svg>
            </div>
            <h3 className="session-list__confirm-title">{t('sessions.confirmDelete')}</h3>
            <p className="session-list__confirm-desc">
              "{deleteConfirmTarget.name || t('sessions.untitledSession')}"
              <br />
              {t('sessions.confirmDeleteDesc')}
            </p>
            <div className="session-list__confirm-actions">
              <button
                className="session-list__confirm-btn session-list__confirm-btn--cancel"
                onClick={() => setDeleteConfirmTarget(null)}
                disabled={deleting}
              >
                {t('sessions.cancel')}
              </button>
              <button
                className="session-list__confirm-btn session-list__confirm-btn--danger"
                onClick={handleDelete}
                disabled={deleting}
              >
                {deleting ? '...' : t('sessions.deleteSession')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Action Toast */}
      {actionToast && (
        <div className="session-list__toast" role="alert" aria-live="assertive">{actionToast}</div>
      )}
    </>
  );
};

export default SessionListPage;
