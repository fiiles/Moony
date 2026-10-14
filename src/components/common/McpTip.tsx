import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { TipBanner } from '@/components/common/TipBanner';
import { mcpTipKey, type McpTipDomain } from '@/utils/tips';

interface McpTipProps {
  /** Which empty page this is — picks the summary line and the dismissal key. */
  domain: McpTipDomain;
  /** Overrides the shared summary/body (insurance keeps its own, older wording). */
  summary?: ReactNode;
  body?: ReactNode;
  legacyStorageKey?: string;
  className?: string;
}

/**
 * "An AI assistant can enter this for you" tip under an empty page's primary
 * action — shown on every list whose records the MCP server can create.
 */
export function McpTip({ domain, summary, body, legacyStorageKey, className }: McpTipProps) {
  const { t } = useTranslation('common');
  const [, navigate] = useLocation();

  return (
    <TipBanner
      className={className ?? 'w-full max-w-md text-left'}
      storageKey={mcpTipKey(domain)}
      legacyStorageKey={legacyStorageKey}
      summary={summary ?? t(`mcpTip.${domain}`)}
      details={
        <>
          <p>{body ?? t('mcpTip.body')}</p>
          <button
            type="button"
            onClick={() => navigate('/settings/integrations#mcp')}
            className="underline underline-offset-[3px] text-ink hover:text-ink-2"
          >
            {t('mcpTip.setupLink')}
          </button>
        </>
      }
    />
  );
}
