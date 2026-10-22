//! Native Rust model of the Proteus VSM simulation-model interface.
//!
//! Facts (evidence, not headers — the VSM SDK ships only under NDA):
//! - Community documention (ee-diary, TEP) + `strings` on shipped binaries:
//!   `ISIS.DLL` imports the exact export names `createactivemodel` and
//!   `deleteactivemodel`; batch entry points `createdevice`/`deletedevice`
//!   are documented for batch mode. `VSMDEBUG.dll` performs the
//!   `LoadLibraryA`/`GetProcAddress` loading; `DSIM.DLL` exports
//!   `dsim_create` + `IIDSIMMODEL`.
//! - Class surface (documented): `IACTIVEMODEL::{initialize, getspicemodel,
//!   getdsimmodel, plot, animate, actuate}`; specializations
//!   `ISPICEMODEL` (analog), `IDSIMMODEL` (digital), `IMIXEDMODEL` (both).
//! - Binding in designs: `MODDLL=<file>` + `PRIMITIVE=ACTIVE` in `*PARTLIST`.
//! - Proteus 8 hosts 32-bit x86 model DLLs; Proteus 9 moved to 64-bit.
//!
//! v0 scope: export-name constants + safe-Rust traits mirroring the
//! documented methods. A loadable `cdylib` with MSVC-compatible vtables
//! needs the original `vsm.hpp` (not available) and stays out of scope.

/// Export looked up by the host for interactive simulation models.
pub const EXPORT_CREATE_ACTIVE_MODEL: &str = "createactivemodel";
/// Export looked up by the host to destroy interactive models.
pub const EXPORT_DELETE_ACTIVE_MODEL: &str = "deleteactivemodel";
/// Batch-mode creation entry point.
pub const EXPORT_CREATE_DEVICE: &str = "createdevice";
/// Batch-mode destruction entry point.
pub const EXPORT_DELETE_DEVICE: &str = "deletedevice";
/// Digital-simulation factory exported by `DSIM.DLL`.
pub const EXPORT_DSIM_CREATE: &str = "dsim_create";

/// Pin logic state passed to digital models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicState {
    /// Low / false.
    Low,
    /// High / true.
    High,
    /// Floating / undefined.
    Floating,
}

/// Analog behavior of a model (`ISPICEMODEL` role).
pub trait SpiceModel {
    /// Primitive name this model implements (e.g. `"ADC"`, `"DAC"`).
    fn primitive(&self) -> &str;
}

/// Digital behavior of a model (`IDSIMMODEL` role).
pub trait DsimModel {
    /// Evaluate one timestep; returns `true` when outputs changed.
    fn evaluate(&mut self, time_ps: u64) -> bool;
    /// Read back an output pin state.
    fn pin_state(&self, pin: &str) -> LogicState;
}

/// Full interactive model (`IACTIVEMODEL` role).
pub trait ActiveModel: SpiceModel {
    /// Called once with the component instance properties.
    fn initialize(&mut self, props: &std::collections::HashMap<String, String>);
    /// Digital facet, when the model drives digital pins.
    fn dsim(&mut self) -> Option<&mut dyn DsimModel> {
        None
    }
    /// Frame render hook (`plot`).
    fn plot(&mut self) {}
    /// User-interaction hook (`actuate`); `true` consumes the event.
    fn actuate(&mut self, _key: u16, _x: i32, _y: i32, _flags: u32) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Dummy {
        props: HashMap<String, String>,
        toggled: bool,
    }

    impl SpiceModel for Dummy {
        fn primitive(&self) -> &'static str {
            "DUMMY"
        }
    }

    impl DsimModel for Dummy {
        fn evaluate(&mut self, _time_ps: u64) -> bool {
            self.toggled = !self.toggled;
            true
        }

        fn pin_state(&self, _pin: &str) -> LogicState {
            if self.toggled {
                LogicState::High
            } else {
                LogicState::Low
            }
        }
    }

    impl ActiveModel for Dummy {
        fn initialize(&mut self, props: &HashMap<String, String>) {
            self.props = props.clone();
        }

        fn dsim(&mut self) -> Option<&mut dyn DsimModel> {
            Some(self)
        }
    }

    #[test]
    fn export_names_match_shipped_binaries() {
        assert_eq!(EXPORT_CREATE_ACTIVE_MODEL, "createactivemodel");
        assert_eq!(EXPORT_DELETE_ACTIVE_MODEL, "deleteactivemodel");
        assert_eq!(EXPORT_CREATE_DEVICE, "createdevice");
        assert_eq!(EXPORT_DELETE_DEVICE, "deletedevice");
        assert_eq!(EXPORT_DSIM_CREATE, "dsim_create");
    }

    #[test]
    fn dummy_model_roundtrip() {
        let mut m = Dummy {
            props: HashMap::new(),
            toggled: false,
        };
        m.initialize(&HashMap::from([("VALUE".to_owned(), "1".to_owned())]));
        assert_eq!(m.props["VALUE"], "1");
        assert!(m.dsim().unwrap().evaluate(0));
        assert_eq!(m.pin_state("Q"), LogicState::High);
    }
}
