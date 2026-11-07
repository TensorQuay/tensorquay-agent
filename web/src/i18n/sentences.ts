/**
 * The gap between two sentences shown side by side: a space in languages that space their sentences, nothing in
 * Chinese and Japanese, whose full stop (。) already ends the sentence ("…查看。到期时间…", WEB-U-361; UI-GUIDELINES
 * §11b).
 */
export function sentenceGap(locale: string): string {
  return /^(zh|ja)\b/.test(locale) ? "" : " ";
}
