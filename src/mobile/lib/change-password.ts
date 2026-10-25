/**
 * Zabezpečení → Změnit heslo (spec §8.9, ruling 13): the desktop's two-phase change with the phone's
 * own key step. Pure rules of the sheet; the calls live in `use-change-password.ts`.
 */

/**
 * Where the change is: `form` (the three passwords), `key` (the new recovery key, saved and two of
 * its groups retyped; nothing has changed yet) and `done` (the password and the key are changed; the
 * sheet closes).
 */
export type ChangePasswordStep = 'form' | 'key' | 'done';

/**
 * "Změnit heslo" is on once the two retyped groups match the new key, and off while the change is
 * running, so a second tap never sends it twice.
 */
export function canConfirmChange(input: { verified: boolean; confirming: boolean }): boolean {
  return input.verified && !input.confirming;
}
