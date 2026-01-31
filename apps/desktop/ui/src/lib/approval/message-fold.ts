// A long message on an approval card is folded to its first lines until the person opens it (11 §5.5).

/** A message over this many lines is folded. */
export const MESSAGE_FOLD_AT = 12;
/** The lines kept while folded. */
export const MESSAGE_FOLD_KEEP = 6;

export function foldMessage(text: string, opened: boolean): { text: string; folded: boolean } {
  const lines = text.split('\n');
  if (opened || lines.length <= MESSAGE_FOLD_AT) return { text, folded: false };
  return { text: lines.slice(0, MESSAGE_FOLD_KEEP).join('\n'), folded: true };
}
