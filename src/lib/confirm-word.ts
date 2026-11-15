/**
 * Whether the typed text is the confirmation word of a destructive action (audit B5: "Smazat
 * všechna data" asks for the word in the UI language, "SMAZAT", "LÖSCHEN", "削除"). Case and the
 * spaces around it do not matter, nor the Unicode form the keyboard produced; the letters,
 * diacritics included, must be the word's. An empty word never matches.
 */
export function confirmWordMatches(typed: string, word: string): boolean {
  const fold = (text: string) => text.trim().normalize('NFC').toLocaleUpperCase();
  return fold(word) !== '' && fold(typed) === fold(word);
}
