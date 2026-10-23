// The layouts and sort orders the menus and Settings offer, with their names.
// No imports, so settings validation can use them too.

/** Layouts, in the View menu's order. */
export const VIEWS: [string, string][] = [
  ["icons_xl", "Extra large icons"],
  ["icons_l", "Large icons"],
  ["icons_m", "Medium icons"],
  ["icons_s", "Small icons"],
  ["list", "List"],
  ["details", "Details"],
  ["content", "Content"],
  ["tiles", "Tiles"],
  ["gallery", "Gallery"],
];
export const VIEW_KEYS = VIEWS.map(([k]) => k);

/** The Sort menu's orders; the first five can also be the default for new folders. */
export const SORTS: [string, string][] = [
  ["name", "Name"],
  ["modified", "Date modified"],
  ["type", "Type"],
  ["size", "Size"],
  ["created", "Date created"],
  ["author", "Authors"],
  ["category", "Categories"],
  ["tags", "Tags"],
  ["title", "Title"],
];
export const DEFAULT_SORTS = SORTS.slice(0, 5);
