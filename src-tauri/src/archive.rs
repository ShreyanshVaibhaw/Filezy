//! Archives as places: a ZIP, 7z or tar listed like a folder, one file opened
//! from a read-only copy, safe extraction, and ZIP creation.
//!
//! An archive location is the archive's path, "|", and a folder inside it
//! ("C:\a.zip|docs\2024"; the root is "C:\a.zip|"). "|" can't occur in a
//! Windows path, so no file system call can mistake one for a real file, and
//! everything that handles one has to know it is in an archive.
//!
//! Entry names are untrusted. Listing shows them as stored; extraction writes
//! only names that stay inside the destination, valid on Windows, and never
//! creates links.
use crate::fs_list::{filetime_ms, wide};
use crate::listing::{Entry, ATTR_DIRECTORY};
use crate::paths;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use windows::core::PCWSTR;

pub const SEP: char = '|';

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Zip,
    SevenZ,
    Tar,
    TarGz,
}

/// The archive formats Filezy opens, by extension.
pub fn kind_of(path: &str) -> Option<Kind> {
    let l = path.to_lowercase();
    if l.ends_with(".zip") {
        Some(Kind::Zip)
    } else if l.ends_with(".7z") {
        Some(Kind::SevenZ)
    } else if l.ends_with(".tar.gz") || l.ends_with(".tgz") {
        Some(Kind::TarGz)
    } else if l.ends_with(".tar") {
        Some(Kind::Tar)
    } else {
        None
    }
}

/// "C:\a.zip|docs\2024" -> ("C:\a.zip", "docs\2024"); None for a plain path.
pub fn split(loc: &str) -> Option<(&str, &str)> {
    let (file, inner) = loc.split_once(SEP)?;
    Some((file, inner.trim_matches('\\')))
}

pub fn location(file: &str, inner: &str) -> String {
    format!("{file}{SEP}{inner}")
}

/// Why an entry is never written out.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Problem {
    /// "../x" or "a/../../x": a name that climbs out of the folder.
    Outside,
    /// "C:\x", "/x" or "\\server\share\x".
    Absolute,
    /// A symbolic or hard link.
    Link,
    /// A device, pipe or other special tar entry.
    Special,
}

impl Problem {
    pub fn says(self) -> &'static str {
        match self {
            Problem::Outside => "its name leads outside the folder it would go to",
            Problem::Absolute => "its name is a full path rather than one inside the archive",
            Problem::Link => "it is a link, and Filezy doesn't create links from archives",
            Problem::Special => "it isn't a file or a folder",
        }
    }
}

/// One entry, as the archive lists it.
#[derive(Clone, Debug)]
pub struct Item {
    /// Inside the archive, components joined with '\'. Hostile names lose
    /// the parts that point elsewhere and keep a `problem`.
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// Milliseconds since 1970 (UTC); 0 when unknown.
    pub modified: u64,
    pub encrypted: bool,
    pub problem: Option<Problem>,
    /// Position in the archive; None for a folder only implied by the
    /// entries inside it ("a/b.txt" with no "a/").
    pub index: Option<usize>,
}

impl Item {
    pub fn name(&self) -> &str {
        self.path.rsplit('\\').next().unwrap_or(&self.path)
    }
}

pub struct Index {
    pub kind: Kind,
    pub items: Vec<Item>,
    /// Lowercased folder path ("" for the root) -> its children.
    children: HashMap<String, Vec<usize>>,
    /// Lowercased path -> item (a later duplicate wins, as when extracting).
    by_path: HashMap<String, usize>,
}

impl Index {
    fn new(kind: Kind, raw: Vec<Item>) -> Index {
        let mut idx = Index { kind, items: Vec::new(), children: HashMap::new(), by_path: HashMap::new() };
        for it in raw {
            idx.add(it);
        }
        idx
    }

    fn add(&mut self, it: Item) {
        if it.path.is_empty() {
            return;
        }
        let key = it.path.to_lowercase();
        // Folders inside it may have been implied already; a real entry for
        // the folder keeps that slot and fills in its date.
        if let Some(&i) = self.by_path.get(&key) {
            if self.items[i].is_dir && it.is_dir {
                if it.index.is_some() {
                    self.items[i] = it;
                }
                return;
            }
        }
        let parent = match it.path.rfind('\\') {
            Some(p) => it.path[..p].to_string(),
            None => String::new(),
        };
        if !parent.is_empty() && !self.by_path.get(&parent.to_lowercase()).is_some_and(|&i| self.items[i].is_dir) {
            self.add(Item { path: parent.clone(), is_dir: true, size: 0, modified: 0, encrypted: false, problem: None, index: None });
        }
        let i = self.items.len();
        self.items.push(it);
        self.children.entry(parent.to_lowercase()).or_default().push(i);
        self.by_path.insert(key, i);
    }

    pub fn find(&self, path: &str) -> Option<usize> {
        self.by_path.get(&path.trim_matches('\\').to_lowercase()).copied()
    }

    pub fn get(&self, path: &str) -> Option<&Item> {
        self.find(path).map(|i| &self.items[i])
    }

    pub fn is_dir(&self, path: &str) -> bool {
        path.trim_matches('\\').is_empty() || self.get(path).is_some_and(|i| i.is_dir)
    }

    pub fn children(&self, dir: &str) -> impl Iterator<Item = &Item> {
        self.children.get(&dir.trim_matches('\\').to_lowercase()).into_iter().flatten().map(|&i| &self.items[i])
    }

}

/// An entry's stored name, split into components. Leading roots, drive
/// letters and ".." are taken out and reported; empty and "." parts dropped.
fn normalize(raw: &str) -> (String, Option<Problem>) {
    let s = raw.replace('/', "\\");
    let mut problem = None;
    let mut rest = s.as_str();
    if rest.starts_with("\\\\") {
        // \\server\share\x: the share is a place, not a folder in the archive.
        problem = Some(Problem::Absolute);
        rest = rest.trim_start_matches('\\');
        for _ in 0..2 {
            rest = rest.split_once('\\').map(|(_, r)| r).unwrap_or("");
        }
    } else if rest.len() >= 2
        && rest.as_bytes()[1] == b':'
        && rest.as_bytes()[0].is_ascii_alphabetic()
        && (rest.len() == 2 || rest.as_bytes()[2] == b'\\')
    {
        // "C:\x" or "C:"; "a:b.txt" is a name with a colon in it.
        problem = Some(Problem::Absolute);
        rest = &rest[2..];
    }
    if rest.starts_with('\\') {
        problem = Some(Problem::Absolute);
    }
    let mut parts = Vec::new();
    for c in rest.split('\\') {
        match c {
            "" | "." => {}
            ".." => problem = problem.or(Some(Problem::Outside)),
            _ => parts.push(c),
        }
    }
    (parts.join("\\"), problem)
}

