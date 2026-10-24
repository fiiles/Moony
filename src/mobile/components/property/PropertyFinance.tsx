import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CreditCard, Shield, Unlink } from 'lucide-react';
import type { InsurancePolicy, Loan, RealEstate } from '@shared/schema';
import { recordCurrency } from '@shared/currencies';
import type { YearlyBalance } from '@/utils/real-estate';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { RowMenu, RowMenuItem } from '@/mobile/components/cashflow/RowMenu';
import { SectionHead } from '@/mobile/components/SectionHead';
import { YearlyBalanceCard } from '@/mobile/components/property/YearlyBalanceCard';
import { useMoneyText } from '@/mobile/hooks/use-money-text';
import { useLinkableLoans, useLinkablePolicies } from '@/mobile/hooks/use-property';
import { usePropertyMutations } from '@/mobile/hooks/use-property-mutations';
import { policyPremium } from '@/mobile/lib/property-detail';
import { loanPath, policyPath } from '@/mobile/shell/routes';
import { LinkSheet } from '@/mobile/sheets/LinkSheet';

/** Whole amounts: the rows show no decimals. */
const WHOLE = { decimals: 0 } as const;

interface PropertyFinanceProps {
  property: RealEstate;
  balance: YearlyBalance;
  loans: readonly Loan[];
  policies: readonly InsurancePolicy[];
  /** The main currency. */
  currency: string;
}

/** The head of one group under "Finance": its name and the action that adds to it (a 44 pt target). */
function GroupHead({
  title,
  action,
}: {
  title: string;
  action?: { label: string; onClick: () => void };
}) {
  return (
    <div className="mx-0.5 mb-2 mt-4 flex items-baseline justify-between gap-3">
      <h3 className="m-0 min-w-0 truncate text-m-sub font-semibold text-ink-3">{title}</h3>
      {action && (
        <button
          type="button"
          onClick={action.onClick}
          className="relative flex-none whitespace-nowrap text-m-sub font-semibold text-ink-3 [-webkit-touch-callout:none] after:absolute after:-inset-x-2 after:-inset-y-[14px] after:content-[''] active:text-ink"
        >
          {action.label}
        </button>
      )}
    </div>
  );
}

/**
 * "Finance" of a property (spec §8.8 "Nemovitost"): the loans and the policies linked to it as
 * rows, each group with its Připojit, then the yearly balance. A loan row is the name, "splátka
 * 12 340 Kč / měs." and what is outstanding, a policy row the name, the insurer and the premium
 * with its frequency, each in its own currency; a tap opens the loan or the policy, a long press
 * the row menu with Odpojit. Připojit opens a sheet of the loans or policies no property owns yet,
 * so linking never moves one from another property. Every amount goes through the eye.
 */
export function PropertyFinance({
  property,
  balance,
  loans,
  policies,
  currency,
}: PropertyFinanceProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('realEstate');
  const { t: ti } = useTranslation('insurance');
  const moneyText = useMoneyText();
  const { link, unlink } = usePropertyMutations(property.id);

  const [menuFor, setMenuFor] = useState<string | null>(null);
  // The sheet keeps its kind while it slides away: which one and whether it is open are apart
  const [linkKind, setLinkKind] = useState<'loan' | 'insurance'>('loan');
  const [linkOpen, setLinkOpen] = useState(false);
  const loanOffer = useLinkableLoans(loans, linkOpen && linkKind === 'loan');
  const policyOffer = useLinkablePolicies(policies, linkOpen && linkKind === 'insurance');
  const offer = linkKind === 'loan' ? loanOffer : policyOffer;
  const candidates =
    linkKind === 'loan'
      ? loanOffer.candidates.map((loan) => ({
          id: loan.id,
          label: loan.name,
          sub: moneyText(Number(loan.principal) || 0, recordCurrency(loan.currency), WHOLE),
        }))
      : policyOffer.candidates.map((policy) => ({
          id: policy.id,
          label: policy.policyName,
          sub: [policy.provider, ti(`types.${policy.type}`, { defaultValue: policy.type })]
            .filter(Boolean)
            .join(' · '),
        }));

  const openLink = (kind: 'loan' | 'insurance') => {
    setLinkKind(kind);
    setLinkOpen(true);
  };
  const menu = (key: string, kind: 'loan' | 'insurance', itemId: string, name: string) => (
    <RowMenu
      open={menuFor === key}
      onOpenChange={(next) => setMenuFor(next ? key : null)}
      label={t('property.rowMenu', { name })}
    >
      <RowMenuItem onSelect={() => unlink.mutate({ kind, itemId })}>
        <Unlink strokeWidth={1.75} aria-hidden />
        {tr(kind === 'loan' ? 'detail.loans.unlink' : 'detail.insurances.unlink')}
      </RowMenuItem>
    </RowMenu>
  );

  return (
    <>
      <SectionHead title={tr('detail.tabs.financials')} />

      <GroupHead
        title={tr('detail.loans.title')}
        action={{ label: tr('detail.loans.link'), onClick: () => openLink('loan') }}
      />
      {loans.length === 0 ? (
        <p className="mx-0.5 mb-3.5 text-m-sub text-ink-3">{tr('detail.loans.empty')}</p>
      ) : (
        <EntityRows>
          {loans.map((loan) => {
            const code = recordCurrency(loan.currency);
            const key = `loan-${loan.id}`;
            return (
              <EntityRow
                key={loan.id}
                chip={
                  <RowChip>
                    <CreditCard strokeWidth={1.75} />
                  </RowChip>
                }
                title={loan.name}
                sub={t('property.loanPayment', {
                  amount: moneyText(Number(loan.monthlyPayment) || 0, code, WHOLE),
                })}
                value={moneyText(Number(loan.outstandingBalance) || 0, code, WHOLE)}
                href={loanPath(loan.id)}
                onLongPress={() => setMenuFor(key)}
                accessory={menu(key, 'loan', loan.id, loan.name)}
              />
            );
          })}
        </EntityRows>
      )}

      <GroupHead
        title={tr('detail.insurances.title')}
        action={{ label: tr('detail.insurances.link'), onClick: () => openLink('insurance') }}
      />
      {policies.length === 0 ? (
        <p className="mx-0.5 mb-3.5 text-m-sub text-ink-3">{tr('detail.insurances.empty')}</p>
      ) : (
        <EntityRows>
          {policies.map((policy) => {
            const premium = policyPremium(policy);
            const key = `policy-${policy.id}`;
            return (
              <EntityRow
                key={policy.id}
                chip={
                  <RowChip>
                    <Shield strokeWidth={1.75} />
                  </RowChip>
                }
                title={policy.policyName}
                sub={policy.provider}
                value={moneyText(premium.amount, premium.currency, WHOLE)}
                valueSub={
                  premium.frequency
                    ? ti(`frequencyShort.${premium.frequency}`)
                    : policy.paymentFrequency
                }
                href={policyPath(policy.id)}
                onLongPress={() => setMenuFor(key)}
                accessory={menu(key, 'insurance', policy.id, policy.policyName)}
              />
            );
          })}
        </EntityRows>
      )}

      <GroupHead title={tr('detail.balance.title')} />
      <YearlyBalanceCard property={property} balance={balance} currency={currency} />

      <LinkSheet
        open={linkOpen}
        onOpenChange={setLinkOpen}
        kind={linkKind}
        candidates={candidates}
        isLoading={offer.isLoading}
        isPending={link.isPending}
        onLink={(itemId) =>
          link.mutate({ kind: linkKind, itemId }, { onSuccess: () => setLinkOpen(false) })
        }
      />
    </>
  );
}
