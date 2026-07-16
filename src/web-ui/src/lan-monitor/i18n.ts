import { formatDateForLocale } from '@/infrastructure/i18n/core/localeFormatting';
import { SHARED_TERMS_BY_LOCALE } from '@/infrastructure/i18n/presets/generatedLocaleContract';
import enUS from './locales/en-US.json';
import zhCN from './locales/zh-CN.json';
import zhTW from './locales/zh-TW.json';

type Messages = Record<string, string>;

const resources: Record<string, Messages> = {
  'en-US': { ...enUS, workspace: SHARED_TERMS_BY_LOCALE['en-US'].features.workspace },
  'zh-CN': { ...zhCN, workspace: SHARED_TERMS_BY_LOCALE['zh-CN'].features.workspace },
  'zh-TW': { ...zhTW, workspace: SHARED_TERMS_BY_LOCALE['zh-TW'].features.workspace },
};

function currentLocale(): keyof typeof resources {
  const language = navigator.language;
  if (/^zh-(TW|HK|MO|Hant)/i.test(language)) return 'zh-TW';
  if (/^en/i.test(language)) return 'en-US';
  return 'zh-CN';
}

export function translate(key: string, values?: Record<string, string | number>): string {
  const messages = resources[currentLocale()] ?? resources['zh-CN'];
  let message = messages[key] ?? resources['en-US'][key] ?? key;
  for (const [name, value] of Object.entries(values ?? {})) {
    message = message.split(`{{${name}}}`).join(String(value));
  }
  return message;
}

export function formatTimestamp(timestamp: number): string {
  return formatDateForLocale(currentLocale(), timestamp, {
    hour: '2-digit',
    minute: '2-digit',
  });
}
