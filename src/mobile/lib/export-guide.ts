/**
 * The per-bank export guides (spec §8.6, rung 2): each bank preset carries, in its JSON, three short
 * steps in every UI language that say how to get a statement file out of that bank on a phone. The
 * guides are data — this only picks which one to show and in which language.
 */
import type { CsvPreset, MobileExportGuide } from '@shared/schema';
import { matchLanguage } from '@/i18n/languages';

/** A preset that has a guide. */
export type GuidedPreset = CsvPreset & { mobileExport: MobileExportGuide };

const hasGuide = (preset: CsvPreset): preset is GuidedPreset => preset.mobileExport !== null;

/**
 * The steps in `language` ("de-AT" reads as German), else in English — a locale the guide lacks (or
 * leaves empty) never shows nothing. Empty only when the guide has no English either.
 */
export function guideSteps(guide: MobileExportGuide, language: string): string[] {
  const own = guide.steps[matchLanguage(language) ?? 'en'];
  if (own && own.length > 0) return own;
  return guide.steps.en ?? [];
}

/** The guide of the bank an account belongs to (its institution), or null: the bank has none, or none is known. */
export function guideForAccount(
  presets: readonly CsvPreset[],
  account: { institutionId: string | null } | null | undefined
): GuidedPreset | null {
  const institutionId = account?.institutionId;
  if (!institutionId) return null;
  return presets.filter(hasGuide).find((preset) => preset.institutionId === institutionId) ?? null;
}

/** The banks that have a guide, by country and then by name (diacritics ignored: "Česká spořitelna" among the Cs). */
export function guidesList(presets: readonly CsvPreset[]): GuidedPreset[] {
  return presets
    .filter(hasGuide)
    .sort(
      (a, b) =>
        a.country.localeCompare(b.country) ||
        a.bankName.localeCompare(b.bankName, undefined, { sensitivity: 'base' })
    );
}

/** "Chase" for the data's "Chase (US)": the guide and its list say the bank, the country is a line of its own. */
export function guideBankName(bankName: string): string {
  const bare = bankName.replace(/\s*\([^)]*\)\s*$/, '').trim();
  return bare === '' ? bankName : bare;
}
