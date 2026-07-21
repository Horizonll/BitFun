import { useState, type CSSProperties } from 'react';
import { Check, ShieldAlert, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@/component-library';
import type {
  PermissionReplyKind,
  PermissionRequest,
} from '@/infrastructure/api/service-api/AgentAPI';
import { useChatInputState } from '../../store/chatInputStateStore';
import { CHAT_INPUT_DROP_ZONE_BOTTOM_PX } from '../../utils/flowChatScrollLayout';
import './PermissionRequestPanel.scss';

const PERMISSION_PANEL_INPUT_GAP_PX = 16;

interface PermissionRequestPanelProps {
  requests: PermissionRequest[];
  onRespond: (requestId: string, reply: PermissionReplyKind, feedback?: string) => Promise<void>;
  onRespondBatch: (requestId: string, reply: PermissionReplyKind, feedback?: string) => Promise<void>;
  aboveChatInput?: boolean;
}

const PERMISSION_ACTION_LABEL_KEYS: Record<string, string> = {
  read: 'permission.actions.read',
  edit: 'permission.actions.edit',
  bash: 'permission.actions.bash',
  git: 'permission.actions.git',
  computer_use: 'permission.actions.computerUse',
  websearch: 'permission.actions.webSearch',
  webfetch: 'permission.actions.webFetch',
  mcp: 'permission.actions.mcp',
  task: 'permission.actions.task',
  skill: 'permission.actions.skill',
  custom_tool: 'permission.actions.customTool',
  external_directory: 'permission.actions.externalDirectory',
};

function permissionActionLabel(action: string, t: (key: string) => string): string {
  return t(PERMISSION_ACTION_LABEL_KEYS[action] ?? 'permission.actions.other');
}

export function PermissionRequestPanel({
  requests,
  onRespond,
  onRespondBatch,
  aboveChatInput = false,
}: PermissionRequestPanelProps) {
  const { t } = useTranslation('flow-chat');
  const [feedback, setFeedback] = useState('');
  const [responding, setResponding] = useState(false);
  const [error, setError] = useState(false);
  const inputHeight = useChatInputState((state) => state.inputHeight);
  const request = requests[0];
  const risk = [request?.displayMetadata?.riskDescription, request?.displayMetadata?.risk].find(
    (value): value is string => typeof value === 'string' && value.trim().length > 0,
  );

  const alwaysAllowTooltip = request?.saveResources?.length
    ? request.projectPath?.trim()
      ? t('permission.allowAlwaysTooltip', { projectPath: request.projectPath.trim() })
      : t('permission.allowAlwaysTooltipCurrentProject')
    : t('permission.allowAlwaysTooltipNoGrant');

  const panelStyle = aboveChatInput && inputHeight > 0
    ? {
        '--permission-request-panel-bottom': `${
          inputHeight + CHAT_INPUT_DROP_ZONE_BOTTOM_PX + PERMISSION_PANEL_INPUT_GAP_PX
        }px`,
      } as CSSProperties
    : undefined;

  const respond = async (reply: PermissionReplyKind) => {
    setResponding(true);
    setError(false);
    try {
      await onRespond(request.requestId, reply, reply === 'reject' ? feedback : undefined);
    } catch {
      setError(true);
    } finally {
      setResponding(false);
    }
  };

  const respondBatch = async (reply: PermissionReplyKind) => {
    setResponding(true);
    setError(false);
    try {
      await onRespondBatch(request.requestId, reply, reply === 'reject' ? feedback : undefined);
    } catch {
      setError(true);
    } finally {
      setResponding(false);
    }
  };

  if (!request) return null;

  return (
    <section
      className={`permission-request-panel${aboveChatInput ? ' permission-request-panel--above-chat-input' : ''}`}
      style={panelStyle}
      aria-label={t('permission.title')}
    >
      <div className="permission-request-panel__heading">
        <div className="permission-request-panel__heading-title">
          <ShieldAlert size={18} aria-hidden="true" />
          <h2>{t('permission.title')}</h2>
        </div>
        <span className="permission-request-panel__count">
          {t('permission.batchCount', { count: requests.length })}
        </span>
      </div>
      <div className="permission-request-panel__requests" role="list">
        {requests.map((item, index) => (
          <div
            className={`permission-request-panel__request${index === 0 ? ' permission-request-panel__request--active' : ''}`}
            key={item.requestId}
            role="listitem"
          >
            <div className="permission-request-panel__request-heading">
              <div className="permission-request-panel__tool-identity">
                <strong>{item.source.identity}</strong>
                {item.delegation && (
                  <span className="permission-request-panel__subagent">
                    {t('permission.subagentOwner', { subagent: item.delegation.subagentType })}
                  </span>
                )}
              </div>
              <span>{index === 0 ? t('permission.current') : t('permission.pending')}</span>
            </div>
            <div className="permission-request-panel__request-details">
              <span className="permission-request-panel__action">
                {permissionActionLabel(item.action, t)}
              </span>
              <span className="permission-request-panel__detail-separator" aria-hidden="true">·</span>
              <Tooltip content={item.resources.join(', ')} placement="top">
                <code className="permission-request-panel__resource-summary">
                  {item.resources.join(', ')}
                </code>
              </Tooltip>
            </div>
          </div>
        ))}
      </div>
      {risk && <p className="permission-request-panel__risk">{risk}</p>}
      {error && <p role="alert">{t('permission.responseFailed')}</p>}
      <textarea
        value={feedback}
        onChange={(event) => setFeedback(event.target.value)}
        placeholder={t('permission.feedbackPlaceholder')}
        aria-label={t('permission.feedbackLabel')}
        disabled={responding}
        rows={2}
      />
      <div className="permission-request-panel__actions">
        <div className="permission-request-panel__single-actions">
          <button type="button" onClick={() => void respond('once')} disabled={responding}>
            <Check size={15} aria-hidden="true" /> {t('permission.allowOnce')}
          </button>
          <Tooltip content={alwaysAllowTooltip} placement="top">
            <button type="button" onClick={() => void respond('always')} disabled={responding}>
              <Check size={15} aria-hidden="true" /> {t('permission.allowAlways')}
            </button>
          </Tooltip>
          <button
            type="button"
            className="permission-request-panel__reject"
            onClick={() => void respond('reject')}
            disabled={responding}
          >
            <X size={15} aria-hidden="true" /> {t('permission.reject')}
          </button>
        </div>
        {requests.length > 1 && (
          <div className="permission-request-panel__batch-actions">
          <button type="button" onClick={() => void respondBatch('once')} disabled={responding}>
            <Check size={15} aria-hidden="true" /> {t('permission.allowCurrentAndFollowing')}
          </button>
          <button
            type="button"
            className="permission-request-panel__reject"
            onClick={() => void respondBatch('reject')}
            disabled={responding}
          >
            <X size={15} aria-hidden="true" /> {t('permission.rejectCurrentAndFollowing')}
          </button>
          </div>
        )}
      </div>
    </section>
  );
}
