//! The Windows Search index: whether it covers a folder, and indexed queries.
//!
//! Queries go through Windows Search's query helper, which turns the search
//! box's text into the index's SQL the way Explorer does (names, properties,
//! file contents, Advanced Query Syntax), and run on the index's OLE DB
//! provider. Results come from the index without opening files, so cloud-only
//! files stay in the cloud.
use crate::fs_list::{entry_from, wide};
use crate::listing::{self, Entry};
use crate::paths;
use crate::search::{max_results, Sink};
use windows::core::{w, Error, Interface, GUID, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Storage::FileSystem::{FindClose, FindFirstFileW, WIN32_FIND_DATAW};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Search::{
    CatalogStatus, CSearchManager, IAccessor, ICommandText, IDBCreateCommand, IDBCreateSession, IDBInitialize, IDataInitialize, IRowset,
    ISearchManager, CLUSION_REASON, DBACCESSOR_ROWDATA, DBBINDING, DBMEMOWNER_CLIENTOWNED, DBPARAMIO_NOTPARAM, DBPART_LENGTH,
    DBPART_STATUS, DBPART_VALUE, DBSTATUS_S_OK, DBTYPE_BYREF, DBTYPE_WSTR, DB_S_ENDOFROWSET, HACCESSOR, MSDAINITIALIZE,
};

thread_local! {
    static COM_INIT: () = unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); };
}

/// What the index says about a folder.
#[derive(Debug, Clone, PartialEq)]
pub enum Coverage {
    Indexed,
    /// Inside the index's scope but excluded (a rule or a user choice).
    Excluded,
    NotIndexed,
    /// The Windows Search service isn't there or isn't answering.
    Unavailable,
}

/// The catalog's "file:" URL for a folder, as the crawl scope stores them.
fn url_of(path: &str) -> String {
    format!("file:///{}", path.trim_end_matches('\\'))
}

pub fn coverage(path: &str) -> Coverage {
    COM_INIT.with(|_| {});
    unsafe {
        let Ok(mgr) = CoCreateInstance::<_, ISearchManager>(&CSearchManager, None, CLSCTX_LOCAL_SERVER) else {
            return Coverage::Unavailable;
        };
        let Ok(cat) = mgr.GetCatalog(windows::core::w!("SystemIndex")) else { return Coverage::Unavailable };
        let (mut status, mut why) = (CatalogStatus::default(), Default::default());
        // A paused or rebuilding index still answers, from what it has.
        if cat.GetCatalogStatus(&mut status, &mut why).is_err() {
            return Coverage::Unavailable;
        }
        let Ok(scope) = cat.GetCrawlScopeManager() else { return Coverage::Unavailable };
        let w = wide(&url_of(path));
        let (mut included, mut reason) = (windows::core::BOOL(0), CLUSION_REASON::default());
        if scope.IncludedInCrawlScopeEx(PCWSTR(w.as_ptr()), &mut included, &mut reason).is_err() {
            return Coverage::Unavailable;
        }
        // CLUSION_REASON_USER/GROUPPOLICY... with "not included" means a rule excluded it.
        match (included.as_bool(), reason.0) {
            (true, _) => Coverage::Indexed,
            (false, 0) => Coverage::NotIndexed, // CLUSION_REASON_UNKNOWNSCOPE: outside every rule
            (false, _) => Coverage::Excluded,
        }
    }
}

/// OLE DB's dialect for "the provider's own SQL" (DBGUID_DEFAULT in oledb.h).
const DBGUID_DEFAULT: GUID = GUID::from_u128(0xc8b521fb_5cf3_11ce_ade5_00aa0044773d);

/// The index's URL for a folder in its SQL ("file:C:/Users/me"), quotes doubled.
fn scope_url(root: &str) -> String {
    let p = root.trim_end_matches('\\').replace('\\', "/").replace('\'', "''");
    // A drive root keeps its slash: "file:C:/".
    if p.len() == 2 { format!("file:{p}/") } else { format!("file:{p}") }
}

/// A string the shell allocated, read and freed.
unsafe fn take(p: PWSTR) -> String {
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    s
}

/// The SQL for `query` under the roots, and the connection string to run it
/// with. Windows' own query helper writes it, so Advanced Query Syntax (words,
/// kind:, size:, dates, file contents) means exactly what it means in Explorer.
unsafe fn sql_for(roots: &[String], query: &str, folder_only: bool) -> windows::core::Result<(String, String)> {
    unsafe {
        let mgr: ISearchManager = CoCreateInstance(&CSearchManager, None, CLSCTX_LOCAL_SERVER)?;
        let helper = mgr.GetCatalog(w!("SystemIndex"))?.GetQueryHelper()?;
        helper.SetQuerySelectColumns(w!("System.ItemPathDisplay"))?;
        // DIRECTORY is the folder's own items; SCOPE takes in every subfolder.
        let pred = if folder_only { "DIRECTORY" } else { "SCOPE" };
        let scopes: Vec<String> = roots.iter().map(|r| format!("{pred}='{}'", scope_url(r))).collect();
        helper.SetQueryWhereRestrictions(&HSTRING::from(format!("AND ({})", scopes.join(" OR "))))?;
        helper.SetQueryMaxResults(i32::try_from(max_results() + 1).unwrap_or(i32::MAX))?;
        let sql = take(helper.GenerateSQLFromUserQuery(&HSTRING::from(query))?);
        Ok((sql, take(helper.ConnectionString()?)))
    }
}