/// A name valid on Windows: reserved characters become "_", as do trailing
/// dots and spaces (Windows would drop them); device names get a "_" in front.
pub fn sanitize(name: &str) -> String {
    let mut out: String = name.chars().map(|c| if c < ' ' || "<>:\"|?*".contains(c) { '_' } else { c }).collect();
    let kept = out.trim_end_matches(['.', ' ']).len();
    if kept < out.len() {
        let tail = out.len() - kept;
        out.truncate(kept);
        out.push_str(&"_".repeat(tail));
    }
    let stem = out.split('.').next().unwrap_or("").trim_end().to_uppercase();
    const DEVICES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
    let numbered = |p: &str| {
        stem.chars().count() == 4 && stem.starts_with(p) && stem.chars().nth(3).is_some_and(|c| c.is_ascii_digit() || "¹²³".contains(c))
    };
    if DEVICES.contains(&stem.as_str()) || numbered("COM") || numbered("LPT") || stem.starts_with("CONIN$") || stem.starts_with("CONOUT$") {
        out.insert(0, '_');
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

/// A ZIP name: UTF-8 when it is valid UTF-8 (flagged or plain ASCII), else the
/// system's OEM code page, which is what Windows' own zip folder assumes.
fn decode_name(raw: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(raw) {
        return s.to_string();
    }
    use windows::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};
    let mut buf = vec![0u16; raw.len() * 2 + 1];
    let n = unsafe { MultiByteToWideChar(CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), raw, Some(&mut buf)) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// A ZIP or DOS timestamp (local time, as Windows wrote it) as UTC milliseconds.
fn local_ms(y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> u64 {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{SystemTimeToFileTime, TzSpecificLocalTimeToSystemTime};
    let local = SYSTEMTIME { wYear: y, wMonth: mo as u16, wDay: d as u16, wHour: h as u16, wMinute: mi as u16, wSecond: s as u16, ..Default::default() };
    let mut utc = SYSTEMTIME::default();
    let mut ft = FILETIME::default();
    unsafe {
        if TzSpecificLocalTimeToSystemTime(None, &local, &mut utc).is_err() || SystemTimeToFileTime(&utc, &mut ft).is_err() {
            return 0;
        }
    }
    filetime_ms((ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64)
}

fn read_zip(file: &Path) -> Result<Vec<Item>, String> {
    let mut z = zip::ZipArchive::new(BufReader::new(File::open(file).map_err(|e| e.to_string())?)).map_err(damaged)?;
    let mut out = Vec::with_capacity(z.len());
    for i in 0..z.len() {
        // Raw access reads an encrypted entry's details without its password.
        let e = z.by_index_raw(i).map_err(damaged)?;
        let (path, mut problem) = normalize(&decode_name(e.name_raw()));
        if e.is_symlink() {
            problem = Some(Problem::Link);
        }
        let modified = e
            .extra_data_fields()
            .find_map(|f| match f {
                zip::extra_fields::ExtraField::ExtendedTimestamp(t) => t.mod_time().map(|s| s as u64 * 1000),
                _ => None,
            })
            .or_else(|| {
                e.last_modified()
                    .map(|t| local_ms(t.year(), t.month(), t.day(), t.hour(), t.minute(), t.second()))
            })
            .unwrap_or(0);
        out.push(Item { path, is_dir: e.is_dir(), size: e.size(), modified, encrypted: e.encrypted(), problem, index: Some(i) });
    }
    Ok(out)
}

fn read_7z(file: &Path) -> Result<Vec<Item>, String> {
    let a = sevenz_rust2::Archive::open(file).map_err(|e| match e {
        sevenz_rust2::Error::PasswordRequired => "This archive is protected with a password, and Filezy can't open protected 7z archives yet.".to_string(),
        e => damaged(e),
    })?;
    Ok(a.files
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let (path, mut problem) = normalize(&e.name);
            // 7-Zip keeps a Unix link's mode in the high word, or marks a
            // Windows link as a reparse point.
            let attrs = e.windows_attributes;
            let unix_link = attrs & 0x8000 != 0 && (attrs >> 16) & 0o170000 == 0o120000;
            if e.has_windows_attributes && (attrs & 0x400 != 0 || unix_link) {
                problem = Some(Problem::Link);
            }
            let modified = if e.has_last_modified_date { filetime_ms(e.last_modified_date.into()) } else { 0 };
            Item { path, is_dir: e.is_directory, size: e.size, modified, encrypted: false, problem, index: Some(i) }
        })
        .collect())
}

fn tar_reader(file: &Path, kind: Kind) -> Result<tar::Archive<Box<dyn Read>>, String> {
    let f = BufReader::new(File::open(file).map_err(|e| e.to_string())?);
    let r: Box<dyn Read> = if kind == Kind::TarGz { Box::new(flate2::read::GzDecoder::new(f)) } else { Box::new(f) };
    Ok(tar::Archive::new(r))
}

fn read_tar(file: &Path, kind: Kind) -> Result<Vec<Item>, String> {
    use tar::EntryType;
    let mut t = tar_reader(file, kind)?;
    let mut out = Vec::new();
    for (i, e) in t.entries().map_err(damaged)?.enumerate() {
        let e = e.map_err(damaged)?;
        let h = e.header();
        let raw = e.path().map_err(damaged)?.to_string_lossy().to_string();
        let (path, mut problem) = normalize(&raw);
        let ty = h.entry_type();
        let is_dir = ty == EntryType::Directory;
        if matches!(ty, EntryType::Symlink | EntryType::Link) {
            problem = Some(Problem::Link);
        } else if !is_dir && !matches!(ty, EntryType::Regular | EntryType::Continuous) {
            problem = Some(Problem::Special);
        }
        let modified = h.mtime().map(|s| s * 1000).unwrap_or(0);
        out.push(Item { path, is_dir, size: h.size().unwrap_or(0), modified, encrypted: false, problem, index: Some(i) });
    }
    Ok(out)
}

fn damaged(e: impl std::fmt::Display) -> String {
    format!("This archive is damaged or isn't in the format its name says ({e}).")
}

struct Cached {
    file: String,
    stamp: (u64, u64),
    index: Arc<Index>,
}

/// The last few archives' indexes. Listing a tar means reading all of it, so
/// moving between its folders must not do that again.
static CACHE: Mutex<Vec<Cached>> = Mutex::new(Vec::new());

/// The archive's entries, from the cache while the file is unchanged.
pub fn index(file: &str) -> Result<Arc<Index>, String> {
    let kind = kind_of(file).ok_or_else(|| format!("Filezy can't open '{}' as an archive.", paths::file_name(file)))?;
    let stamp = crate::ops::stamp(file);
    let key = file.to_lowercase();
    {
        let mut c = CACHE.lock().unwrap();
        if let Some(i) = c.iter().position(|x| x.file == key) {
            if c[i].stamp == stamp {
                let hit = c.remove(i);
                let index = hit.index.clone();
                c.push(hit);
                return Ok(index);
            }
            c.remove(i);
        }
    }
    let native = PathBuf::from(paths::to_native(file));
    let raw = match kind {
        Kind::Zip => read_zip(&native),
        Kind::SevenZ => read_7z(&native),
        Kind::Tar | Kind::TarGz => read_tar(&native, kind),
    }?;
    let index = Arc::new(Index::new(kind, raw));
    let mut c = CACHE.lock().unwrap();
    c.push(Cached { file: key, stamp, index: index.clone() });
    if c.len() > 4 {
        c.remove(0);
    }
    Ok(index)
}

/// A folder inside an archive, as listing rows.
pub fn list(file: &str, inner: &str) -> Result<Vec<Entry>, String> {
    let idx = index(file)?;
    if !idx.is_dir(inner) {
        return Err(format!("'{}' isn't a folder in '{}'.", inner, paths::file_name(file)));
    }
    Ok(idx.children(inner).map(entry).collect())
}

/// An entry as a listing row.
pub fn entry(it: &Item) -> Entry {
    let name = it.name().to_string();
    let ext = if it.is_dir { String::new() } else { name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default() };
    Entry {
        name,
        is_dir: it.is_dir,
        size: if it.is_dir { 0 } else { it.size },
        modified: it.modified,
        created: 0,
        ext,
        attrs: if it.is_dir { ATTR_DIRECTORY } else { 0 },
        parent: None,
    }
}

/// The real-case inner path of a folder, or None if there is no such folder.
pub fn folder(file: &str, inner: &str) -> Result<Option<String>, String> {
    let idx = index(file)?;
    let inner = inner.trim_matches('\\');
    if inner.is_empty() {
        return Ok(Some(String::new()));
    }
    Ok(idx.get(inner).filter(|i| i.is_dir).map(|i| i.path.clone()))
}

/// A typed path that runs through an archive ("C:\a.zip\docs"): the archive
/// and the folder inside it. None when no part of the path is an archive.
pub fn through(display: &str) -> Option<(String, String)> {
    let mut file = display.trim_end_matches('\\');
    let mut inner: Vec<&str> = Vec::new();
    while let Some((up, last)) = file.rsplit_once('\\') {
        if kind_of(file).is_some() && paths::attributes(file).is_ok_and(|a| a & ATTR_DIRECTORY == 0) {
            inner.reverse();
            return Some((file.to_string(), inner.join("\\")));
        }
        inner.push(last);
        file = up;
    }
    None
}

// ---- extraction ----

/// What extracting a selection will write, decided before anything is.
pub struct Plan {
    /// (item, output path relative to the destination, sanitized).
    pub files: Vec<(usize, String)>,
    pub dirs: Vec<String>,
    /// (stored path, why it isn't extracted).
    pub skipped: Vec<(String, String)>,
    /// Names changed to be valid on Windows.
    pub renamed: usize,
    pub bytes: u64,
    /// The top-level names written, for the result and undo.
    pub top: Vec<String>,
}

/// Plan extracting `picked` (inner paths, files or folders, or "" for
/// everything) relative to `base`, the folder they were picked in.
pub fn plan(idx: &Index, base: &str, picked: &[String]) -> Plan {
    let base = base.trim_matches('\\').to_lowercase();
    let mut plan = Plan { files: vec![], dirs: vec![], skipped: vec![], renamed: 0, bytes: 0, top: vec![] };
    let picked: std::collections::HashSet<String> = picked.iter().map(|p| p.trim_matches('\\').to_lowercase()).collect();
    let everything = picked.contains("");
    let depth = if base.is_empty() { 0 } else { base.split('\\').count() };
    // One pass: an item is in when it, or a folder it is in, was picked.
    for (i, it) in idx.items.iter().enumerate() {
        let lower = it.path.to_lowercase();
        let mut at = lower.as_str();
        let mut hit = everything || picked.contains(at);
        while !hit {
            let Some(k) = at.rfind('\\') else { break };
            at = &at[..k];
            hit = picked.contains(at);
        }
        if !hit {
            continue;
        }
        if let Some(why) = it.problem {
            plan.skipped.push((it.path.clone(), why.says().to_string()));
            continue;
        }
        if it.encrypted {
            plan.skipped.push((it.path.clone(), "it is protected with a password, and Filezy can't extract protected entries yet".into()));
            continue;
        }
        // The part after the folder the items were picked in, cut by
        // components: lowercasing can change a name's length in bytes.
        let rel = if base.is_empty() {
            it.path.as_str()
        } else {
            let mut parts = it.path.splitn(depth + 1, '\\');
            let head: Vec<&str> = parts.by_ref().take(depth).collect();
            match parts.next() {
                Some(rest) if head.join("\\").to_lowercase() == base => rest,
                _ => continue,
            }
        };
        let parts: Vec<String> = rel.split('\\').map(sanitize).collect();
        if parts.iter().zip(rel.split('\\')).any(|(a, b)| a != b) {
            plan.renamed += 1;
        }
        let out = parts.join("\\");
        if !plan.top.iter().any(|t: &String| t.eq_ignore_ascii_case(&parts[0])) {
            plan.top.push(parts[0].clone());
        }
        if it.is_dir {
            plan.dirs.push(out);
        } else {
            plan.bytes += it.size;
            plan.files.push((i, out));
        }
    }
    plan
}

/// Free space on the volume holding `dir`.
fn free_space(dir: &str) -> Option<u64> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let w = wide(&paths::to_native(dir));
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}

/// Asked as work goes on: (bytes done, bytes in all, the file being written).
/// False stops the work.
pub type Progress<'a> = &'a mut dyn FnMut(u64, u64, &str) -> bool;

/// A pause for every 256 KB written, only for the E2E tests
/// (FILEZY_E2E_ARCHIVE_DELAY_MS), so a small archive takes long enough to
/// show the progress dialog and be cancelled.
fn e2e_pace(written: usize) {
    static MS: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let ms = *MS.get_or_init(|| crate::e2e_knob("ARCHIVE_DELAY_MS").unwrap_or(0));
    if ms == 0 {
        return;
    }
    thread_local!(static OWED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) });
    OWED.with(|owed| {
        let mut n = owed.get() + written;
        while n >= 256 * 1024 {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            n -= 256 * 1024;
        }
        owed.set(n);
    });
}

