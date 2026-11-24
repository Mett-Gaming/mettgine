//! DLLs der Hersteller zur Laufzeit laden. Gesucht wird nur neben der exe und, zum Entwickeln, in den
//! Ordnern aus `METTGINE_VENDOR` (mit `;` getrennt).

use std::ffi::{c_void, CStr};
use std::path::PathBuf;

/// Umgebungsvariable mit weiteren Ordnern
const PATH_ENV: &str = "METTGINE_VENDOR";

/// Geladene Bibliothek. Sie bleibt bis zum Ende des Prozesses geladen: Treiber und Bibliothek
/// koennen noch eigene Faeden haben.
#[derive(Clone, Copy)]
pub struct Library(*mut c_void);

// SAFETY: Das Handle ist nur eine Nummer, Funktionen darf jeder Faden nachschlagen.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}

/// Wo `name` liegt.
pub fn find(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let own = exe.parent().map(PathBuf::from);
    let more = std::env::var(PATH_ENV).unwrap_or_default();
    let more = more.split(';').filter(|s| !s.is_empty()).map(PathBuf::from);
    own.into_iter()
        .chain(more)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

impl Library {
    /// `None` = nicht da oder nicht ladbar.
    pub fn load(name: &str) -> Option<Self> {
        let path = find(name)?;
        let lib = platform::load(&path);
        if lib.is_none() {
            crate::crash::log(&format!("{name} laesst sich nicht laden"));
        }
        lib.map(Self)
    }

    /// Funktion nachschlagen.
    ///
    /// # Safety
    /// `F` muss der Funktionstyp von `name` sein.
    pub unsafe fn get<F: Copy>(&self, name: &CStr) -> Option<F> {
        const { assert!(size_of::<F>() == size_of::<*mut c_void>()) };
        let p = platform::symbol(self.0, name)?;
        Some(std::mem::transmute_copy(&p))
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::{c_char, c_void, CStr};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    /// Abhaengigkeiten zuerst im Ordner der DLL suchen
    const ALTERED_SEARCH_PATH: u32 = 8;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(lib: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    pub fn load(path: &Path) -> Option<*mut c_void> {
        let path = std::path::absolute(path).ok()?;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: nullterminierter Pfad.
        let p = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), ALTERED_SEARCH_PATH) };
        (!p.is_null()).then_some(p)
    }

    pub fn symbol(lib: *mut c_void, name: &CStr) -> Option<*mut c_void> {
        // SAFETY: gueltiges Handle, nullterminierter Name.
        let p = unsafe { GetProcAddress(lib, name.as_ptr()) };
        (!p.is_null()).then_some(p)
    }
}

#[cfg(not(windows))]
mod platform {
    use std::ffi::{c_void, CStr};
    use std::path::Path;

    pub fn load(_path: &Path) -> Option<*mut c_void> {
        None
    }

    pub fn symbol(_lib: *mut c_void, _name: &CStr) -> Option<*mut c_void> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_libraries_are_no_error() {
        assert!(Library::load("gibt-es-nicht-4711.dll").is_none());
        assert!(find("gibt-es-nicht-4711.dll").is_none());
    }
}
