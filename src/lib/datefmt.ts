// Dates in the user's Windows short date and time formats ("dd-MM-yyyy",
// "HH:mm", "h:mm tt"), as Explorer shows them, rather than in the WebView's
// own idea of the locale. The formats come from the backend (locale_formats).

export interface LocaleFormats {
  date: string;
  time: string;
  am: string;
  pm: string;
}

const pad = (n: number) => String(n).padStart(2, "0");

function names(kind: "month" | "weekday", style: "short" | "long", d: Date): string {
  return new Intl.DateTimeFormat(undefined, { [kind]: style }).format(d);
}

/// One run of a pattern letter (d, M, y, h, H, m, s, t) for this date.
function token(c: string, n: number, d: Date, f: LocaleFormats): string | null {
  switch (c) {
    case "d":
      return n === 1 ? String(d.getDate()) : n === 2 ? pad(d.getDate()) : names("weekday", n === 3 ? "short" : "long", d);
    case "M":
      return n === 1 ? String(d.getMonth() + 1) : n === 2 ? pad(d.getMonth() + 1) : names("month", n === 3 ? "short" : "long", d);
    case "y":
      return n === 1 ? String(d.getFullYear() % 100) : n === 2 ? pad(d.getFullYear() % 100) : String(d.getFullYear());
    case "h": {
      const h = d.getHours() % 12 || 12;
      return n === 1 ? String(h) : pad(h);
    }
    case "H":
      return n === 1 ? String(d.getHours()) : pad(d.getHours());
    case "m":
      return n === 1 ? String(d.getMinutes()) : pad(d.getMinutes());
    case "s":
      return n === 1 ? String(d.getSeconds()) : pad(d.getSeconds());
    case "t": {
      const t = d.getHours() < 12 ? f.am : f.pm;
      return n === 1 ? t.slice(0, 1) : t;
    }
    case "g":
      return ""; // era: Explorer leaves it out of the Gregorian calendar
    default:
      return null;
  }
}

/// Format a date with a Windows picture string; text in single quotes is literal.
export function formatWith(pattern: string, d: Date, f: LocaleFormats): string {
  let out = "";
  let i = 0;
  while (i < pattern.length) {
    const c = pattern[i];
    if (c === "'") {
      const j = pattern.indexOf("'", i + 1);
      out += pattern.slice(i + 1, j < 0 ? undefined : j);
      i = j < 0 ? pattern.length : j + 1;
      continue;
    }
    let n = 1;
    while (pattern[i + n] === c) n++;
    out += token(c, n, d, f) ?? pattern.slice(i, i + n);
    i += n;
  }
  return out.trim();
}
