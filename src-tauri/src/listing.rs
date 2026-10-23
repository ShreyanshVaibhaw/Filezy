//! The one folder (or search result) listing the frontend pages through.
//!
//! Row indices are only meaningful together with the listing's `rev`: every
//! install, sort, or regroup gets a new rev, and commands that name rows by index
//! are refused when their rev is no longer current. Appending search results
//! keeps the rev, since existing indices stay valid.
use crate::{paths, props};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

#[derive(Clone, Serialize)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u64,
    pub created: u64,
    pub ext: String,
    /// Raw FILE_ATTRIBUTE_* bits.
    pub attrs: u32,
    /// Set only on search results: the folder containing this entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

pub const ATTR_HIDDEN: u32 = 0x2;
pub const ATTR_SYSTEM: u32 = 0x4;
pub const ATTR_DIRECTORY: u32 = 0x10;
pub const ATTR_REPARSE: u32 = 0x400;

/// Explorer's Attributes column: R read-only, H hidden, S system, A archive,
/// C compressed, E encrypted, O offline, P pinned, U unpinned, M recall on access.
pub fn attr_letters(a: u32) -> String {
    const LETTERS: [(u32, char); 10] = [
        (0x1, 'R'),
        (0x2, 'H'),
        (0x4, 'S'),
        (0x20, 'A'),
        (0x800, 'C'),
        (0x4000, 'E'),
        (0x1000, 'O'),
        (0x80000, 'P'),
        (0x100000, 'U'),
        (0x400000, 'M'),
    ];
    LETTERS.iter().filter(|(bit, _)| a & bit != 0).map(|(_, c)| c).collect()
}

/// Windows' own sort key for a name (what Explorer's ordering is built on):
/// digit runs as numbers, case ignored, accents and other scripts in their
/// linguistic place rather than after "z".
pub fn sort_key(name: &str) -> Vec<u8> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::Globalization::{LCMapStringEx, LCMAP_SORTKEY, NORM_IGNORECASE, SORT_DIGITSASNUMBERS};
    let w: Vec<u16> = name.encode_utf16().collect();
    let flags = LCMAP_SORTKEY | NORM_IGNORECASE.0 | SORT_DIGITSASNUMBERS.0;
    unsafe {
        let n = LCMapStringEx(PCWSTR::null(), flags, &w, None, None, None, LPARAM(0));
        if n <= 0 {
            return name.to_lowercase().into_bytes();
        }
        // With LCMAP_SORTKEY the output is bytes and its size is counted in
        // bytes, while the binding passes the buffer's length in u16s: give it
        // n u16s so the length it passes is the n bytes needed.
        let mut out = vec![0u16; n as usize];
        let n = LCMapStringEx(PCWSTR::null(), flags, &w, Some(&mut out), None, None, LPARAM(0));
        let bytes = std::slice::from_raw_parts(out.as_ptr() as *const u8, n.max(0) as usize);
        bytes.to_vec()
    }
}

impl Entry {
    /// Stable identity within one listing: the name, or the full path for search results.
    pub fn key(&self) -> String {
        match &self.parent {
            Some(p) => paths::join(p, &self.name),
            None => self.name.clone(),
        }
    }
    pub fn path_in(&self, dir: &str) -> String {
        paths::join(self.parent.as_deref().unwrap_or(dir), &self.name)
    }
}

/// Explorer's visibility rules: hidden items need "Hidden items"; items that are
/// both hidden and system ("protected operating system files") also need
/// "Show protected operating system files".
pub fn visible(attrs: u32, show_hidden: bool, show_protected: bool) -> bool {
    if attrs & ATTR_HIDDEN == 0 {
        return true;
    }
    show_hidden && (attrs & ATTR_SYSTEM == 0 || show_protected)
}

#[derive(Clone, Serialize)]
pub struct Group {
    pub label: String,
    pub start: usize,
    pub count: usize,
}

#[derive(Default)]
pub struct Listing {
    pub rev: u64,
    /// Display path of the folder, or of the search root.
    pub path: String,
    pub entries: Vec<Entry>,
    /// The arrangement last applied, so streamed search results can be re-arranged.
    pub sort: String,
    pub asc: bool,
    pub group: String,
}