pub const CANCELLED: &str = "cancelled";

/// Write the plan's files and folders under `into` (an existing folder).
pub fn extract(file: &str, idx: &Index, plan: &Plan, into: &str, progress: Progress) -> Result<(), String> {
    if let Some(free) = free_space(into) {
        if plan.bytes > free {
            return Err(format!(
                "There isn't enough room on {} for this: it needs {}, and {} is free.",
                paths::root(into).unwrap_or_else(|| into.to_string()),
                size_text(plan.bytes),
                size_text(free)
            ));
        }
    }
    let root = PathBuf::from(paths::to_native(into));
    for d in &plan.dirs {
        std::fs::create_dir_all(root.join(d)).map_err(|e| format!("Couldn't create '{d}': {e}"))?;
    }
    // Archive position -> (where it goes, its date).
    let mut want: HashMap<usize, (&str, u64)> = HashMap::new();
    for (i, out) in &plan.files {
        if let Some(n) = idx.items[*i].index {
            want.insert(n, (out.as_str(), idx.items[*i].modified));
        }
    }
    let mut done = 0u64;
    let total = plan.bytes;
    let native = PathBuf::from(paths::to_native(file));
    let mut write = |n: usize, r: &mut dyn Read| -> Result<(), String> {
        let Some(&(out, modified)) = want.get(&n) else { return Ok(()) };
        let target = root.join(out);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Couldn't create the folder for '{out}': {e}"))?;
        }
        let result = (|| -> Result<(), String> {
            let mut w = BufWriter::new(File::create(&target).map_err(|e| format!("Couldn't write '{out}': {e}"))?);
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                let n = r.read(&mut buf).map_err(|e| format!("Couldn't read '{out}' from the archive: {e}"))?;
                if n == 0 {
                    break;
                }
                w.write_all(&buf[..n]).map_err(|e| format!("Couldn't write '{out}': {e}"))?;
                e2e_pace(n);
                done += n as u64;
                if !progress(done, total, out) {
                    return Err(CANCELLED.into());
                }
            }
            w.flush().map_err(|e| format!("Couldn't write '{out}': {e}"))?;
            Ok(())
        })();
        if result.is_err() {
            // A partial file is worse than none.
            let _ = std::fs::remove_file(&target);
            return result;
        }
        if modified > 0 {
            set_modified(&target, modified);
        }
        Ok(())
    };
    match idx.kind {
        Kind::Zip => {
            let mut z = zip::ZipArchive::new(BufReader::new(File::open(&native).map_err(|e| e.to_string())?)).map_err(damaged)?;
            for (i, _) in &plan.files {
                let it = &idx.items[*i];
                let Some(n) = it.index else { continue };
                let mut e = z.by_index(n).map_err(damaged)?;
                write(n, &mut e)?;
            }
        }
        Kind::SevenZ => {
            let mut r = sevenz_rust2::ArchiveReader::open(&native, sevenz_rust2::Password::empty()).map_err(damaged)?;
            // Entries come in the order their data is stored, not the order
            // they are listed in, so they are matched by stored name (a
            // repeated name takes its positions in turn).
            let mut by_name: HashMap<String, std::collections::VecDeque<usize>> = HashMap::new();
            for (n, f) in r.archive().files.iter().enumerate() {
                if want.contains_key(&n) {
                    by_name.entry(f.name.clone()).or_default().push_back(n);
                }
            }
            let mut failed = None;
            r.for_each_entries(|e, rd| {
                if let Some(n) = by_name.get_mut(&e.name).and_then(|q| q.pop_front()) {
                    if let Err(err) = write(n, rd) {
                        failed = Some(err);
                        return Ok(false);
                    }
                }
                Ok(true)
            })
            .map_err(damaged)?;
            if let Some(e) = failed {
                return Err(e);
            }
        }
        Kind::Tar | Kind::TarGz => {
            let mut t = tar_reader(&native, idx.kind)?;
            for (n, e) in t.entries().map_err(damaged)?.enumerate() {
                let mut e = e.map_err(damaged)?;
                if want.contains_key(&n) {
                    write(n, &mut e)?;
                }
            }
        }
    }
    Ok(())
}

