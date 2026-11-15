import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import 'oink-ui/styles.css';
import { OinkProvider, Toaster } from 'oink-ui';
import { App } from './App';
import './app.css';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <OinkProvider defaultTheme="system" defaultAccent="pink" locale="zh-CN" storageKey="clipsync-ui">
      <App />
      <Toaster position="bottom-right" />
    </OinkProvider>
  </StrictMode>,
);
