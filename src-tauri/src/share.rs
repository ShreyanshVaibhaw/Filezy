//! Windows' Share sheet for files.
//!
//! Desktop apps reach it through DataTransferManager's window interop. The
//! shell's "Share" context-menu verb does nothing when another app hosts it, so
//! Explorer's own menu route isn't available to us.

use std::cell::RefCell;
use windows::core::{factory, AgileReference, Interface, HSTRING};
use windows::ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager};
use windows::Foundation::TypedEventHandler;
use windows::Storage::{IStorageItem, StorageFile};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::IDataTransferManagerInterop;
use windows_collections::IIterable;

/// Files ready to hand to the Share sheet, safe to move between threads.
pub struct Files {
    title: String,
    items: Vec<AgileReference<IStorageItem>>,
}

/// Resolve the paths to storage files. This blocks on the file system, so call
/// it off the UI thread.
pub fn files(paths: &[String]) -> Result<Files, String> {
    let items = paths
        .iter()
        .map(|p| {
            let item = (|| {
                let f = StorageFile::GetFileFromPathAsync(&HSTRING::from(p.as_str()))?.join()?;
                AgileReference::new(&f.cast::<IStorageItem>()?)
            })();
            item.map_err(|_| format!("'{}' can't be shared. Share works for files, not folders.", crate::paths::file_name(p)))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let title = match paths {
        [one] => crate::paths::file_name(one).to_string(),
        many => format!("{} files", many.len()),
    };
    Ok(Files { title, items })
}

thread_local! {
    /// The window's manager and the handler serving the last share. Lives on
    /// the UI thread, where every share runs.
    static CURRENT: RefCell<Option<(DataTransferManager, i64)>> = const { RefCell::new(None) };
}

/// Show the Share sheet over `hwnd`. Must run on the window's UI thread: the
/// sheet asks for the data there, through DataRequested.
pub fn show(hwnd: isize, files: Files) -> Result<(), String> {
    let fail = |e: windows::core::Error| format!("Windows couldn't open Share: {}", e.message());
    let hwnd = HWND(hwnd as *mut _);
    let interop = factory::<DataTransferManager, IDataTransferManagerInterop>().map_err(fail)?;
    let dtm: DataTransferManager = unsafe { interop.GetForWindow(hwnd) }.map_err(fail)?;
    let Files { title, items } = files;
    let handler = TypedEventHandler::<DataTransferManager, DataRequestedEventArgs>::new(move |_, args| {
        let Some(args) = args.as_ref() else { return Ok(()) };
        let data = args.Request()?.Data()?;
        data.Properties()?.SetTitle(&HSTRING::from(title.as_str()))?;
        let list = items.iter().map(|i| i.resolve().map(Some)).collect::<windows::core::Result<Vec<_>>>()?;
        data.SetStorageItems(&IIterable::<IStorageItem>::from(list), true)
    });
    let token = dtm.DataRequested(&handler).map_err(fail)?;
    CURRENT.with(|c| {
        if let Some((old, t)) = c.replace(Some((dtm, token))) {
            let _ = old.RemoveDataRequested(t);
        }
    });
    unsafe { interop.ShowShareUIForWindow(hwnd) }.map_err(fail)
}
