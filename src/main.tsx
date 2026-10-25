import { createRoot } from 'react-dom/client';
import '@fontsource-variable/inter';
import App from './App';
import { i18nReady } from './i18n';
import { isMobileShell } from './lib/platform';
import './index.css';

async function bootstrap() {
  const mobile = isMobileShell(import.meta.env);
  // Dev-only: lets the app run in a plain browser against the real backend.
  if (import.meta.env.DEV) await import('./dev/browser-bridge');
  // The phone shell is its own chunk: the desktop never loads it.
  const Root = mobile ? (await import('./mobile/MobileApp')).default : App;
  // The stored language's strings load before the first frame: no English flash.
  await i18nReady;
  createRoot(document.getElementById('root')!).render(<Root />);
}

void bootstrap();
