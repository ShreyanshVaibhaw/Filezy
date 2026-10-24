// English is the supported language. Every UI message keeps its English source
// as a stable fallback; catalogues can be added without changing components.
import { pluralMessage, translate, type Messages } from "./messages";
let catalogue = $state<Messages>({});
let pseudo = $state(false);
export function msg(message: string, values?: Record<string, string | number>): string {
  return translate(message, values, catalogue, pseudo);
}
/// A sentence with a count in it: `one` or `other` by the language's plural
/// rules, with {n} the formatted count and any other placeholders filled.
export function msgCount(n: number, one: string, other: string, values: Record<string, string | number> = {}): string {
  return msg(pluralMessage(n, one, other), { n: n.toLocaleString(), ...values });
}
export function setMessages(messages: Messages, expand = false) {
  catalogue = messages; pseudo = expand;
  document.documentElement.lang = "en";
}
