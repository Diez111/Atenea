//! Native host for Atenea simulation-model plugins.
//!
//! A model plugin is a `cdylib` exporting, with C ABI:
//!
//! - `atenea_model_api_version() -> u32` (must return [`API_VERSION`]),
//! - `atenea_model_create() -> *mut c_void` (opaque instance),
//! - `atenea_model_step(*mut c_void, dt_ps: u64) -> i32`
//!   (advance one step; returns nonzero on error),
//! - `atenea_model_destroy(*mut c_void)`.
//!
//! This is a clean-room native interface for Atenea models. It mirrors
//! the *roles* of the documented VSM entry points (`createactivemodel`,
//! `deleteactivemodel`, `createdevice`, `deletedevice`) without copying
//! any vendor ABI, header, or binary.
//!
//! # Errors
//!
//! [`LoadError`] covers missing files, missing symbols, version mismatch,
//! and null instance handles.

use std::ffi::c_void;
use std::fmt;

/// Current plugin ABI version.
pub const API_VERSION: u32 = 1;

/// Plugin load failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// `dlopen` failed.
    Open(String),
    /// A required symbol is missing.
    Symbol(String),
    /// `atenea_model_api_version` returned something else.
    Version(u32),
    /// Factory returned null.
    NullInstance,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(e) => write!(f, "open: {e}"),
            Self::Symbol(s) => write!(f, "missing symbol: {s}"),
            Self::Version(v) => write!(f, "unsupported api version: {v}"),
            Self::NullInstance => write!(f, "factory returned null"),
        }
    }
}

impl std::error::Error for LoadError {}

type VersionFn = unsafe extern "C" fn() -> u32;
type CreateFn = unsafe extern "C" fn() -> *mut c_void;
type StepFn = unsafe extern "C" fn(*mut c_void, u64) -> i32;
type DestroyFn = unsafe extern "C" fn(*mut c_void);

/// A loaded model plugin. Destroyed (instance dropped) on [`Drop`].
pub struct Plugin {
    _lib: libloading::Library,
    instance: *mut c_void,
    step: StepFn,
    destroy: DestroyFn,
}

// SAFETY: the instance is an opaque pointer owned by this struct;
// `step` is only called via `&mut self`.
unsafe impl Send for Plugin {}

impl Plugin {
    /// Load `path`, check the ABI version, create one instance.
    ///
    /// # Errors
    ///
    /// [`LoadError`] on any failure; nothing is leaked on error paths
    /// (a created instance is destroyed before returning).
    ///
    /// # Safety
    ///
    /// The file must be a well-formed Atenea plugin: calling its
    /// exports must be sound. Never load untrusted `.so` files.
    pub unsafe fn load(path: &std::path::Path) -> Result<Self, LoadError> {
        // SAFETY: caller guarantees the file is a well-formed plugin.
        let lib = unsafe { libloading::Library::new(path) }
            .map_err(|e| LoadError::Open(e.to_string()))?;
        let version: libloading::Symbol<VersionFn> = unsafe {
            lib.get(b"atenea_model_api_version")
                .map_err(|_| LoadError::Symbol("atenea_model_api_version".into()))?
        };
        let v = unsafe { version() };
        if v != API_VERSION {
            return Err(LoadError::Version(v));
        }
        let create: libloading::Symbol<CreateFn> = unsafe {
            lib.get(b"atenea_model_create")
                .map_err(|_| LoadError::Symbol("atenea_model_create".into()))?
        };
        let instance = unsafe { create() };
        if instance.is_null() {
            return Err(LoadError::NullInstance);
        }
        let step: StepFn = unsafe {
            *lib.get(b"atenea_model_step")
                .map_err(|_| LoadError::Symbol("atenea_model_step".into()))?
        };
        let destroy: DestroyFn = unsafe {
            *lib.get(b"atenea_model_destroy")
                .map_err(|_| LoadError::Symbol("atenea_model_destroy".into()))?
        };
        Ok(Self {
            _lib: lib,
            instance,
            step,
            destroy,
        })
    }

    /// Advance one timestep of `dt_ps` picoseconds.
    ///
    /// # Errors
    ///
    /// Returns the plugin's nonzero status code on model error.
    pub fn step(&mut self, dt_ps: u64) -> Result<(), i32> {
        // SAFETY: instance came from this plugin's own factory.
        let rc = unsafe { (self.step)(self.instance, dt_ps) };
        if rc == 0 { Ok(()) } else { Err(rc) }
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        if !self.instance.is_null() {
            // SAFETY: instance came from this plugin's own factory.
            unsafe { (self.destroy)(self.instance) };
            self.instance = std::ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_error() {
        // SAFETY: no plugin code runs; open fails first.
        let res = unsafe { Plugin::load(std::path::Path::new("/nonexistent.so")) };
        let Err(err) = res else {
            panic!("expected Open error");
        };
        assert!(matches!(err, LoadError::Open(_)));
    }

    #[test]
    fn display_roundtrips() {
        assert_eq!(
            LoadError::Version(9).to_string(),
            "unsupported api version: 9"
        );
        assert_eq!(
            LoadError::Symbol("x".into()).to_string(),
            "missing symbol: x"
        );
    }
}