static REV: AtomicU64 = AtomicU64::new(0);

pub fn next_rev() -> u64 {
    REV.fetch_add(1, AtomicOrdering::SeqCst) + 1
}

/// Rows the frontend wants to keep track of across a re-list or re-sort.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Keep {
    /// Revision `idx`, `all` and `focus` refer to.
    pub rev: u64,
    /// The selection.
    pub idx: Vec<u32>,
    pub all: bool,
    /// Single rows by index, such as the anchor and an open rename; -1 for none.
    pub focus: Vec<i64>,
    /// Rows by identity key (see Entry::key), for rows known by name.
    pub names: Vec<String>,
}

/// Where kept rows ended up: `sel` for the selection (order not preserved),
/// `focus` and `names` aligned with the request, -1 when the row is gone.
#[derive(Serialize, Default)]
pub struct Kept {
    pub sel: Vec<u32>,
    pub focus: Vec<i64>,
    pub names: Vec<i64>,
}

/// A Keep resolved to identity keys against the listing it was made for.
#[derive(Default)]
pub struct KeepKeys {
    sel: Vec<String>,
    focus: Vec<Option<String>>,
}

impl Listing {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Identity keys for the selection part of `keep`, resolved against this listing.
    /// Indices from another revision resolve to nothing.
    pub fn sel_keys(&self, keep: &Keep) -> Vec<String> {
        if keep.rev != self.rev {
            return Vec::new();
        }
        if keep.all {
            self.entries.iter().map(Entry::key).collect()
        } else {
            keep.idx.iter().filter_map(|&i| self.entries.get(i as usize)).map(Entry::key).collect()
        }
    }

    pub fn keep_keys(&self, keep: &Keep) -> KeepKeys {
        let current = keep.rev == self.rev;
        KeepKeys {
            sel: self.sel_keys(keep),
            focus: keep
                .focus
                .iter()
                .map(|&i| if current && i >= 0 { self.entries.get(i as usize).map(Entry::key) } else { None })
                .collect(),
        }
    }

    /// New indices of kept rows in this listing.
    pub fn find(&self, keys: &KeepKeys, names: &[String]) -> Kept {
        if keys.sel.is_empty() && keys.focus.iter().all(Option::is_none) && names.is_empty() {
            return Kept { focus: vec![-1; keys.focus.len()], names: vec![-1; names.len()], ..Default::default() };
        }
        let at: std::collections::HashMap<String, u32> =
            self.entries.iter().enumerate().map(|(i, e)| (e.key(), i as u32)).collect();
        let pos = |k: &String| at.get(k).map_or(-1, |&i| i as i64);
        Kept {
            sel: keys.sel.iter().filter_map(|k| at.get(k).copied()).collect(),
            focus: keys.focus.iter().map(|k| k.as_ref().map_or(-1, pos)).collect(),
            names: names.iter().map(pos).collect(),
        }
    }

