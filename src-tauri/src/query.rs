//! The part of Windows' Advanced Query Syntax the search box speaks when Filezy
//! searches without the Windows index: words that must all be in the name,
//! plus kind:, ext: (or type:), size:, datemodified: and datecreated: filters.
//! With the index, the whole query goes to Windows, which also matches file
//! contents and every other property.
//!
//!   report kind:document size:>1MB datemodified:this week
//!   "annual report" ext:pdf datecreated:2024-03-05
use crate::listing::Entry;

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    /// Lowercased words (or quoted phrases), all of which the name must contain.
    pub words: Vec<String>,
    /// Allowed extensions, lowercase without the dot; empty allows any.
    pub exts: Vec<String>,
    /// Folders only (kind:folder), files only (any other kind), or both.
    pub folders: Option<bool>,
    /// Inclusive byte range.
    pub size: Option<(u64, u64)>,
    /// Half-open millisecond ranges [from, to).
    pub modified: Option<(u64, u64)>,
    pub created: Option<(u64, u64)>,
}

impl Query {
    /// Filters beyond plain words, which the walker applies from file metadata.
    pub fn has_filters(&self) -> bool {
        !self.exts.is_empty() || self.folders.is_some() || self.size.is_some() || self.modified.is_some() || self.created.is_some()
    }

    pub fn matches(&self, e: &Entry) -> bool {
        let name = e.name.to_lowercase();
        if !self.words.iter().all(|w| name.contains(w.as_str())) {
            return false;
        }
        if self.folders.is_some_and(|f| f != e.is_dir) {
            return false;
        }
        if !self.exts.is_empty() && (e.is_dir || !self.exts.contains(&e.ext)) {
            return false;
        }
        let within = |r: Option<(u64, u64)>, v: u64| r.is_none_or(|(a, b)| v >= a && v < b);
        if let Some((lo, hi)) = self.size {
            if e.is_dir || e.size < lo || e.size > hi {
                return false;
            }
        }
        within(self.modified, e.modified) && within(self.created, e.created)
    }
}

/// Explorer's kinds, as the extensions the walker can recognize them by.
fn kind_exts(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        "document" | "documents" => &["doc", "docx", "docm", "odt", "rtf", "pdf", "xls", "xlsx", "xlsm", "ods", "csv", "ppt", "pptx", "odp", "txt", "md", "xps", "epub"],
        "picture" | "pictures" | "image" | "photo" => &["jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif", "avif", "ico", "svg", "raw", "cr2", "nef", "dng", "arw"],
        "video" | "videos" | "movie" => &["mp4", "mkv", "avi", "mov", "wmv", "webm", "m4v", "mpg", "mpeg", "flv", "3gp", "ts"],
        "music" | "audio" | "song" => &["mp3", "wav", "flac", "aac", "m4a", "ogg", "opus", "wma", "aiff", "mid", "midi"],
        "archive" | "compressed" => &["zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "xz", "cab", "iso"],
        "program" | "programs" | "app" => &["exe", "msi", "msix", "appx", "bat", "cmd", "com", "lnk", "ps1"],
        "text" => &["txt", "md", "log", "ini", "json", "xml", "csv", "yaml", "yml", "toml"],
        "email" | "e-mail" => &["eml", "msg", "pst", "ost"],
        "link" => &["lnk", "url"],
        _ => return None,
    })
}

/// Explorer's size buckets, as inclusive byte ranges.
fn size_bucket(v: &str) -> Option<(u64, u64)> {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    Some(match v {
        "empty" => (0, 0),
        "tiny" => (1, 16 * KB),
        "small" => (16 * KB + 1, MB),
        "medium" => (MB + 1, 128 * MB),
        "large" => (128 * MB + 1, GB),
        "huge" => (GB + 1, 4 * GB),
        "gigantic" => (4 * GB + 1, u64::MAX),
        _ => return None,
    })
}

/// "10MB", "1.5 GB", "512" (bytes) -> bytes.
fn bytes(v: &str) -> Option<u64> {
    let v = v.trim().to_lowercase();
    let split = v.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(v.len());
    let (num, unit) = v.split_at(split);
    let n: f64 = num.trim().parse().ok()?;
    let mult = match unit.trim() {
        "" | "b" | "bytes" => 1u64,
        "kb" | "k" => 1 << 10,
        "mb" | "m" => 1 << 20,
        "gb" | "g" => 1 << 30,
        "tb" | "t" => 1 << 40,
        _ => return None,
    };
    (n >= 0.0).then_some((n * mult as f64) as u64)
}

