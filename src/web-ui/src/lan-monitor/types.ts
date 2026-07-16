export interface WorkspaceFacts {
  path: string;
  name: string;
  git_branch?: string | null;
  kind: string;
  assistant_id?: string | null;
}

export interface SessionInfo {
  session_id: string;
  name: string;
  agent_type: string;
  created_at: string;
  updated_at: string;
  message_count: number;
  workspace_path?: string | null;
  workspace_name?: string | null;
  state?: string | null;
  active_turn_id?: string | null;
  queue_depth?: number | null;
}

export interface ChatImageAttachment {
  name: string;
  data_url: string;
}

export interface UserMessage {
  id: string;
  content: string;
  timestamp: number;
  images?: ChatImageAttachment[];
}

export interface TextItem {
  type: 'text';
  id: string;
  content: string;
  isMarkdown: boolean;
  timestamp: number;
  status?: string;
  orderIndex?: number;
  subagentSessionId?: string;
  parentTaskToolId?: string;
}

export interface ThinkingItem {
  type: 'thinking';
  id: string;
  content: string;
  isCollapsed: boolean;
  timestamp: number;
  status?: string;
  orderIndex?: number;
  subagentSessionId?: string;
  parentTaskToolId?: string;
}

export interface ToolItem {
  type: 'tool';
  id: string;
  name: string;
  status: string;
  input: unknown;
  result?: unknown;
  success?: boolean;
  error?: string;
  startTime: number;
  endTime?: number;
  durationMs?: number;
  orderIndex?: number;
  subagentSessionId?: string;
  subagentDialogTurnId?: string;
  parentTaskToolId?: string;
  subagentModelId?: string;
  subagentModelDisplayName?: string;
  resultTruncated: boolean;
  resultRef?: string;
}

export type MonitorItem = TextItem | ThinkingItem | ToolItem;

export interface MonitorRound {
  id: string;
  roundIndex: number;
  status: string;
  startTime: number;
  endTime?: number;
  durationMs?: number;
  modelId?: string;
  modelAlias?: string;
  items: MonitorItem[];
}

export interface MonitorTurn {
  turnId: string;
  turnIndex: number;
  kind: string;
  status: string;
  timestamp: number;
  startTime: number;
  endTime?: number;
  durationMs?: number;
  finishReason?: string;
  userMessage: UserMessage;
  rounds: MonitorRound[];
}

export interface TranscriptPage {
  sessionId: string;
  turns: MonitorTurn[];
  totalTurnCount: number;
  hasMore: boolean;
  nextBeforeTurnId?: string;
}

export interface ActiveTool {
  id: string;
  name: string;
  status: string;
  input?: unknown;
  durationMs?: number;
  startMs?: number;
}

export interface ActiveMessageItem {
  type: 'text' | 'thinking' | 'tool';
  content?: string;
  tool?: {
    id: string;
    name: string;
    status: string;
    duration_ms?: number;
    start_ms?: number;
  };
  is_subagent?: boolean;
}

export interface ActiveTurn {
  turnId: string;
  status: string;
  roundIndex: number;
  text: string;
  thinking: string;
  tools: ActiveTool[];
  items: ActiveMessageItem[];
}

export interface PollSnapshot {
  version: number;
  changed: boolean;
  sessionState?: string;
  title?: string;
  activeTurn?: ActiveTurn;
  transcriptChanged: boolean;
  totalTurnCount?: number;
}

export interface ToolResultChunk {
  resultRef: string;
  cursor: number;
  chunk: string;
  nextCursor?: number;
  hasMore: boolean;
}

export type LanMonitorResponse =
  | { resp: 'lan_monitor_workspace'; workspace: WorkspaceFacts | null }
  | { resp: 'lan_monitor_sessions'; sessions: SessionInfo[]; has_more: boolean }
  | { resp: 'lan_monitor_transcript'; page: TranscriptPage }
  | { resp: 'lan_monitor_tool_result'; result: ToolResultChunk }
  | { resp: 'lan_monitor_poll'; snapshot: PollSnapshot }
  | { resp: 'lan_monitor_action_accepted'; action: string; target_id: string }
  | { resp: 'pong' }
  | { resp: 'error'; message: string };
