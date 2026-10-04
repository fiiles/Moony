/** The categorization rules page. */
export const RULES_PAGE_PATH = '/settings/categorization-rules';

/** Longest payee text taken from a link; real payee names are far shorter. */
const MAX_PAYEE_LENGTH = 200;

export interface NewRuleParams {
  /** Payee or description the new rule should match. */
  payee: string;
  /** Category the rule assigns, when known. */
  categoryId: string | null;
}

/**
 * Link to the rules page that opens the "new rule" dialog prefilled
 * (`?new=1&payee=…&category=…`), e.g. from a transaction row's "Create a rule" button (RUL-03).
 */
export function newRuleHref(params: { payee?: string | null; categoryId?: string | null }): string {
  const query = new URLSearchParams({ new: '1' });
  const payee = params.payee?.trim();
  if (payee) query.set('payee', payee);
  if (params.categoryId) query.set('category', params.categoryId);
  return `${RULES_PAGE_PATH}?${query.toString()}`;
}

/** The prefill requested by the rules page's query string, or `null` when it asks for none. */
export function parseNewRuleSearch(search: string): NewRuleParams | null {
  const query = new URLSearchParams(search);
  if (query.get('new') !== '1') return null;
  return {
    payee: (query.get('payee') ?? '').trim().slice(0, MAX_PAYEE_LENGTH),
    categoryId: query.get('category') || null,
  };
}