    /// Sort, then group, as one stable arrangement; returns the groups and starts
    /// a new revision. Property sorts (Authors, Tags...) read one property per
    /// file, so they are capped.
    pub fn arrange(&mut self, sort: &str, asc: bool, group: &str, tz_off_ms: i64) -> Result<Vec<Group>, String> {
        let prop = props::is_prop(sort).then_some(sort);
        // Status asks the shell about each cloud item, so it is capped too.
        if (prop.is_some() || sort == "status") && self.entries.len() > PROP_SORT_MAX {
            return Err(format!(
                "Too many items to sort by this property (more than {PROP_SORT_MAX}). Sorted by name instead."
            ));
        }
        let dir = self.path.clone();
        // String keys computed once per entry (a document property, the type
        // name Explorer sorts Type by, the folder, the attribute letters), so
        // grouped and ungrouped orders share one comparison.
        let pkeys: Option<Vec<Option<String>>> = match prop {
            Some(p) => Some(
                self.entries
                    .iter()
                    .map(|e| if e.is_dir { None } else { props::value(&e.path_in(&dir), e.modified, p) })
                    .collect(),
            ),
            None if sort == "type" => Some(self.entries.iter().map(|e| Some(type_label(e))).collect()),
            None if sort == "location" => {
                Some(self.entries.iter().map(|e| Some(e.parent.clone().unwrap_or_else(|| dir.clone()))).collect())
            }
            None if sort == "attributes" => Some(self.entries.iter().map(|e| Some(attr_letters(e.attrs))).collect()),
            None if sort == "status" => {
                // Only items that can have a state are asked about, all in one go:
                // all of a synced folder's (Windows hides a downloaded file's
                // placeholder bit from apps), elsewhere those whose attributes say so.
                let synced = crate::cloud::synced(&dir);
                let asked: Vec<usize> =
                    (0..self.entries.len()).filter(|&i| synced || crate::cloud::may_have_state(self.entries[i].attrs)).collect();
                let paths: Vec<String> = asked.iter().map(|&i| self.entries[i].path_in(&dir)).collect();
                let mut keys = vec![None; self.entries.len()];
                for (i, s) in asked.into_iter().zip(props::cloud_states(&paths)) {
                    keys[i] = s.map(|s| s.text);
                }
                Some(keys)
            }
            None => None,
        };
        // ponytail: Windows sort keys only when a name needs them; the byte-wise
        // compare gives the same order for plain ASCII names and costs nothing.
        let nkeys: Option<Vec<Vec<u8>>> =
            self.entries.iter().any(|e| !e.name.is_ascii()).then(|| self.entries.iter().map(|e| sort_key(&e.name)).collect());
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let buckets: Option<Vec<(u32, String)>> = (group != "none")
            .then(|| self.entries.iter().map(|e| group_bucket(e, group, now_ms, tz_off_ms)).collect());

        let mut idx: Vec<usize> = (0..self.entries.len()).collect();
        idx.sort_by(|&a, &b| {
            let g = match &buckets {
                Some(bk) => bk[a].0.cmp(&bk[b].0).then_with(|| cmp_name(&bk[a].1, &bk[b].1)),
                None => Ordering::Equal,
            };
            g.then_with(|| {
                let pk = pkeys.as_ref().map(|k| (&k[a], &k[b]));
                let nk = nkeys.as_ref().map(|k| (k[a].as_slice(), k[b].as_slice()));
                cmp_entries(&self.entries[a], &self.entries[b], sort, asc, pk, nk)
            })
        });

        let mut groups: Vec<Group> = Vec::new();
        if let Some(bk) = &buckets {
            for (pos, &i) in idx.iter().enumerate() {
                match groups.last_mut() {
                    Some(g) if g.label == bk[i].1 => g.count += 1,
                    _ => groups.push(Group { label: bk[i].1.clone(), start: pos, count: 1 }),
                }
            }
        }
        let mut old: Vec<Option<Entry>> = std::mem::take(&mut self.entries).into_iter().map(Some).collect();
        self.entries = idx.into_iter().map(|i| old[i].take().unwrap()).collect();
        self.sort = sort.to_string();
        self.asc = asc;
        self.group = group.to_string();
        self.rev = next_rev();
        Ok(groups)
    }
}

/// Property sorts stat every file; cap so a 100k system folder can't hang the sort.
pub const PROP_SORT_MAX: usize = 20_000;

/// Folders first in both directions (Explorer), then the key, then the name.
/// A blank value counts as the smallest, as in Windows' property compare
/// (PVCF_DEFAULT): first when ascending, last when descending. `nk` are
/// Windows sort keys for the two names, when the folder has names that need them.
fn cmp_entries(
    a: &Entry,
    b: &Entry,
    key: &str,
    asc: bool,
    pk: Option<(&Option<String>, &Option<String>)>,
    nk: Option<(&[u8], &[u8])>,
) -> Ordering {
    b.is_dir.cmp(&a.is_dir).then_with(|| {
        let ord = match (key, pk) {
            (_, Some((va, vb))) => match (va, vb) {
                (Some(x), Some(y)) => cmp_name(x, y),
                (Some(_), None) => Ordering::Greater,
                (None, Some(_)) => Ordering::Less,
                (None, None) => Ordering::Equal,
            },
            ("modified", _) => a.modified.cmp(&b.modified),
            ("created", _) => a.created.cmp(&b.created),
            ("size", _) => a.size.cmp(&b.size),
            _ => Ordering::Equal,
        }
        .then_with(|| match nk {
            Some((x, y)) => x.cmp(y),
            None => cmp_name(&a.name, &b.name),
        });
        if asc { ord } else { ord.reverse() }
    })
}