fn set_modified(path: &Path, ms: u64) {
    if let Ok(f) = File::options().write(true).open(path) {
        let _ = f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms));
    }
}

pub fn size_text(b: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{b} bytes") } else { format!("{v:.1} {}", UNITS[u]) }
}

/// Something an extraction or compression put on disk, for the activity list
/// and for undo.
#[derive(serde::Serialize)]
pub struct Made {
    pub path: String,
    pub size: u64,
    pub modified: u64,
    /// It took the place of an item that was already there.
    pub replaced: bool,
}

#[derive(serde::Serialize)]
pub struct Skip {
    pub name: String,
    pub why: String,
}

#[derive(serde::Serialize, Default)]
pub struct Extracted {
    pub dest: String,
    pub made: Vec<Made>,
    pub skipped: Vec<Skip>,
    /// Names changed to be valid on Windows.
    pub renamed: usize,
    /// Items left as they were when Windows asked about a name already in use.
    pub kept: usize,
    pub cancelled: bool,
    pub error: Option<String>,
}

fn made(path: &str, replaced: bool) -> Made {
    let (size, modified) = crate::ops::stamp(path);
    Made { path: path.to_string(), size, modified, replaced }
}

/// Extract `picked` (inner paths picked in the folder `base`; "" for
/// everything) into `dest`. A new folder is written directly; into an
/// existing one, the items are written beside it first and then moved in by
/// Windows, which asks about names already in use.
pub fn extract_to(file: &str, base: &str, picked: &[String], dest: &str, owner: isize, progress: Progress) -> Extracted {
    let mut out = Extracted { dest: dest.to_string(), ..Default::default() };
    let idx = match index(file) {
        Ok(i) => i,
        Err(e) => {
            out.error = Some(e);
            return out;
        }
    };
    let plan = plan(&idx, base, picked);
    out.skipped = plan.skipped.iter().map(|(name, why)| Skip { name: name.clone(), why: why.clone() }).collect();
    out.renamed = plan.renamed;
    if plan.files.is_empty() && plan.dirs.is_empty() {
        out.error = Some(if plan.skipped.is_empty() { "There's nothing in it to extract." } else { "Nothing in it can be extracted." }.into());
        return out;
    }
    let fail = |out: &mut Extracted, e: String| {
        if e == CANCELLED {
            out.cancelled = true;
        } else {
            out.error = Some(e);
        }
    };
    if paths::attributes(dest).is_err() {
        if let Err(e) = std::fs::create_dir_all(paths::to_native(dest)) {
            out.error = Some(format!("Couldn't create '{dest}': {e}"));
            return out;
        }
        match extract(file, &idx, &plan, dest, progress) {
            Ok(()) => out.made.push(made(dest, false)),
            Err(e) => {
                let _ = std::fs::remove_dir_all(paths::to_native(dest));
                fail(&mut out, e);
            }
        }
        return out;
    }
    let stage = format!(
        "{}\\.filezy-extract-{}-{}",
        dest.trim_end_matches('\\'),
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    );
    let native_stage = paths::to_native(&stage);
    if let Err(e) = std::fs::create_dir(&native_stage) {
        out.error = Some(format!("Couldn't write to '{dest}': {e}"));
        return out;
    }
    hide(&native_stage);
    match extract(file, &idx, &plan, &stage, progress) {
        Ok(()) => {
            let srcs: Vec<String> =
                plan.top.iter().map(|t| format!("{stage}\\{t}")).filter(|p| paths::attributes(p).is_ok()).collect();
            let r = crate::ops::run(crate::ops::Op::Place { srcs, dest: dest.to_string() }, owner);
            for it in &r.items {
                match it.state {
                    crate::ops::State::Done => out.made.push(made(&it.dst, it.replaced)),
                    crate::ops::State::Skipped => out.kept += 1,
                    _ => {}
                }
            }
            out.cancelled |= r.cancelled;
            out.error = r.error.or_else(|| r.items.iter().find_map(|i| i.error.clone()));
        }
        Err(e) => fail(&mut out, e),
    }
    let _ = std::fs::remove_dir_all(&native_stage);
    out
}

