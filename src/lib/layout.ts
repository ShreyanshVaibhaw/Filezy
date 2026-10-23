// Layout math for the file views, apart from FilePane so node tests can load it.

/** A group's items, `start` to `start + count`; a collapsed group shows none. */
export interface GroupSpan {
  start: number;
  count: number;
  open: boolean;
}

/// The item Up, Down, Page Up or Page Down lands on: `rows` visual rows from
/// item `i` (negative is up) in a grid of `cols` columns where each group
/// starts a new row and collapsed groups show nothing. The column is kept,
/// or the last item of a shorter row is taken, as in Explorer. `groups` null
/// means one group of `total` items.
export function gridStep(i: number, rows: number, cols: number, groups: GroupSpan[] | null, total: number): number {
  const spans = groups ?? [{ start: 0, count: total, open: true }];
  // Visual rows as [first item, item count], top to bottom.
  const lines: [number, number][] = [];
  let at = -1;
  for (const g of spans) {
    if (!g.open) continue;
    for (let r = 0; r * cols < g.count; r++) {
      const first = g.start + r * cols;
      const len = Math.min(cols, g.count - r * cols);
      if (i >= first && i < first + len) at = lines.length;
      lines.push([first, len]);
    }
  }
  if (!lines.length) return i;
  let target = at + rows;
  if (at < 0) {
    // In no visible row (a collapsed group): step from where it would sit.
    const next = lines.findIndex(([first]) => first > i);
    const below = next < 0 ? lines.length : next;
    target = rows > 0 ? below + rows - 1 : below + rows;
  }
  const col = at >= 0 ? i - lines[at][0] : 0;
  const [first, len] = lines[Math.max(0, Math.min(lines.length - 1, target))];
  return first + Math.min(col, len - 1);
}