/// Explorer-style natural sort: case-insensitive, digit runs compare numerically
/// ("file2" < "file10"). Zero-allocation so 100k-entry sorts stay instant.
// ponytail: byte-wise natural compare; LCMapStringEx sort keys if exact shell collation of
// accented letters and punctuation matters more than the extra pass over every name
pub fn cmp_name(a: &str, b: &str) -> Ordering {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < ab.len() && j < bb.len() {
        let (ca, cb) = (ab[i], bb[j]);
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let (si, sj) = (i, j);
            while i < ab.len() && ab[i].is_ascii_digit() {
                i += 1;
            }
            while j < bb.len() && bb[j].is_ascii_digit() {
                j += 1;
            }
            // Compare numerically: strip leading zeros, then longer run wins, then lexical.
            let (mut pa, mut pb) = (si, sj);
            while pa + 1 < i && ab[pa] == b'0' {
                pa += 1;
            }
            while pb + 1 < j && bb[pb] == b'0' {
                pb += 1;
            }
            let ord = (i - pa).cmp(&(j - pb)).then_with(|| ab[pa..i].cmp(&bb[pb..j]));
            if ord != Ordering::Equal {
                return ord;
            }
        } else {
            let (la, lb) = (ca.to_ascii_lowercase(), cb.to_ascii_lowercase());
            if la != lb {
                return la.cmp(&lb);
            }
            i += 1;
            j += 1;
        }
    }
    (ab.len() - i).cmp(&(bb.len() - j))
}

/// Days-since-unix-epoch -> (year, month, day). Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (rank, label) bucket for an entry under a grouping key, matching Explorer's buckets.
/// `tz_off_ms` shifts UTC timestamps to the user's local time so date buckets fall on
/// calendar-day boundaries (passed from the frontend, DST-correct for the current date).
pub fn group_bucket(e: &Entry, key: &str, now_ms: u64, tz_off_ms: i64) -> (u32, String) {
    match key {
        "name" => {
            let c = e.name.chars().next().unwrap_or('#').to_ascii_uppercase();
            match c {
                '0'..='9' => (0, "0 - 9".into()),
                'A'..='H' => (1, "A - H".into()),
                'I'..='P' => (2, "I - P".into()),
                'Q'..='Z' => (3, "Q - Z".into()),
                _ => (4, "Other".into()),
            }
        }
        "modified" | "created" => {
            const DAY: i64 = 86_400_000;
            let stamp = if key == "created" { e.created } else { e.modified };
            let now_day = (now_ms as i64 + tz_off_ms).div_euclid(DAY);
            let mod_day = (stamp as i64 + tz_off_ms).div_euclid(DAY).min(now_day);
            // Explorer's buckets: Today, Yesterday, Earlier this week, Last week,
            // Earlier this month, Last month, Earlier this year, A long time ago.
            let weekday = (now_day + 4).rem_euclid(7); // 0 = Sunday (1970-01-01 was a Thursday)
            let week_start = now_day - (weekday + 6) % 7; // weeks start Monday
            let (ny, nm, _) = civil_from_days(now_day);
            let (my, mm, _) = civil_from_days(mod_day);
            let last_month = if nm == 1 { (ny - 1, 12) } else { (ny, nm - 1) };
            if mod_day == now_day {
                (0, "Today".into())
            } else if mod_day == now_day - 1 {
                (1, "Yesterday".into())
            } else if mod_day >= week_start {
                (2, "Earlier this week".into())
            } else if mod_day >= week_start - 7 {
                (3, "Last week".into())
            } else if (my, mm) == (ny, nm) {
                (4, "Earlier this month".into())
            } else if (my, mm) == last_month {
                (5, "Last month".into())
            } else if my == ny {
                (6, "Earlier this year".into())
            } else {
                (7, "A long time ago".into())
            }
        }
        // Gallery: one group per calendar month of the modified date, newest first.
        "month" => {
            const MONTHS: [&str; 12] =
                ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
            let (y, m, _) = civil_from_days((e.modified as i64 + tz_off_ms).div_euclid(86_400_000));
            (u32::MAX - (y as u32 * 12 + m), format!("{} {y}", MONTHS[m as usize - 1]))
        }
        "size" => {
            if e.is_dir {
                return (0, "Unspecified".into());
            }
            const KB: u64 = 1024;
            const MB: u64 = KB * 1024;
            const GB: u64 = MB * 1024;
            match e.size {
                0 => (1, "Empty".into()),
                s if s <= 16 * KB => (2, "Tiny".into()),
                s if s <= MB => (3, "Small".into()),
                s if s <= 128 * MB => (4, "Medium".into()),
                s if s <= GB => (5, "Large".into()),
                s if s <= 4 * GB => (6, "Huge".into()),
                _ => (7, "Gigantic".into()),
            }
        }
        // type: one rank, ordered by label
        _ => (0, type_label(e)),
    }
}

