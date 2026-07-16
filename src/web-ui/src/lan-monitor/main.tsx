import React from 'react';
import ReactDOM from 'react-dom/client';
import LanMonitorApp from './LanMonitorApp';
import '@/app/styles/index.scss';
import './styles.scss';

document.documentElement.dataset.theme = 'bitfun-dark';
document.documentElement.dataset.themeType = 'dark';
document.documentElement.style.colorScheme = 'dark';
ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <LanMonitorApp />
  </React.StrictMode>,
);