/// Whether opening this archive should show what's inside, as Explorer does
/// when Windows' own zip folder is its default program. With another
/// archiver chosen, it opens in that program instead.
pub fn browses_inside(path: &str) -> bool {
    use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_NOTRUNCATE, ASSOCSTR_PROGID};
    if kind_of(path).is_none() {
        return false;
    }
    let ext = match path.rfind('.') {
        Some(i) => &path[i..],
        None => return false,
    };
    let w = wide(ext);
    let mut buf = [0u16; 260];
    let mut len = buf.len() as u32;
    let hr = unsafe {
        AssocQueryStringW(ASSOCF_NOTRUNCATE, ASSOCSTR_PROGID, PCWSTR(w.as_ptr()), PCWSTR::null(), Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len)
    };
    if hr.is_err() {
        // Nothing opens it: show it as a folder.
        return true;
    }
    let progid = String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)]);
    progid.is_empty() || progid.to_lowercase().contains("compressedfolder")
}

#[derive(serde::Serialize, Default)]
pub struct Compressed {
    pub made: Option<Made>,
    pub cancelled: bool,
    pub error: Option<String>,
}

/// Compress `items` into a new ZIP beside them, named after `name_from` (the
/// item the command was given on) without its extension, as Explorer names it.
pub fn compress_to(items: &[String], name_from: &str, progress: Progress) -> Compressed {
    let mut out = Compressed::default();
    let Some(dir) = items.first().and_then(|p| paths::parent(p)) else {
        out.error = Some("There is nothing to compress.".into());
        return out;
    };
    let is_dir = paths::attributes(&format!("{}\\{name_from}", dir.trim_end_matches('\\'))).is_ok_and(|a| a & ATTR_DIRECTORY != 0);
    let stem = match name_from.rfind('.') {
        Some(i) if i > 0 && !is_dir => &name_from[..i],
        _ => name_from,
    };
    let zip = unique(&dir, stem, ".zip");
    match compress(items, &zip, progress) {
        Ok(()) => out.made = Some(made(&zip, false)),
        Err(e) if e == CANCELLED => out.cancelled = true,
        Err(e) => out.error = Some(e),
    }
    out
}

// ---- one file opened from an archive ----

/// Where opened files are copied: one folder per archive and version, under
/// %TEMP%\Filezy\Open. The copies are read-only, so an edited one has to be
/// saved somewhere else; they are deleted when Filezy next starts.
fn open_dir(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    file.to_lowercase().hash(&mut h);
    crate::ops::stamp(file).hash(&mut h);
    std::env::temp_dir().join("Filezy").join("Open").join(format!("{:016x}", h.finish()))
}