/// The Type column's text: the registered type name, or Explorer's "XYZ File" fallback.
pub fn type_label(e: &Entry) -> String {
    let label = crate::icons::ext_meta(&e.ext, e.is_dir).type_name;
    if !label.is_empty() {
        label
    } else if e.is_dir {
        "File folder".into()
    } else if e.ext.is_empty() {
        "File".into()
    } else {
        format!("{} File", e.ext.to_uppercase())
    }
}

/// Whether `name` starts with `lower`, a lowercase prefix, ignoring case,
/// without building a lowercase copy of the name.
pub fn starts_with_lower(name: &str, lower: &str) -> bool {
    let mut chars = name.chars().flat_map(char::to_lowercase);
    lower.chars().all(|c| chars.next() == Some(c))
}

/// Whether `name` contains `lower`, a lowercase word, ignoring case. An ASCII
/// name is compared in place; only other names are lowercased first.
pub fn contains_lower(name: &str, lower: &str) -> bool {
    if !name.is_ascii() {
        return name.to_lowercase().contains(lower);
    }
    let (name, word) = (name.as_bytes(), lower.as_bytes());
    word.is_empty() || name.windows(word.len()).any(|w| w.eq_ignore_ascii_case(word))
}

#[cfg(test)]
mod tests {
    #[test]
    fn case_blind_matches() {
        use super::{contains_lower, starts_with_lower};
        assert!(starts_with_lower("Report.TXT", "rep"));
        assert!(starts_with_lower("Ärger.txt", "är"));
        assert!(!starts_with_lower("Re", "rep"));
        assert!(starts_with_lower("anything", ""));
        assert!(contains_lower("My Report.TXT", "report.t"));
        assert!(contains_lower("ÜBER Bericht", "über"));
        assert!(!contains_lower("Report", "über"));
        assert!(!contains_lower("ab", "abc"));
        assert!(contains_lower("ab", ""));
    }

    use super::*;

    fn e(name: &str, is_dir: bool, size: u64, modified: u64) -> Entry {
        let ext = if is_dir { String::new() } else { name.rsplit_once('.').map(|(_, x)| x.to_lowercase()).unwrap_or_default() };
        Entry { name: name.into(), is_dir, size, modified, created: modified, ext, attrs: 0, parent: None }
    }

