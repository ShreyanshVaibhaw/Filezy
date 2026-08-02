/// The one list of native context-menu entries the user can switch off.
/// The Settings page renders its checkboxes from this; each menu asks
/// `menuOn("<id>")` at its own render site. `npm run check:menuids` fails the
/// build if the two ever drift apart.

export interface MenuItemDef {
  id: string;
  label: string;
  glyph: string;
}

export interface MenuGroup {
  key: string;
  label: string;
  desc: string;
  items: MenuItemDef[];
}

export const MENU_GROUPS: MenuGroup[] = [
  {
    key: "file",
    label: "File or folder",
    desc: "Right-clicking an item in the file list",
    items: [
      { id: "file.cut", label: "Cut", glyph: "\uE8C6" },
      { id: "file.copy", label: "Copy", glyph: "\uE8C8" },
      { id: "file.rename", label: "Rename", glyph: "\uE8AC" },
      { id: "file.delete", label: "Delete", glyph: "\uE74D" },
      { id: "file.open", label: "Open", glyph: "\uE8E5" },
      { id: "file.openwith", label: "Open with", glyph: "\uE7AC" },
      { id: "file.pin", label: "Pin to / Unpin from Quick access", glyph: "\uE840" },
      { id: "file.copypath", label: "Copy as path", glyph: "\uE8C8" },
      { id: "file.properties", label: "Properties", glyph: "\uE90F" },
    ],
  },
  {
    key: "blank",
    label: "Empty space",
    desc: "Right-clicking the background of a folder",
    items: [
      { id: "blank.refresh", label: "Refresh", glyph: "\uE72C" },
      { id: "blank.paste", label: "Paste", glyph: "\uE77F" },
      { id: "blank.new", label: "New", glyph: "\uE710" },
      { id: "blank.selectall", label: "Select all", glyph: "\uE8B3" },
      { id: "blank.terminal", label: "Open in Terminal", glyph: "\uE756" },
    ],
  },
  {
    key: "drive",
    label: "Drive",
    desc: "Right-clicking a drive on the This PC page",
    items: [
      { id: "drive.open", label: "Open", glyph: "\uE8E5" },
      { id: "drive.pin", label: "Pin to Quick access", glyph: "\uE840" },
      { id: "drive.properties", label: "Properties", glyph: "\uE90F" },
    ],
  },
  {
    key: "nav",
    label: "Navigation pane",
    desc: "Right-clicking a pinned folder in the sidebar",
    items: [{ id: "nav.unpin", label: "Unpin from Quick access", glyph: "\uE77A" }],
  },
  {
    key: "tab",
    label: "Tab",
    desc: "Right-clicking a tab in the title bar",
    items: [
      { id: "tab.duplicate", label: "Duplicate tab", glyph: "\uE8C8" },
      { id: "tab.close", label: "Close tab", glyph: "\uE8BB" },
      { id: "tab.closeothers", label: "Close other tabs", glyph: "\uE8BB" },
      { id: "tab.closeright", label: "Close tabs to the right", glyph: "\uE8BB" },
    ],
  },
];

export const ALL_MENU_IDS: string[] = MENU_GROUPS.flatMap((g) => g.items.map((i) => i.id));
