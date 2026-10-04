import React, { Component, ErrorInfo, ReactNode } from 'react';
import { AlertCircle, Check, Copy, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import i18n from '@/i18n';

interface Props {
  children?: ReactNode;
}

interface State {
  hasError: boolean;
  error?: Error;
  componentStack?: string;
  copied: boolean;
}

/** Plain-text report for bug reports: message, stack and the React component stack. */
function buildDetails(error: Error | undefined, componentStack: string | undefined): string {
  const header = `${error?.name ?? 'Error'}: ${error?.message ?? ''}`;
  const stack = error?.stack;
  const lines: string[] = [];
  // Chromium stacks start with the header line, WebKit's (macOS) do not.
  if (stack?.startsWith(header)) lines.push(stack);
  else lines.push(header, ...(stack ? ['', stack] : []));
  if (componentStack) lines.push('', 'Component stack:', componentStack.trim());
  return lines.join('\n');
}

export class ErrorBoundary extends Component<Props, State> {
  public state: State = {
    hasError: false,
    copied: false,
  };

  private copiedTimer: ReturnType<typeof setTimeout> | undefined;

  public static getDerivedStateFromError(error: Error): Partial<State> {
    return { hasError: true, error };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('Uncaught error:', error, errorInfo);
    this.setState({ componentStack: errorInfo.componentStack ?? undefined });
  }

  public componentWillUnmount() {
    clearTimeout(this.copiedTimer);
  }

  private handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(
        buildDetails(this.state.error, this.state.componentStack)
      );
      this.setState({ copied: true });
      clearTimeout(this.copiedTimer);
      this.copiedTimer = setTimeout(() => this.setState({ copied: false }), 2000);
    } catch (err) {
      console.error('Could not copy error details:', err);
    }
  };

  public render() {
    if (!this.state.hasError) {
      return this.props.children;
    }

    const { error, componentStack, copied } = this.state;
    const t = (key: string) => i18n.t(`common:errors.boundary.${key}`);

    return (
      <div className="flex min-h-screen items-center justify-center bg-canvas p-6 text-ink">
        <Card className="w-full max-w-lg p-6">
          <div className="flex items-start gap-3">
            <AlertCircle className="mt-0.5 size-5 shrink-0 text-loss" aria-hidden="true" />
            <div className="min-w-0 flex-1">
              <h1 className="text-h2 text-ink">{t('title')}</h1>
              <p className="mt-1.5 text-body text-ink-3">{t('description')}</p>
            </div>
          </div>
          <div className="mt-5 flex flex-wrap gap-2">
            <Button onClick={() => window.location.reload()}>
              <RefreshCw aria-hidden="true" />
              {t('reload')}
            </Button>
            <Button variant="outline" onClick={this.handleCopy}>
              {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
              {copied ? t('copied') : t('copyDetails')}
            </Button>
          </div>
          <details className="mt-5 rounded-r2 border border-line bg-well text-caption">
            <summary className="cursor-pointer select-none px-3 py-2 font-600 text-ink-2">
              {t('technicalDetails')}
            </summary>
            <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words border-t border-line p-3 font-mono text-micro text-ink-2">
              {buildDetails(error, componentStack)}
            </pre>
          </details>
        </Card>
      </div>
    );
  }
}
