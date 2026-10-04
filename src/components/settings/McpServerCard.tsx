import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Check, ChevronDown, CircleAlert, Copy, Eye, EyeOff } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useAuth } from '@/hooks/use-auth';
import { authApi } from '@/lib/tauri-api';
import { queryClient } from '@/lib/queryClient';
import { translateApiError } from '@/lib/translate-api-error';
import { cn } from '@/lib/utils';

/**
 * Settings → Integrations → AI asistent (MCP server). Advanced and technical
 *: collapsed unless a link asks for it (`/settings/integrations#mcp`).
 */
export function McpServerCard() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const { user: profile } = useAuth();
  const [open, setOpen] = useState(() => window.location.hash === '#mcp');
  const cardRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (window.location.hash === '#mcp') {
      cardRef.current?.scrollIntoView({ block: 'start' });
    }
  }, []);
  const [showToken, setShowToken] = useState(false);
  const [portDraft, setPortDraft] = useState<string | null>(null);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  const enabled = profile?.mcpServerEnabled ?? false;

  const setMcpMutation = useMutation({
    mutationFn: (value: boolean) => authApi.setMcpServerEnabled(value),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['user-profile'] });
      queryClient.invalidateQueries({ queryKey: ['mcp-status'] });
      queryClient.invalidateQueries({ queryKey: ['mcp-token'] });
    },
    onError: (error: Error) => {
      toast.error(t('mcpServer.updateFailed'), { description: error.message });
    },
  });

  const { data: mcpStatus } = useQuery({
    queryKey: ['mcp-status'],
    queryFn: () => authApi.getMcpServerStatus(),
    refetchInterval: open ? 5000 : false,
  });

  const { data: token } = useQuery({
    queryKey: ['mcp-token'],
    queryFn: () => authApi.getMcpServerToken(),
    enabled,
  });

  const portMutation = useMutation({
    mutationFn: (port: number) => authApi.setMcpServerPort(port),
    onSuccess: () => {
      setPortDraft(null);
      queryClient.invalidateQueries({ queryKey: ['mcp-status'] });
      toast.success(t('mcpServer.portUpdated'));
    },
    onError: (error: Error) => {
      queryClient.invalidateQueries({ queryKey: ['mcp-status'] });
      toast.error(t('mcpServer.updateFailed'), {
        description: translateApiError(error.message, tc),
      });
    },
  });

  const regenerateMutation = useMutation({
    mutationFn: () => authApi.regenerateMcpToken(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['mcp-token'] });
      queryClient.invalidateQueries({ queryKey: ['mcp-status'] });
      toast.success(t('mcpServer.tokenRegenerated'));
    },
    onError: (error: Error) => {
      // The token may have been replaced even when the restart failed —
      // refetch both so the reveal field shows the real stored token.
      queryClient.invalidateQueries({ queryKey: ['mcp-token'] });
      queryClient.invalidateQueries({ queryKey: ['mcp-status'] });
      toast.error(t('mcpServer.updateFailed'), { description: error.message });
    },
  });

  const draftPort = portDraft === null ? null : Number(portDraft);
  const draftPortValid =
    draftPort !== null && Number.isInteger(draftPort) && draftPort >= 1024 && draftPort <= 65535;

  const copy = async (key: string, text: string) => {
    await navigator.clipboard.writeText(text);
    setCopiedKey(key);
    setTimeout(() => setCopiedKey(null), 2000);
  };

  const url = mcpStatus?.url ?? 'http://127.0.0.1:41414/mcp';
  const tokenValue = token ?? '';
  const forDisplay = (s: string) =>
    !tokenValue || showToken ? s : s.replaceAll(tokenValue, '••••••••');

  const claudeCodeSnippet = `claude mcp add --transport http moony ${url} --header "Authorization: Bearer ${tokenValue}"`;
  const desktopSnippet = JSON.stringify(
    {
      mcpServers: {
        moony: {
          command: 'npx',
          args: ['mcp-remote', url, '--allow-http', '--header', 'Authorization:${AUTH_HEADER}'],
          env: { AUTH_HEADER: `Bearer ${tokenValue}` },
        },
      },
    },
    null,
    2
  );
  const mcpJsonSnippet = JSON.stringify(
    {
      mcpServers: {
        moony: { type: 'http', url, headers: { Authorization: `Bearer ${tokenValue}` } },
      },
    },
    null,
    2
  );

  const copyButton = (id: string, text: string) => (
    <Button
      variant="ghost"
      size="icon-sm"
      aria-label={tc('buttons.copy')}
      onClick={() => copy(id, text)}
    >
      {copiedKey === id ? <Check /> : <Copy />}
    </Button>
  );

  const snippet = (id: string, label: string, text: string, note?: string) => (
    <div>
      <div className="flex items-center justify-between gap-3">
        <p className="text-caption font-650 text-ink-2">{label}</p>
        {copyButton(id, text)}
      </div>
      {note && <p className="mb-1.5 text-micro font-500 text-ink-4">{note}</p>}
      <pre className="overflow-auto whitespace-pre-wrap break-all rounded-r2 border border-line bg-paper px-3 py-2 font-mono text-[11px] leading-[1.5] text-ink-2">
        {forDisplay(text)}
      </pre>
    </div>
  );

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <SettingsCard
        id="mcp"
        ref={cardRef}
        className="scroll-mt-4"
        title={
          <span className="inline-flex flex-wrap items-center gap-2">
            {t('mcpServer.title')}
            <Badge>{t('mcpServer.advancedBadge')}</Badge>
          </span>
        }
        description={open ? t('mcpServer.description') : t('mcpServer.summary')}
        action={
          <CollapsibleTrigger asChild>
            <Button variant="ghost" size="sm">
              {open ? t('mcpServer.collapse') : t('mcpServer.expand')}
              <ChevronDown
                className={cn('transition-transform duration-base', open && 'rotate-180')}
              />
            </Button>
          </CollapsibleTrigger>
        }
      >
        <CollapsibleContent>
          <SettingsRow
            htmlFor="mcp-enabled"
            label={t('mcpServer.enable')}
            hint={t('mcpServer.enableHint')}
          >
            <Switch
              id="mcp-enabled"
              checked={enabled}
              onCheckedChange={(value) => setMcpMutation.mutate(value)}
              disabled={setMcpMutation.isPending}
            />
          </SettingsRow>

          {mcpStatus?.running && (
            <Alert className="mt-3">
              <Check />
              <AlertDescription>{t('mcpServer.statusRunning', { url })}</AlertDescription>
            </Alert>
          )}
          {enabled && !mcpStatus?.running && mcpStatus?.lastError && (
            <Alert variant="destructive" className="mt-3">
              <CircleAlert />
              <AlertDescription>
                {t('mcpServer.statusError', { error: mcpStatus.lastError })}
              </AlertDescription>
            </Alert>
          )}

          {enabled && (
            <>
              <SettingsRow label={t('mcpServer.endpoint')}>
                <code className="rounded-r1 bg-well px-2 py-1 font-mono text-[11px] text-ink-2">
                  {url}
                </code>
                {copyButton('url', url)}
              </SettingsRow>

              <SettingsRow
                label={t('mcpServer.token')}
                hint={`${t('mcpServer.tokenHint')} ${t('mcpServer.regenerateHint')}`}
              >
                <code className="rounded-r1 bg-well px-2 py-1 font-mono text-[11px] text-ink-2">
                  {showToken ? tokenValue : '••••••••-••••-••••-••••-••••••••••••'}
                </code>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={showToken ? t('apiKeys.hide') : t('apiKeys.show')}
                  onClick={() => setShowToken((v) => !v)}
                >
                  {showToken ? <EyeOff /> : <Eye />}
                </Button>
                {copyButton('token', tokenValue)}
                <Button
                  variant="outline"
                  size="sm"
                  loading={regenerateMutation.isPending}
                  onClick={() => regenerateMutation.mutate()}
                >
                  {t('mcpServer.regenerate')}
                </Button>
              </SettingsRow>

              <SettingsRow
                htmlFor="mcp-port"
                label={t('mcpServer.port')}
                hint={t('mcpServer.portHint')}
              >
                <Input
                  id="mcp-port"
                  type="number"
                  min={1024}
                  max={65535}
                  className="w-[110px] num"
                  value={portDraft ?? String(mcpStatus?.port ?? 41414)}
                  onChange={(e) => setPortDraft(e.target.value)}
                />
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!draftPortValid}
                  loading={portMutation.isPending}
                  onClick={() => portMutation.mutate(draftPort as number)}
                >
                  {t('mcpServer.applyPort')}
                </Button>
              </SettingsRow>

              <div className="mt-4 space-y-3 rounded-r3 bg-well px-4 py-[14px]">
                <p className="text-body font-650 text-ink">{t('mcpServer.setupHeading')}</p>
                {snippet('cc', t('mcpServer.setupClaudeCode'), claudeCodeSnippet)}
                {snippet(
                  'cd',
                  t('mcpServer.setupClaudeDesktop'),
                  desktopSnippet,
                  t('mcpServer.desktopNote')
                )}
                {snippet('json', t('mcpServer.setupJson'), mcpJsonSnippet, t('mcpServer.jsonNote'))}
              </div>
            </>
          )}
        </CollapsibleContent>
      </SettingsCard>
    </Collapsible>
  );
}
