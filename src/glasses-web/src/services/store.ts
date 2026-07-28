import { create } from 'zustand';
import type {
  SessionInfo,
  ChatMessage,
  WorkspaceInfo,
  ActiveTurnSnapshot,
  AssistantEntry,
} from './RemoteSessionManager';

type ConnectionStatus = 'idle' | 'pairing' | 'paired' | 'error';
export type ConnectionHealth = 'unpaired' | 'checking' | 'connected' | 'unreachable';

interface MobileStore {
  connectionStatus: ConnectionStatus;
  setConnectionStatus: (s: ConnectionStatus) => void;
  connectionHealth: ConnectionHealth;
  setConnectionHealth: (h: ConnectionHealth) => void;

  currentWorkspace: WorkspaceInfo | null;
  setCurrentWorkspace: (w: WorkspaceInfo | null) => void;

  currentAssistant: AssistantEntry | null;
  setCurrentAssistant: (a: AssistantEntry | null) => void;

  /** One-shot hint after pairing so SessionList matches desktop assistant vs project workspace. */
  pairedDisplayMode: 'pro' | 'assistant' | null;
  setPairedDisplayMode: (m: 'pro' | 'assistant' | null) => void;

  authenticatedUserId: string | null;
  setAuthenticatedUserId: (userId: string | null) => void;

  /**
   * Current same-account control target (delegated identity flow).
   * `isHome` marks the QR-paired desktop this mobile session started from.
   */
  controlTarget: { deviceId: string; deviceName: string | null; isHome: boolean } | null;
  setControlTarget: (
    target: { deviceId: string; deviceName: string | null; isHome: boolean } | null,
  ) => void;

  sessions: SessionInfo[];
  setSessions: (s: SessionInfo[]) => void;
  updateSessionName: (sessionId: string, name: string) => void;
  removeSession: (sessionId: string) => void;

  activeSessionId: string | null;
  setActiveSessionId: (id: string | null) => void;

  messagesBySession: Record<string, ChatMessage[]>;
  deletedMessageIds: Record<string, Set<string>>;
  getMessages: (sessionId: string) => ChatMessage[];
  setMessages: (sessionId: string, m: ChatMessage[]) => void;
  appendNewMessages: (sessionId: string, messages: ChatMessage[]) => void;
  deleteMessage: (sessionId: string, messageId: string) => void;

  activeTurn: ActiveTurnSnapshot | null;
  setActiveTurn: (t: ActiveTurnSnapshot | null) => void;

  error: string | null;
  setError: (e: string | null) => void;

  resetConnectionState: () => void;
  /** Clear per-device UI state when switching the control target device. */
  resetForDeviceSwitch: () => void;
}

export const useMobileStore = create<MobileStore>((set, get) => ({
  connectionStatus: 'idle',
  setConnectionStatus: (connectionStatus) => set({ connectionStatus }),
  connectionHealth: 'unpaired',
  setConnectionHealth: (connectionHealth) => set({ connectionHealth }),

  currentWorkspace: null,
  setCurrentWorkspace: (currentWorkspace) => set({ currentWorkspace }),

  currentAssistant: null,
  setCurrentAssistant: (currentAssistant) => set({ currentAssistant }),

  pairedDisplayMode: null,
  setPairedDisplayMode: (pairedDisplayMode) => set({ pairedDisplayMode }),

  authenticatedUserId: null,
  setAuthenticatedUserId: (authenticatedUserId) => set({ authenticatedUserId }),

  controlTarget: null,
  setControlTarget: (controlTarget) => set({ controlTarget }),

  sessions: [],
  setSessions: (sessions) => set({ sessions }),
  updateSessionName: (sessionId, name) =>
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.session_id === sessionId ? { ...s, name } : s,
      ),
    })),
  removeSession: (sessionId) =>
    set((state) => {
      const { [sessionId]: _, ...rest } = state.messagesBySession;
      return {
        sessions: state.sessions.filter((s) => s.session_id !== sessionId),
        messagesBySession: rest,
      };
    }),

  activeSessionId: null,
  setActiveSessionId: (activeSessionId) => set({ activeSessionId }),

  messagesBySession: {},
  deletedMessageIds: {},
  getMessages: (sessionId: string) => {
    const msgs = get().messagesBySession[sessionId] || [];
    const deleted = get().deletedMessageIds[sessionId];
    return deleted ? msgs.filter((m) => !deleted.has(m.id)) : msgs;
  },
  setMessages: (sessionId, m) =>
    set((s) => {
      const deleted = s.deletedMessageIds[sessionId];
      const filtered = deleted ? m.filter((msg) => !deleted.has(msg.id)) : m;
      return { messagesBySession: { ...s.messagesBySession, [sessionId]: filtered } };
    }),
  appendNewMessages: (sessionId, messages) =>
    set((s) => {
      if (messages.length === 0) return s;
      const prev = s.messagesBySession[sessionId] || [];
      const deleted = s.deletedMessageIds[sessionId];
      const byId = new Map(prev.map((m) => [m.id, m]));
      let changed = false;
      for (const incoming of messages) {
        if (deleted?.has(incoming.id)) continue;
        const existing = byId.get(incoming.id);
        if (!existing) {
          byId.set(incoming.id, incoming);
          changed = true;
          continue;
        }
        // Same id can arrive again with a longer body after turn-end persist.
        const prevLen = (existing.content?.length ?? 0)
          + (existing.thinking?.length ?? 0)
          + (existing.items?.length ?? 0);
        const nextLen = (incoming.content?.length ?? 0)
          + (incoming.thinking?.length ?? 0)
          + (incoming.items?.length ?? 0);
        if (nextLen > prevLen) {
          byId.set(incoming.id, { ...existing, ...incoming });
          changed = true;
        }
      }
      if (!changed) return s;
      const merged = prev.map((m) => byId.get(m.id) ?? m);
      for (const incoming of messages) {
        if (deleted?.has(incoming.id)) continue;
        if (!prev.some((m) => m.id === incoming.id)) {
          merged.push(byId.get(incoming.id)!);
        }
      }
      return {
        messagesBySession: {
          ...s.messagesBySession,
          [sessionId]: merged,
        },
      };
    }),
  deleteMessage: (sessionId, messageId) =>
    set((s) => {
      const prev = s.messagesBySession[sessionId];
      const deleted = new Set(s.deletedMessageIds[sessionId] || []);
      deleted.add(messageId);
      return {
        deletedMessageIds: { ...s.deletedMessageIds, [sessionId]: deleted },
        messagesBySession: prev
          ? { ...s.messagesBySession, [sessionId]: prev.filter((m) => m.id !== messageId) }
          : s.messagesBySession,
      };
    }),

  activeTurn: null,
  setActiveTurn: (activeTurn) => set({ activeTurn }),

  error: null,
  setError: (error) => set({ error }),

  resetConnectionState: () =>
    set({
      connectionStatus: 'idle',
      currentWorkspace: null,
      currentAssistant: null,
      pairedDisplayMode: null,
      authenticatedUserId: null,
      controlTarget: null,
      sessions: [],
      activeSessionId: null,
      messagesBySession: {},
      deletedMessageIds: {},
      activeTurn: null,
      error: null,
    }),

  resetForDeviceSwitch: () =>
    set({
      currentWorkspace: null,
      currentAssistant: null,
      pairedDisplayMode: null,
      sessions: [],
      activeSessionId: null,
      messagesBySession: {},
      deletedMessageIds: {},
      activeTurn: null,
      error: null,
    }),
}));
