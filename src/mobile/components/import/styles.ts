/**
 * The footer buttons of the import sheet and the sheets it stacks (README §13, `.m-btn--wrap`): a
 * ghost "Zrušit" at its natural width and a primary that takes the rest of the row and may wrap
 * to a second line — "48 Transaktionen importieren" beside "Abbrechen" does not fit one line at 360.
 */
export const FOOTER_GHOST =
  'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-[18px] text-m-row';
export const FOOTER_GHOST_WIDE =
  'h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row';
export const FOOTER_PRIMARY =
  'h-auto min-h-12 min-w-0 flex-1 whitespace-normal rounded-[10px] py-2 text-center text-m-row leading-[1.15]';
