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
      { id: "file.opennewtab", label: "Open in new tab", glyph: "\uE8A7" },
      { id: "file.opennewwindow", label: "Open in new window", glyph: "\uE737" },
      { id: "file.openlocation", label: "Open file location", glyph: "\uE838" },
      { id: "file.openwith", label: "Open with", glyph: "\uE7AC" },
      { id: "file.extractall", label: "Extract all / Extract (archives)", glyph: "\uE8B7" },
      { id: "file.keep", label: "Always keep on this device (cloud files)", glyph: "\uE73E" },
      { id: "file.freeup", label: "Free up space (cloud files)", glyph: "\uE753" },
      { id: "file.pin", label: "Pin to / Unpin from Quick access", glyph: "\uE840" },
      { id: "file.favorite", label: "Add to / Remove from Favorites (files)", glyph: "\uE734" },
      { id: "file.compress", label: "Compress to ZIP file", glyph: "\uF012" },
      { id: "file.copypath", label: "Copy as path", glyph: "\uE8C8" },
      { id: "file.sendto", label: "Copy to / Move to another window's folder", glyph: "\uE8F0" },
      { id: "file.checksum", label: "Checksums", glyph: "\uE8D7" },
      { id: "file.compare", label: "Compare files (two files)", glyph: "\uE8F1" },
      { id: "file.size", label: "Calculate size", glyph: "\uEDA2" },
      { id: "file.shortcut", label: "Create shortcut", glyph: "\uE71B" },
      { id: "file.share", label: "Share", glyph: "\uE72D" },
      { id: "file.properties", label: "Properties", glyph: "\uE90F" },
      { id: "file.moreoptions", label: "Show more options", glyph: "\uE712" },
    ],
  },
  {
    key: "blank",
    label: "Empty space",
    desc: "Right-clicking the background of a folder",
    items: [
      { id: "blank.refresh", label: "Refresh", glyph: "\uE72C" },
      { id: "blank.paste", label: "Paste", glyph: "\uE77F" },
      { id: "blank.pasteshortcut", label: "Paste shortcut", glyph: "\uE71B" },
      { id: "blank.new", label: "New", glyph: "\uE710" },
      { id: "blank.selectall", label: "Select all", glyph: "\uE8B3" },
      { id: "blank.terminal", label: "Open in Terminal", glyph: "\uE756" },
      { id: "blank.sizes", label: "Folder sizes", glyph: "\uEDA2" },
    ],
  },
  {
    key: "drive",
    label: "Drive",
    desc: "Right-clicking a drive on the This PC page",
    items: [
      { id: "drive.open", label: "Open", glyph: "\uE8E5" },
      { id: "drive.pin", label: "Pin to Quick access", glyph: "\uE840" },
      { id: "drive.eject", label: "Eject", glyph: "\uF847" },
      { id: "drive.disconnect", label: "Disconnect (network drives)", glyph: "\uE8CD" },
      { id: "drive.properties", label: "Properties", glyph: "\uE90F" },
      { id: "drive.moreoptions", label: "Show more options", glyph: "\uE712" },
    ],
  },
  {
    key: "home",
    label: "Home",
    desc: "Right-clicking a Quick access folder, a favorite or a recent file on Home",
    items: [
      { id: "home.open", label: "Open", glyph: "" },
      { id: "home.newtab", label: "Open in new tab", glyph: "" },
      { id: "home.newwindow", label: "Open in new window", glyph: "" },
      { id: "home.openlocation", label: "Open file location", glyph: "" },
      { id: "home.pin", label: "Unpin from Quick access", glyph: "" },
      { id: "home.favorite", label: "Add to / Remove from Favorites", glyph: "" },
      { id: "home.removerecent", label: "Remove from Recent", glyph: "" },
      { id: "home.properties", label: "Properties", glyph: "" },
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
      { id: "tab.movewindow", label: "Move to new window", glyph: "\uE737" },
      { id: "tab.close", label: "Close tab", glyph: "\uE8BB" },
      { id: "tab.closeothers", label: "Close other tabs", glyph: "\uE8BB" },
      { id: "tab.closeright", label: "Close tabs to the right", glyph: "\uE8BB" },
    ],
  },
];