/// Run `sql` through the index's OLE DB provider.
unsafe fn execute(conn: &str, sql: &str) -> windows::core::Result<IRowset> {
    unsafe {
        let init: IDataInitialize = CoCreateInstance(&MSDAINITIALIZE, None, CLSCTX_INPROC_SERVER)?;
        let mut source = None;
        init.GetDataSource(None, CLSCTX_INPROC_SERVER.0, &HSTRING::from(conn), &IDBInitialize::IID, &mut source)?;
        let source: IDBInitialize = source.ok_or_else(|| Error::from(E_FAIL))?.cast()?;
        source.Initialize()?;
        let session: IDBCreateCommand = source.cast::<IDBCreateSession>()?.CreateSession(None, &IDBCreateCommand::IID)?.cast()?;
        let cmd: ICommandText = session.CreateCommand(None, &ICommandText::IID)?.cast()?;
        cmd.SetCommandText(&DBGUID_DEFAULT, &HSTRING::from(sql))?;
        let mut rows = None;
        cmd.Execute(None, &IRowset::IID, None, None, Some(&mut rows))?;
        rows.ok_or_else(|| Error::from(E_FAIL))?.cast()
    }
}

/// Where GetData puts one row's path: a provider-allocated string (BYREF,
/// client-owned, so any length fits) and its status.
#[repr(C)]
struct Cell {
    value: *mut u16,
    /// In bytes, without the terminator.
    length: usize,
    status: u32,
}

/// Read every path from the rowset, `f` taking each until it returns false.
unsafe fn each_path(rows: &IRowset, mut f: impl FnMut(String) -> bool) -> windows::core::Result<()> {
    unsafe {
        let acc: IAccessor = rows.cast()?;
        let binding = DBBINDING {
            iOrdinal: 1,
            obValue: std::mem::offset_of!(Cell, value),
            obLength: std::mem::offset_of!(Cell, length),
            obStatus: std::mem::offset_of!(Cell, status),
            pTypeInfo: std::mem::ManuallyDrop::new(None),
            pObject: std::ptr::null_mut(),
            pBindExt: std::ptr::null_mut(),
            dwPart: (DBPART_VALUE.0 | DBPART_LENGTH.0 | DBPART_STATUS.0) as u32,
            dwMemOwner: DBMEMOWNER_CLIENTOWNED.0 as u32,
            eParamIO: DBPARAMIO_NOTPARAM.0 as u32,
            cbMaxLen: 0,
            dwFlags: 0,
            wType: (DBTYPE_WSTR.0 | DBTYPE_BYREF.0) as u16,
            bPrecision: 0,
            bScale: 0,
        };
        let mut h = HACCESSOR::default();
        acc.CreateAccessor(DBACCESSOR_ROWDATA.0 as u32, 1, &binding, size_of::<Cell>(), &mut h, None)?;
        let mut more = true;
        while more {
            // The binding's GetNextRows takes the row count from a slice's length,
            // which doesn't fit OLE DB's HROW** argument; call the table directly.
            let mut batch = [0usize; 64];
            let (mut got, mut p) = (0usize, batch.as_mut_ptr());
            let hr = (Interface::vtable(rows).GetNextRows)(Interface::as_raw(rows), 0, 0, batch.len() as isize, &mut got, &mut p);
            if hr.is_err() || got == 0 {
                break;
            }
            for &row in &batch[..got] {
                let mut cell = Cell { value: std::ptr::null_mut(), length: 0, status: 0 };
                let read = rows.GetData(row, h, &mut cell as *mut Cell as *mut _).is_ok();
                if !cell.value.is_null() {
                    let path = String::from_utf16_lossy(std::slice::from_raw_parts(cell.value, cell.length / 2));
                    CoTaskMemFree(Some(cell.value as *const _));
                    if more && read && cell.status == DBSTATUS_S_OK.0 as u32 {
                        more = f(path);
                    }
                }
            }
            let _ = rows.ReleaseRows(got, batch.as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut());
            more &= hr != DB_S_ENDOFROWSET;
        }
        let _ = acc.ReleaseAccessor(h, None);
        Ok(())
    }
}

/// One result's listing entry, from the file system (sizes and dates come
/// from the directory entry, as for every other row).
fn entry_at(path: &str) -> Option<Entry> {
    let w = wide(&paths::to_native(path));
    let mut data = WIN32_FIND_DATAW::default();
    let h = unsafe { FindFirstFileW(PCWSTR(w.as_ptr()), &mut data) }.ok()?;
    unsafe { let _ = FindClose(h); }
    let mut e = entry_from(&data)?;
    e.parent = paths::parent(path);
    Some(e)
}

/// Run `query` against the index under each root, pushing results into the
/// sink. `folder_only` keeps to the roots' own items. False when the index
/// couldn't be asked at all.
pub fn search(roots: &[String], query: &str, folder_only: bool, show_hidden: bool, show_protected: bool, sink: &mut Sink) -> bool {
    COM_INIT.with(|_| {});
    let rows = unsafe { sql_for(roots, query, folder_only).and_then(|(sql, conn)| execute(&conn, &sql)) };
    let Ok(rows) = rows else { return false };
    let roots: Vec<String> = roots.iter().map(|r| r.trim_end_matches('\\').to_lowercase()).collect();
    let read = unsafe {
        each_path(&rows, |path| {
            if sink.cancelled() {
                return false;
            }
            // The scope's own folder isn't one of its results.
            if roots.contains(&path.trim_end_matches('\\').to_lowercase()) {
                return true;
            }
            // Gone since it was indexed, or not a file system item.
            let Some(e) = entry_at(&path) else { return true };
            !listing::visible(e.attrs, show_hidden, show_protected) || sink.push(e)
        })
    };
    read.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_read_as_the_index_writes_them() {
        assert_eq!(scope_url(r"C:\Users\me\"), "file:C:/Users/me");
        assert_eq!(scope_url(r"C:\"), "file:C:/");
        assert_eq!(scope_url(r"D:\Bob's files"), "file:D:/Bob''s files");
    }
}

