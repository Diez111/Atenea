//! Reference Atenea model plugin: counts stepped picoseconds.
//!
//! Exports the `vsm-host` C ABI (`atenea_model_*`). Built as `cdylib`
//! and loaded by `crates/vsm-host/examples/host_demo.rs`.

use std::ffi::c_void;

struct Counter {
    total_ps: u64,
}

/// ABI version implemented by this plugin.
#[unsafe(no_mangle)]
pub extern "C" fn atenea_model_api_version() -> u32 {
    1
}

/// Create one counter instance (caller owns it; see `atenea_model_destroy`).
#[unsafe(no_mangle)]
pub extern "C" fn atenea_model_create() -> *mut c_void {
    Box::into_raw(Box::new(Counter { total_ps: 0 })).cast::<c_void>()
}

/// Advance `dt_ps`; always succeeds for the null model.
#[unsafe(no_mangle)]
pub extern "C" fn atenea_model_step(inst: *mut c_void, dt_ps: u64) -> i32 {
    if inst.is_null() {
        return -1;
    }
    // SAFETY: `inst` comes from `atenea_model_create`.
    let counter = unsafe { &mut *inst.cast::<Counter>() };
    counter.total_ps = counter.total_ps.saturating_add(dt_ps);
    0
}

/// Destroy an instance created by [`atenea_model_create`].
#[unsafe(no_mangle)]
pub extern "C" fn atenea_model_destroy(inst: *mut c_void) {
    if !inst.is_null() {
        // SAFETY: `inst` comes from `atenea_model_create`, freed once.
        unsafe {
            drop(Box::from_raw(inst.cast::<Counter>()));
        }
    }
}
