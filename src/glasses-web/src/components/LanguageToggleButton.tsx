import React from 'react';
import { useI18n } from '../i18n';
import { getMobileLanguageShortName } from '../i18n/localeRegistry';

interface LanguageToggleButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  className?: string;
}

const LanguageToggleButton: React.FC<LanguageToggleButtonProps> = ({
  className,
  type = 'button',
  ...rest
}) => {
  const { language, toggleLanguage, t } = useI18n();

  return (
    <button
      type={type}
      className={className || 'mobile-lang-btn'}
      aria-label={t('common.switchLanguage')}
      title={t('common.switchLanguage')}
      {...rest}
      onClick={toggleLanguage}
    >
      {getMobileLanguageShortName(language)}
    </button>
  );
};

export default LanguageToggleButton;