    fn names(l: &Listing) -> Vec<&str> {
        l.entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn accented_names_sort_where_windows_puts_them() {
        let mut l = Listing { entries: ["Zebra.txt", "école.txt", "apple.txt", "File10.txt", "file2.txt"].map(|n| e(n, false, 0, 0)).to_vec(), ..Default::default() };
        l.arrange("name", true, "none", 0).unwrap();
        assert_eq!(names(&l), ["apple.txt", "école.txt", "file2.txt", "File10.txt", "Zebra.txt"]);
    }

    #[test]
    fn attribute_letters_read_like_explorer() {
        assert_eq!(attr_letters(0x1 | 0x20), "RA");
        assert_eq!(attr_letters(0x2 | 0x4 | 0x20), "HSA");
        assert_eq!(attr_letters(0), "");
    }

    #[test]
    fn natural_names() {
        let mut v = vec!["file10", "File3", "file2", "file1", "file100", "a01", "a1", "b"];
        v.sort_by(|a, b| cmp_name(a, b));
        assert_eq!(v, ["a01", "a1", "b", "file1", "file2", "File3", "file10", "file100"]);
    }

    #[test]
    fn folders_first_both_directions() {
        let mut l = Listing { entries: vec![e("b.txt", false, 5, 0), e("Z", true, 0, 0), e("a.txt", false, 9, 0)], ..Default::default() };
        l.arrange("size", true, "none", 0).unwrap();
        assert_eq!(names(&l), ["Z", "b.txt", "a.txt"]);
        l.arrange("size", false, "none", 0).unwrap();
        assert_eq!(names(&l), ["Z", "a.txt", "b.txt"]);
    }

    #[test]
    fn arrange_groups_and_bumps_rev() {
        let mut l = Listing {
            entries: vec![e("q.txt", false, 0, 0), e("b.txt", false, 0, 0), e("9.txt", false, 0, 0), e("a.txt", false, 0, 0)],
            ..Default::default()
        };
        let r0 = l.rev;
        let groups = l.arrange("name", false, "name", 0).unwrap();
        assert!(l.rev > r0);
        assert_eq!(names(&l), ["9.txt", "b.txt", "a.txt", "q.txt"]);
        let g: Vec<(&str, usize, usize)> = groups.iter().map(|g| (g.label.as_str(), g.start, g.count)).collect();
        assert_eq!(g, [("0 - 9", 0, 1), ("A - H", 1, 2), ("Q - Z", 3, 1)]);
    }

    #[test]
    fn keep_follows_items_across_revisions() {
        let mut l = Listing { entries: vec![e("c", false, 1, 0), e("a", false, 3, 0), e("b", false, 2, 0)], ..Default::default() };
        l.arrange("name", true, "none", 0).unwrap(); // a b c
        let keep = Keep {
            rev: l.rev,
            idx: vec![0, 2],
            focus: vec![1, -1],
            names: vec!["b".into(), "gone".into()],
            ..Default::default()
        };
        let keys = l.keep_keys(&keep);
        l.arrange("size", true, "none", 0).unwrap(); // c b a
        let kept = l.find(&keys, &keep.names);
        let mut sel = kept.sel.clone();
        sel.sort();
        assert_eq!(sel, [0, 2]); // c and a
        assert_eq!(kept.focus, [1, -1]); // b stays in the middle
        assert_eq!(kept.names, [1, -1]);
        // Indices from an older revision resolve to nothing.
        assert!(l.sel_keys(&Keep { rev: l.rev - 1, idx: vec![0], ..Default::default() }).is_empty());
    }

    #[test]
    fn search_results_are_keyed_by_path() {
        let mut a = e("same.txt", false, 0, 0);
        a.parent = Some(r"C:\x".into());
        let mut b = e("same.txt", false, 0, 0);
        b.parent = Some(r"C:\y".into());
        assert_ne!(a.key(), b.key());
        assert_eq!(b.path_in(r"C:\ignored"), r"C:\y\same.txt");
    }

    #[test]
    fn visibility_rules() {
        assert!(visible(0, false, false));
        assert!(visible(ATTR_SYSTEM, false, false));
        assert!(!visible(ATTR_HIDDEN, false, false));
        assert!(visible(ATTR_HIDDEN, true, false));
        assert!(!visible(ATTR_HIDDEN | ATTR_SYSTEM, true, false));
        assert!(visible(ATTR_HIDDEN | ATTR_SYSTEM, true, true));
    }

    #[test]
    fn date_buckets() {
        const DAY: u64 = 86_400_000;
        let now = 20_000 * DAY + 12 * 3_600_000; // a Friday, noon UTC
        let at = |days_ago: u64| group_bucket(&e("x", false, 0, now - days_ago * DAY), "modified", now, 0).1;
        assert_eq!(at(0), "Today");
        assert_eq!(at(1), "Yesterday");
        assert_eq!(at(3), "Earlier this week");
        assert_eq!(at(7), "Last week");
        assert_eq!(at(4000), "A long time ago");
        let month = |days_ago: u64| group_bucket(&e("x", false, 0, now - days_ago * DAY), "month", now, 0);
        assert_eq!(month(0).1, "October 2024");
        assert_eq!(month(31).1, "September 2024");
        assert!(month(0).0 < month(31).0, "newest month first");
    }
}