fn size_filter(v: &str) -> Option<(u64, u64)> {
    if let Some(b) = size_bucket(v) {
        return Some(b);
    }
    if let Some((a, b)) = v.split_once("..") {
        return Some((bytes(a)?, bytes(b)?));
    }
    if let Some(r) = v.strip_prefix(">=") {
        return Some((bytes(r)?, u64::MAX));
    }
    if let Some(r) = v.strip_prefix("<=") {
        return Some((0, bytes(r)?));
    }
    if let Some(r) = v.strip_prefix('>') {
        return Some((bytes(r)?.saturating_add(1), u64::MAX));
    }
    if let Some(r) = v.strip_prefix('<') {
        return Some((0, bytes(r)?.saturating_sub(1)));
    }
    let n = bytes(v.trim_start_matches('='))?;
    Some((n, n))
}

const DAY: i64 = 86_400_000;

/// Days since the Unix epoch for a civil date (Howard Hinnant's days_from_civil).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil(day: i64) -> (i64, i64, i64) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// A date value as a [from, to) range of UTC milliseconds. Relative names
/// follow Explorer: weeks start on Monday; "this month" is the calendar month.
/// `tz` shifts UTC to local time, in milliseconds.
fn date_range(v: &str, now_ms: i64, tz: i64) -> Option<(u64, u64)> {
    let today = (now_ms + tz).div_euclid(DAY);
    let (y, m, _) = civil(today);
    let weekday = (today + 3).rem_euclid(7); // 0 = Monday (1970-01-01 was a Thursday)
    let week = today - weekday;
    let days = |a: i64, b: i64| Some((((a * DAY) - tz).max(0) as u64, ((b * DAY) - tz).max(0) as u64));
    let month = |y: i64, m: i64| days_from_civil(y, m, 1);
    let next = |y: i64, m: i64| if m == 12 { month(y + 1, 1) } else { month(y, m + 1) };
    match v {
        "today" => days(today, today + 1),
        "yesterday" => days(today - 1, today),
        "this week" => days(week, week + 7),
        "last week" => days(week - 7, week),
        "this month" => days(month(y, m), next(y, m)),
        "last month" => {
            let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
            days(month(py, pm), month(y, m))
        }
        "this year" => days(month(y, 1), month(y + 1, 1)),
        "last year" => days(month(y - 1, 1), month(y, 1)),
        _ => {
            let (op, rest) = [">=", "<=", ">", "<"].iter().find_map(|op| v.strip_prefix(op).map(|r| (*op, r))).unwrap_or(("", v));
            let mut parts = rest.split(['-', '/', '.']);
            let (a, b, c): (i64, i64, i64) = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
            if parts.next().is_some() {
                return None;
            }
            // yyyy-mm-dd; also d-m-yyyy as Windows writes it in many locales.
            let (yy, mm, dd) = if a > 31 { (a, b, c) } else { (c, b, a) };
            if !(1..=12).contains(&mm) || !(1..=31).contains(&dd) {
                return None;
            }
            let day = days_from_civil(yy, mm, dd);
            match op {
                ">" => days(day + 1, 1_000_000),
                ">=" => days(day, 1_000_000),
                "<" => days(0, day),
                "<=" => days(0, day + 1),
                _ => days(day, day + 1),
            }
        }
    }
}

/// Values Explorer writes with a space in them ("this week").
const TWO_WORDS: [&str; 6] = ["this week", "last week", "this month", "last month", "this year", "last year"];

