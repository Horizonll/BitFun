import React, { useLayoutEffect } from 'react';
import { darkTheme } from './presets/dark';

const THEME_STYLE_ATTR = 'data-bitfun-theme';
const BACKGROUND_KEY = '--color-bg-primary';
const TEXT_KEY = '--color-text-primary';

function buildThemeCSS(): string {
  const fg = darkTheme[TEXT_KEY];
  const parts: string[] = [':root {'];
  for (const [key, value] of Object.entries(darkTheme)) {
    parts.push(`  ${key}: ${value};`);
  }
  parts.push('  color-scheme: dark;');
  parts.push('}');
  // Transparent page chrome so Android camera (phone) / optical AR shows through.
  parts.push(`html, body { background-color: transparent; color: ${fg}; color-scheme: dark; }`);
  return parts.join('\n');
}

const darkCss = buildThemeCSS();

function commitDarkThemeDOM() {
  const root = document.documentElement;
  const body = document.body;

  const newStyleEl = document.createElement('style');
  newStyleEl.setAttribute(THEME_STYLE_ATTR, 'dark');
  newStyleEl.textContent = darkCss;
  document.head.appendChild(newStyleEl);
  document.head.querySelectorAll(`style[${THEME_STYLE_ATTR}]`).forEach((el) => {
    if (el !== newStyleEl) el.remove();
  });

  for (const key of Object.keys(darkTheme)) {
    root.style.setProperty(key, darkTheme[key]);
  }
  root.style.setProperty('color-scheme', 'dark');
  root.setAttribute('data-theme', 'dark');
  root.setAttribute('data-theme-type', 'dark');
  body.setAttribute('data-theme', 'dark');
  body.style.backgroundColor = 'transparent';
  body.style.color = darkTheme[TEXT_KEY];

  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) {
    meta.setAttribute('content', darkTheme[BACKGROUND_KEY]);
    meta.removeAttribute('media');
  }
}

/** Dark-only theme bootstrap for AR glasses (no light/dark toggle). */
export const ThemeProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  useLayoutEffect(() => {
    commitDarkThemeDOM();
  }, []);

  return <>{children}</>;
};
