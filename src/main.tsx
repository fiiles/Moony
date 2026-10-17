import { createRoot } from 'react-dom/client';
import '@fontsource-variable/inter';
import App from './App';
import { i18nReady } from './i18n';
import { isMobileShell } from './lib/platform';
import './index.css';

async function bootstrap() {
  const mobile = isMobileShell(import.meta.env);
  // The phone is light only until Phase 6 (dark phone tokens): `theme-boot.js` stamped `data-theme`
  // from the OS appearance, which would put the desktop's dark block under a shell that has no
  // ThemeProvider and no dark design. Overwrite its only attribute before anything renders.
  if (mobile) document.documentElement.dataset.theme = 'light';
  // Dev-only: lets the app run in a plain browser against the real backend.
  if (import.meta.env.DEV) await import('./dev/browser-bridge');
  // The phone shell is its own chunk: the desktop never loads it.
  const Root = mobile ? (await import('./mobile/MobileApp')).default : App;
  // The stored language's strings load before the first frame: no English flash.
  await i18nReady;
  createRoot(document.getElementById('root')!).render(<Root />);
}

void bootstrap();
