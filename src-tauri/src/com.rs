//! COM apartments. A thread stays in the apartment it joins first, and Tauri's
//! blocking pool and async workers run every command, so a module that joined
//! one kind on a shared thread would leave the next task that wants the other
//! kind running in the wrong one.
//!
//! So the two kinds never share a thread. Shell work that loads extensions
//! (icons, thumbnails, properties, shortcuts, the Recycle Bin) joins a
//! single-threaded apartment wherever it runs (`sta`). WinRT and the search
//! index, library and device code run in the multithreaded apartment, always
//! on a thread of their own (`mta`).
use std::sync::Once;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{
    CoIncrementMTAUsage, CoInitializeEx, CoUninitialize, COINIT, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED,
};

/// Join a single-threaded apartment on this thread, once.
pub fn sta() {
    thread_local!(static JOINED: bool = join(COINIT_APARTMENTTHREADED, "single-threaded"));
    JOINED.with(|_| {});
}

/// Run `f` in the multithreaded apartment on a thread of its own, and wait for it.
pub fn mta<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    // Keeps the apartment alive between calls, so each call's thread joins it
    // rather than building it, and loading WinRT, again.
    static KEEP: Once = Once::new();
    KEEP.call_once(|| {
        if let Err(e) = unsafe { CoIncrementMTAUsage() } {
            eprintln!("COM: couldn't keep the multithreaded apartment alive: {e}");
        }
    });
    std::thread::scope(|s| {
        s.spawn(|| {
            let joined = join(COINIT_MULTITHREADED, "multithreaded");
            let out = f();
            if joined {
                unsafe { CoUninitialize() };
            }
            out
        })
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// CoInitializeEx, saying so when the thread was already in the other kind of
/// apartment. True when the call must be paired with CoUninitialize.
fn join(model: COINIT, name: &str) -> bool {
    let hr = unsafe { CoInitializeEx(None, model) };
    if hr == RPC_E_CHANGED_MODE {
        let thread = std::thread::current();
        eprintln!("COM: thread {:?} ({}) is in another apartment; {name} work runs there anyway", thread.id(), thread.name().unwrap_or("unnamed"));
    } else if hr.is_err() {
        eprintln!("COM: couldn't join a {name} apartment: {}", windows::core::Error::from(hr).message());
    }
    hr.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{CoGetApartmentType, APTTYPE, APTTYPEQUALIFIER, APTTYPE_MTA, APTTYPE_STA};

    fn apartment() -> APTTYPE {
        let (mut kind, mut qualifier) = (APTTYPE::default(), APTTYPEQUALIFIER::default());
        unsafe { CoGetApartmentType(&mut kind, &mut qualifier) }.unwrap();
        kind
    }

    #[test]
    fn mta_work_never_lands_in_a_thread_that_joined_an_sta() {
        sta();
        assert_eq!(apartment(), APTTYPE_STA);
        assert_eq!(mta(apartment), APTTYPE_MTA);
        // The calling thread is unchanged, and a second call works the same.
        assert_eq!(apartment(), APTTYPE_STA);
        let local = 7;
        assert_eq!(mta(|| (apartment(), local)), (APTTYPE_MTA, 7));
    }
}
