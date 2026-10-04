import { createRoot } from 'react-dom/client';
import '@fontsource-variable/inter';
import App from './App';
import './index.css';

async function bootstrap() {
  // Dev-only: lets the app run in a plain browser against the real backend.
  if (import.meta.env.DEV) await import('./dev/browser-bridge');
  createRoot(document.getElementById('root')!).render(<App />);
}

void bootstrap();