/// Copy one file out of the archive for opening; its path.
pub fn stage(file: &str, inner: &str, progress: Progress) -> Result<String, String> {
    let idx = index(file)?;
    let i = idx.find(inner).ok_or_else(|| format!("'{inner}' isn't in '{}' anymore.", paths::file_name(file)))?;
    let it = &idx.items[i];
    if it.is_dir {
        return Err(format!("'{}' is a folder.", it.name()));
    }
    let dir = open_dir(file);
    let name = sanitize(it.name());
    let target = dir.join(&name);
    // Opened before and unchanged: open the same copy again.
    if std::fs::metadata(&target).is_ok_and(|m| m.len() == it.size) {
        return Ok(target.display().to_string());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if target.exists() {
        // Read-only from last time; it is about to be written again.
        let _ = std::fs::metadata(&target).map(|m| {
            let mut p = m.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            p.set_readonly(false);
            std::fs::set_permissions(&target, p)
        });
    }
    let plan = Plan { files: vec![(i, name.clone())], dirs: vec![], skipped: vec![], renamed: 0, bytes: it.size, top: vec![name] };
    if let Some(why) = it.problem {
        return Err(format!("'{}' can't be opened: {}.", it.name(), why.says()));
    }
    if it.encrypted {
        return Err(format!("'{}' is protected with a password, and Filezy can't open protected entries yet.", it.name()));
    }
    extract(file, &idx, &plan, &dir.display().to_string(), progress)?;
    if let Ok(m) = std::fs::metadata(&target) {
        let mut p = m.permissions();
        p.set_readonly(true);
        let _ = std::fs::set_permissions(&target, p);
    }
    Ok(target.display().to_string())
}

/// Delete copies of opened files made over a day ago. Newer ones may still be
/// on screen in another program (Notepad doesn't keep its file open), even
/// one started by another Filezy window; a file in use stays until next time.
pub fn clean_staged() {
    let root = std::env::temp_dir().join("Filezy").join("Open");
    let Ok(dirs) = std::fs::read_dir(&root) else { return };
    let day = std::time::Duration::from_secs(24 * 60 * 60);
    for d in dirs.flatten() {
        let old = d.metadata().and_then(|m| m.created()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > day);
        if !old {
            continue;
        }
        if let Ok(files) = std::fs::read_dir(d.path()) {
            for f in files.flatten() {
                if let Ok(m) = f.metadata() {
                    let mut p = m.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    p.set_readonly(false);
                    let _ = std::fs::set_permissions(f.path(), p);
                }
                let _ = std::fs::remove_file(f.path());
            }
        }
        let _ = std::fs::remove_dir(d.path());
    }
}

// ---- making a ZIP ----

/// Add files and folders (display paths) to a new ZIP at `out`. Names inside
/// are relative to each item's own folder. Links aren't followed.
pub fn compress(items: &[String], out: &str, progress: Progress) -> Result<(), String> {
    use zip::write::SimpleFileOptions;
    // Everything to add: (path on disk, name in the zip, is a folder, size).
    let mut todo: Vec<(PathBuf, String, bool, u64)> = Vec::new();
    for it in items {
        let disk = PathBuf::from(paths::to_native(it));
        let name = paths::file_name(it);
        walk(&disk, name, &mut todo).map_err(|e| format!("Couldn't read '{name}': {e}"))?;
    }
    let total: u64 = todo.iter().map(|t| t.3).sum();
    let tmp = format!("{out}.filezy-part");
    let native_tmp = paths::to_native(&tmp);
    let result = (|| -> Result<(), String> {
        let f = File::create(&native_tmp).map_err(|e| format!("Couldn't create '{}': {e}", paths::file_name(out)))?;
        hide(&native_tmp);
        let mut z = zip::ZipWriter::new(BufWriter::new(f));
        let mut done = 0u64;
        let mut buf = vec![0u8; 256 * 1024];
        for (disk, name, is_dir, size) in &todo {
            let modified = std::fs::metadata(disk).ok().and_then(|m| m.modified().ok());
            let mut o = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .large_file(*size >= u32::MAX as u64);
            if let Some(t) = modified.and_then(dos_time) {
                o = o.last_modified_time(t);
            }
            if *is_dir {
                z.add_directory(name.as_str(), o).map_err(|e| e.to_string())?;
                continue;
            }
            z.start_file(name.as_str(), o).map_err(|e| e.to_string())?;
            let mut r = File::open(disk).map_err(|e| format!("Couldn't read '{name}': {e}"))?;
            loop {
                let n = r.read(&mut buf).map_err(|e| format!("Couldn't read '{name}': {e}"))?;
                if n == 0 {
                    break;
                }
                z.write_all(&buf[..n]).map_err(|e| format!("Couldn't write the zip: {e}"))?;
                e2e_pace(n);
                done += n as u64;
                if !progress(done, total, name) {
                    return Err(CANCELLED.into());
                }
            }
        }
        z.finish().map_err(|e| format!("Couldn't write the zip: {e}"))?.flush().map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&native_tmp);
        return result;
    }
    unhide(&native_tmp);
    std::fs::rename(&native_tmp, paths::to_native(out)).map_err(|e| {
        let _ = std::fs::remove_file(&native_tmp);
        format!("Couldn't name the zip '{}': {e}", paths::file_name(out))
    })
}

/// Collect a file, or a folder and everything in it, for a ZIP. Reparse
/// points (links, junctions) are skipped rather than followed.
fn walk(disk: &Path, name: &str, out: &mut Vec<(PathBuf, String, bool, u64)>) -> std::io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    let m = std::fs::symlink_metadata(disk)?;
    if m.file_attributes() & 0x400 != 0 {
        return Ok(());
    }
    if m.is_dir() {
        out.push((disk.to_path_buf(), format!("{name}/"), true, 0));
        let mut kids: Vec<_> = std::fs::read_dir(disk)?.flatten().collect();
        kids.sort_by_key(|k| k.file_name());
        for k in kids {
            let child = format!("{name}/{}", k.file_name().to_string_lossy());
            walk(&k.path(), &child, out)?;
        }
    } else {
        out.push((disk.to_path_buf(), name.to_string(), false, m.len()));
    }
    Ok(())
}

/// A file time as the ZIP's DOS time, in local time as Windows writes it.
fn dos_time(t: std::time::SystemTime) -> Option<zip::DateTime> {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let ms = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
    let ft100 = (ms + 11_644_473_600_000) * 10_000;
    let ft = FILETIME { dwLowDateTime: ft100 as u32, dwHighDateTime: (ft100 >> 32) as u32 };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    unsafe {
        FileTimeToSystemTime(&ft, &mut utc).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).ok()?;
    }
    zip::DateTime::from_date_and_time(local.wYear, local.wMonth as u8, local.wDay as u8, local.wHour as u8, local.wMinute as u8, local.wSecond as u8).ok()
}

