export type Messages = Readonly<Record<string, string>>;

/** Keep named placeholders intact, including in expanded test translations. */
export function translate(message: string, values: Record<string, string | number> = {}, messages: Messages = {}, pseudo = false): string {
  let text = messages[message] ?? message;
  if (pseudo) text = `[${text.replace(/\{[\w]+\}|[aeiouAEIOU]/g, (s) => s.startsWith("{") ? s : s + s)}]`;
  return text.replace(/\{([\w]+)\}/g, (match, key: string) => Object.hasOwn(values, key) ? String(values[key]) : match);
}

export function pluralMessage(count: number, one: string, other: string, locale = "en"): string {
  return new Intl.PluralRules(locale).select(count) === "one" ? one : other;
}