/// Read a query. `now_ms` and `tz` (local offset in ms) place relative dates.
pub fn parse(q: &str, now_ms: i64, tz: i64) -> Result<Query, String> {
    let mut out = Query { words: vec![], exts: vec![], folders: None, size: None, modified: None, created: None };
    let mut tokens: Vec<String> = Vec::new();
    // Split on spaces, keeping "quoted phrases" (and quoted filter values) whole.
    let mut cur = String::new();
    let mut quoted = false;
    for c in q.trim().chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].clone();
        i += 1;
        let Some((key, raw)) = t.split_once(':').filter(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphabetic())) else {
            out.words.push(t.to_lowercase());
            continue;
        };
        let key = key.to_lowercase();
        let mut val = raw.trim_start_matches('=').to_lowercase();
        // "datemodified:this week" arrives as two tokens.
        if i < tokens.len() && TWO_WORDS.contains(&format!("{val} {}", tokens[i].to_lowercase()).as_str()) {
            val = format!("{val} {}", tokens[i].to_lowercase());
            i += 1;
        }
        let bad = || format!("Search doesn't understand \"{t}\".");
        match key.as_str() {
            "kind" => {
                if val == "folder" || val == "folders" {
                    out.folders = Some(true);
                } else {
                    out.folders = Some(false);
                    out.exts.extend(kind_exts(&val).ok_or_else(bad)?.iter().map(|x| x.to_string()));
                }
            }
            "ext" | "type" | "fileext" => {
                out.folders = Some(false);
                out.exts.extend(val.split([',', ';']).map(|x| x.trim().trim_start_matches('.').to_string()).filter(|x| !x.is_empty()));
            }
            "size" => out.size = Some(size_filter(&val).ok_or_else(bad)?),
            "datemodified" | "modified" | "date" => out.modified = Some(date_range(&val, now_ms, tz).ok_or_else(bad)?),
            "datecreated" | "created" => out.created = Some(date_range(&val, now_ms, tz).ok_or_else(bad)?),
            "name" | "filename" => out.words.push(val),
            // Any other property needs the Windows index.
            _ => return Err(format!("\"{t}\" needs the Windows index, which doesn't cover this location.")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, size: u64, modified: u64) -> Entry {
        let ext = name.rsplit_once('.').map(|(_, x)| x.to_lowercase()).unwrap_or_default();
        Entry { name: name.into(), is_dir: false, size, modified, created: modified, ext, attrs: 0, parent: None }
    }

    // 2026-10-12 (a Monday) 10:00 UTC, in UTC.
    const NOW: i64 = 1_791_799_200_000;

    #[test]
    fn words_and_phrases_match_names() {
        let q = parse(r#"annual "q3 report""#, NOW, 0).unwrap();
        assert_eq!(q.words, ["annual", "q3 report"]);
        assert!(q.matches(&e("Annual Q3 Report.pdf", 1, 0)));
        assert!(!q.matches(&e("annual.pdf", 1, 0)));
        assert!(!q.has_filters());
    }

    #[test]
    fn kinds_extensions_and_sizes() {
        let q = parse("kind:picture size:>1kb", NOW, 0).unwrap();
        assert!(q.matches(&e("a.JPG", 5000, 0)));
        assert!(!q.matches(&e("a.jpg", 100, 0)), "too small");
        assert!(!q.matches(&e("a.txt", 5000, 0)), "not a picture");
        let q = parse("ext:.pdf,docx size:small", NOW, 0).unwrap();
        assert!(q.matches(&e("x.docx", 20_000, 0)));
        assert!(!q.matches(&e("x.pdf", 2_000_000, 0)), "medium, not small");
        assert_eq!(parse("size:1MB..2MB", NOW, 0).unwrap().size, Some((1 << 20, 2 << 20)));
        assert_eq!(parse("size:empty", NOW, 0).unwrap().size, Some((0, 0)));
    }

    #[test]
    fn relative_and_absolute_dates() {
        let today = parse("datemodified:today", NOW, 0).unwrap().modified.unwrap();
        assert!(today.0 <= NOW as u64 && (NOW as u64) < today.1);
        let week = parse("datemodified:this week", NOW, 0).unwrap().modified.unwrap();
        assert_eq!(week.0, today.0, "Monday starts the week");
        let last = parse("modified:last month", NOW, 0).unwrap().modified.unwrap();
        assert_eq!(last, ((days_from_civil(2026, 9, 1) * DAY) as u64, (days_from_civil(2026, 10, 1) * DAY) as u64));
        let d = parse("datecreated:2024-03-05", NOW, 0).unwrap().created.unwrap();
        assert_eq!(d.1 - d.0, DAY as u64);
        assert_eq!(parse("created:5-3-2024", NOW, 0).unwrap().created, Some(d), "day first, as Windows writes it here");
        // An hour east of UTC: the local day starts an hour earlier in UTC.
        let east = parse("datemodified:today", NOW, 3_600_000).unwrap().modified.unwrap();
        assert_eq!(east.0, today.0 - 3_600_000);
    }

    #[test]
    fn unknown_filters_say_so() {
        assert!(parse("size:abc", NOW, 0).unwrap_err().contains("size:abc"));
        assert!(parse("author:smith", NOW, 0).unwrap_err().contains("index"));
    }
}
