// English is the supported language. Every UI message keeps its English source
// as a stable fallback; catalogues can be added without changing components.
import { translate, type Messages } from "./messages";
let catalogue = $state<Messages>({});
let pseudo = $state(false);
export function msg(message: string, values?: Record<string, string | number>): string {
  return translate(message, values, catalogue, pseudo);
}
export function setMessages(messages: Messages, expand = false) {
  catalogue = messages; pseudo = expand;
  document.documentElement.lang = "en";
}
