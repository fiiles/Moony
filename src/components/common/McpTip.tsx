import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { TipBanner } from '@/components/common/TipBanner';
import { mcpTipKey, type McpTipDomain } from '@/utils/tips';

interface McpTipProps {
  /** Which empty page this is — picks the summary line and the dismissal key. */
  domain: McpTipDomain;
  className?: string;
}

/**
 * "An AI assistant can enter this for you" tip under an empty page's primary
 * action — shown on every list whose records the MCP server can create.
 */
export function McpTip({ domain, className }: McpTipProps) {
  const { t } = useTranslation('common');
  const [, navigate] = useLocation();

  return (
    <TipBanner
      className={className ?? 'w-full max-w-md text-left'}
      storageKey={mcpTipKey(domain)}
      summary={t(`mcpTip.${domain}`)}
      details={
        <>
          <p>{t('mcpTip.body')}</p>
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