fn hide(native: &str) {
    use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};
    let w = wide(native);
    unsafe { let _ = SetFileAttributesW(PCWSTR(w.as_ptr()), FILE_ATTRIBUTE_HIDDEN); }
}

fn unhide(native: &str) {
    use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_NORMAL};
    let w = wide(native);
    unsafe { let _ = SetFileAttributesW(PCWSTR(w.as_ptr()), FILE_ATTRIBUTE_NORMAL); }
}

/// A name not yet used in `dir`: "name.ext", then "name (2).ext" and on.
pub fn unique(dir: &str, stem: &str, ext: &str) -> String {
    for n in 1.. {
        let name = if n == 1 { format!("{stem}{ext}") } else { format!("{stem} ({n}){ext}") };
        let p = format!("{}\\{name}", dir.trim_end_matches('\\'));
        if paths::attributes(&p).is_err() {
            return p;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_names_lose_what_points_elsewhere() {
        assert_eq!(normalize("../evil.txt"), ("evil.txt".into(), Some(Problem::Outside)));
        assert_eq!(normalize("dir/../../x.txt"), ("dir\\x.txt".into(), Some(Problem::Outside)));
        assert_eq!(normalize("C:/abs.txt"), ("abs.txt".into(), Some(Problem::Absolute)));
        assert_eq!(normalize("/rooted.txt"), ("rooted.txt".into(), Some(Problem::Absolute)));
        assert_eq!(normalize(r"\\server\share\unc.txt"), ("unc.txt".into(), Some(Problem::Absolute)));
        assert_eq!(normalize(r"dir\back.txt"), ("dir\\back.txt".into(), None));
        assert_eq!(normalize("./a//b/"), ("a\\b".into(), None));
    }

    #[test]
    fn names_become_valid_on_windows() {
        assert_eq!(sanitize("a:b.txt"), "a_b.txt");
        assert_eq!(sanitize("trail. "), "trail__");
        assert_eq!(sanitize("con.txt"), "_con.txt");
        assert_eq!(sanitize("COM1"), "_COM1");
        assert_eq!(sanitize("lpt².log"), "_lpt².log");
        assert_eq!(sanitize("console.txt"), "console.txt");
        assert_eq!(sanitize("what?.txt"), "what_.txt");
        assert_eq!(sanitize("école.txt"), "école.txt");
    }

    #[test]
    fn folders_only_implied_by_their_entries_appear() {
        let item = |p: &str, d: bool| Item { path: p.into(), is_dir: d, size: 1, modified: 0, encrypted: false, problem: None, index: Some(0) };
        let idx = Index::new(Kind::Zip, vec![item("a\\b\\c.txt", false), item("a", true), item("top.txt", false)]);
        let names = |dir: &str| idx.children(dir).map(|i| i.name().to_string()).collect::<Vec<_>>();
        assert_eq!(names(""), ["a", "top.txt"]);
        assert_eq!(names("A"), ["b"]);
        assert_eq!(names("a\\b"), ["c.txt"]);
        assert!(idx.is_dir("a\\B") && !idx.is_dir("top.txt"));
        // The real "a/" entry took over the implied folder's slot.
        assert_eq!(idx.get("a").unwrap().index, Some(0));
    }

    #[test]
    fn a_plan_skips_what_must_not_be_written() {
        let item = |p: &str, problem: Option<Problem>| Item { path: p.into(), is_dir: false, size: 5, modified: 0, encrypted: false, problem, index: Some(0) };
        let idx = Index::new(
            Kind::Zip,
            vec![item("docs\\a:b.txt", None), item("docs\\ok.txt", None), item("evil.txt", Some(Problem::Outside)), item("docs\\con.txt", None)],
        );
        let p = plan(&idx, "DOCS", &["docs\\a:b.txt".into(), "docs\\ok.txt".into(), "docs\\con.txt".into()]);
        assert_eq!(p.files.iter().map(|f| f.1.as_str()).collect::<Vec<_>>(), ["a_b.txt", "ok.txt", "_con.txt"]);
        assert_eq!(p.renamed, 2);
        assert_eq!(p.bytes, 15);
        let all = plan(&idx, "", &["".into()]);
        assert_eq!(all.skipped.len(), 1);
        assert_eq!(all.top, ["docs"]);
    }

    /// A fresh folder under %TEMP% for one test.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("filezy-archive-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn extracting_writes_only_inside_the_destination() {
        use zip::write::SimpleFileOptions;
        let d = scratch("extract");
        let zip_path = d.join("in.zip");
        let mut z = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        let o = SimpleFileOptions::default();
        for (name, data) in [("docs/école.txt", "e"), ("docs/sub/deep.txt", "deep"), ("top.txt", "top"), ("../evil.txt", "evil"), ("a:b.txt", "ads")] {
            z.start_file(name, o).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
        z.add_directory("empty/", o).unwrap();
        z.finish().unwrap();
        let file = zip_path.display().to_string();

        let names = |v: Vec<Entry>| { let mut n: Vec<String> = v.into_iter().map(|e| e.name).collect(); n.sort(); n };
        assert_eq!(names(list(&file, "").unwrap()), ["a:b.txt", "docs", "empty", "evil.txt", "top.txt"]);
        assert_eq!(names(list(&file, "DOCS").unwrap()), ["sub", "école.txt"]);
        assert!(list(&file, "top.txt").is_err());

        let out = d.join("out");
        std::fs::create_dir(&out).unwrap();
        let idx = index(&file).unwrap();
        let p = plan(&idx, "", &["".into()]);
        assert_eq!(p.skipped.len(), 1, "the ../ entry");
        extract(&file, &idx, &p, &out.display().to_string(), &mut |_, _, _| true).unwrap();
        let read = |rel: &str| std::fs::read_to_string(out.join(rel)).unwrap();
        assert_eq!(read("docs\\école.txt"), "e");
        assert_eq!(read("docs\\sub\\deep.txt"), "deep");
        assert_eq!(read("a_b.txt"), "ads");
        assert!(out.join("empty").is_dir());
        assert!(!d.join("evil.txt").exists() && !out.join("evil.txt").exists());

        // Just one folder's files, relative to that folder.
        let one = d.join("one");
        std::fs::create_dir(&one).unwrap();
        let p = plan(&idx, "docs", &["docs\\sub".into()]);
        extract(&file, &idx, &p, &one.display().to_string(), &mut |_, _, _| true).unwrap();
        assert_eq!(std::fs::read_to_string(one.join("sub\\deep.txt")).unwrap(), "deep");
        assert_eq!(p.top, ["sub"]);

        // Stopping midway leaves no partial file behind.
        let stop = d.join("stop");
        std::fs::create_dir(&stop).unwrap();
        let p = plan(&idx, "", &["top.txt".into()]);
        assert_eq!(extract(&file, &idx, &p, &stop.display().to_string(), &mut |_, _, _| false), Err(CANCELLED.to_string()));
        assert!(!stop.join("top.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_compressed_folder_reads_back() {
        let d = scratch("compress");
        let src = d.join("Report");
        std::fs::create_dir_all(src.join("empty")).unwrap();
        std::fs::create_dir_all(src.join("日本")).unwrap();
        std::fs::write(src.join("日本\\a.txt"), "a").unwrap();
        std::fs::write(d.join("loose.txt"), "loose").unwrap();
        let out = unique(&d.display().to_string(), "Report", ".zip");
        assert!(out.ends_with("Report.zip"));
        compress(&[src.display().to_string(), d.join("loose.txt").display().to_string()], &out, &mut |_, _, _| true).unwrap();
        let mut n: Vec<String> = index(&out).unwrap().items.iter().map(|i| format!("{}{}", i.path, if i.is_dir { "\\" } else { "" })).collect();
        n.sort();
        assert_eq!(n, ["Report\\", "Report\\empty\\", "Report\\日本\\", "Report\\日本\\a.txt", "loose.txt"]);
        assert!(unique(&d.display().to_string(), "Report", ".zip").ends_with("Report (2).zip"));
        // A stopped compression leaves nothing, not even the part file.
        let out2 = d.join("stopped.zip").display().to_string();
        assert!(compress(&[src.display().to_string()], &out2, &mut |_, _, _| false).is_err());
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 3, "Report, loose.txt and Report.zip only");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Archives other tools made (tests/archives): 7-Zip, PowerShell, and
    /// hand-built ones with hostile names, legacy encodings and lies.
    fn fixture(name: &str) -> String {
        format!(r"{}\tests\archives\{name}", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn archives_from_other_tools_list_alike() {
        let tree = ["emoji 😀.txt", "empty", "plain.txt", "sub dir", "école.txt", "日本語.txt"];
        for f in ["7zip.zip", "sample.7z", "sample.tar.gz"] {
            let mut n: Vec<String> = list(&fixture(f), "").unwrap().into_iter().map(|e| e.name).collect();
            n.sort();
            assert_eq!(n, tree, "{f}");
            let deep: Vec<String> = list(&fixture(f), r"sub dir\deeper").unwrap().into_iter().map(|e| e.name).collect();
            assert_eq!(deep, ["ünïcödé.md"], "{f}");
        }
        // PowerShell doesn't store empty folders.
        assert_eq!(list(&fixture("powershell.zip"), "").unwrap().len(), 5);
        assert!(list(&fixture("empty.zip"), "").unwrap().is_empty());
        // The Unicode Path extra field wins over the legacy name.
        assert_eq!(list(&fixture("unicode-extra.zip"), "").unwrap()[0].name, "naïve 日本.txt");
    }

    #[test]
    fn every_format_extracts_the_same_files() {
        for f in ["7zip.zip", "sample.7z", "sample.tar.gz"] {
            let d = scratch(&f.replace('.', "-"));
            let idx = index(&fixture(f)).unwrap();
            let p = plan(&idx, "", &["".into()]);
            extract(&fixture(f), &idx, &p, &d.display().to_string(), &mut |_, _, _| true).unwrap();
            assert_eq!(std::fs::read_to_string(d.join("sub dir\\deeper\\ünïcödé.md")).unwrap().trim(), "deep", "{f}");
            assert_eq!(std::fs::read_to_string(d.join("日本語.txt")).unwrap().trim_start_matches('\u{feff}').trim(), "japanese", "{f}");
            assert!(d.join("empty").is_dir(), "{f}");
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn hostile_damaged_and_protected_archives_are_reported() {
        let idx = index(&fixture("hostile.zip")).unwrap();
        let p = plan(&idx, "", &["".into()]);
        let mut skipped: Vec<&str> = p.skipped.iter().map(|s| s.0.as_str()).collect();
        skipped.sort();
        // ../evil.txt, dir/../../escape.txt, C:/abs.txt, /rooted.txt, \\server\share\unc.txt, and a link.
        assert_eq!(skipped, ["abs.txt", "dir\\escape.txt", "evil.txt", "link", "rooted.txt", "unc.txt"]);
        let mut written: Vec<&str> = p.files.iter().map(|f| f.1.as_str()).collect();
        written.sort();
        assert_eq!(written, ["_con.txt", "a_b.txt", "dir\\backslash.txt", "ok\\fine.txt", "trail__"]);

        // An entry that inflates past its declared size stops the extraction.
        let d = scratch("bomb");
        let idx = index(&fixture("bomb.zip")).unwrap();
        let p = plan(&idx, "", &["".into()]);
        assert!(extract(&fixture("bomb.zip"), &idx, &p, &d.display().to_string(), &mut |_, _, _| true).is_err());
        assert!(!d.join("big.txt").exists());
        let _ = std::fs::remove_dir_all(&d);

        assert!(index(&fixture("truncated.zip")).err().unwrap().starts_with("This archive is damaged"));
        for f in ["zipcrypto.zip", "aes.zip"] {
            let idx = index(&fixture(f)).unwrap();
            assert!(idx.items[0].encrypted, "{f}");
            let p = plan(&idx, "", &["".into()]);
            assert!(p.files.is_empty() && p.skipped.len() == 1, "{f}");
        }
    }

    #[test]
    fn archive_locations_split() {
        assert_eq!(split(r"C:\a.zip|docs\x"), Some((r"C:\a.zip", r"docs\x")));
        assert_eq!(split(r"C:\a.zip|"), Some((r"C:\a.zip", "")));
        assert_eq!(split(r"C:\a.zip"), None);
        assert_eq!(kind_of("x.TAR.GZ"), Some(Kind::TarGz));
        assert_eq!(kind_of("x.docx"), None);
    }
}
