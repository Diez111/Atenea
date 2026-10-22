//! Native behavioral ADC0808 (8-bit successive-approximation ADC).
//!
//! Simplified state machine: on a `START` rising edge with `ALE` the
//! channel (`A`,`B`,`C`) is latched, `EOC` goes low for the conversion
//! time (`~72` clock periods), then `EOC` goes high and `D0..D7` hold
//! `round(VIN/VREF·255)` while `OE` is high (`Floating`/high-Z when low).

use std::collections::HashMap;
use vsm::{ActiveModel, DsimModel, LogicState, SpiceModel};

/// Conversion time in clock periods (datasheet: 64–73).
pub const CONV_CLOCKS: u64 = 72;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    Converting {
        /// Remaining conversion time in ps.
        left_ps: u64,
    },
    Done,
}

/// Behavioral ADC0808.
#[derive(Debug, Clone, Default)]
pub struct Adc0808 {
    /// Reference voltage (V).
    pub vref: f64,
    /// Selected channel 0..7.
    pub channel: u8,
    /// Input voltage being converted (V).
    pub vin: f64,
    /// Clock period in ps.
    pub clk_ps: u64,
    /// Output enable.
    pub oe: bool,
    phase: Phase,
    /// Last conversion result.
    pub code: u8,
    /// EOC pin level.
    pub eoc: bool,
}

/// f64 seconds → ps, saturating. Inputs here are positive sub-hour
/// intervals, far inside `u64`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn s_to_ps(s: f64) -> u64 {
    (s * 1e12).max(1.0) as u64
}

impl Adc0808 {
    fn convert_time_ps(&self) -> u64 {
        self.clk_ps.saturating_mul(CONV_CLOCKS)
    }
}

impl SpiceModel for Adc0808 {
    fn primitive(&self) -> &'static str {
        "ADC0808"
    }
}

impl ActiveModel for Adc0808 {
    fn initialize(&mut self, props: &HashMap<String, String>) {
        let num = |k: &str, d: f64| {
            props
                .get(k)
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(d)
        };
        self.vref = num("VREF", 5.0);
        self.vin = num("VIN", 0.0);
        // `FCLK` in Hz when given (`640k` style handled as plain Hz).
        let fclk = props
            .get("FCLK")
            .and_then(|v| v.trim_end_matches("kHz").parse::<f64>().ok())
            .map(|f| f * 1000.0)
            .or_else(|| props.get("FCLK").and_then(|v| v.parse::<f64>().ok()))
            .unwrap_or(640_000.0);
        self.clk_ps = s_to_ps(1.0 / f64::max(fclk, 1.0));
        self.oe = props.get("OE").is_none_or(|v| v != "0");
        let ch = ["A", "B", "C"].iter().enumerate().fold(0u8, |acc, (i, k)| {
            if props.get(*k).is_some_and(|v| v == "1") {
                acc | (1 << i)
            } else {
                acc
            }
        });
        self.channel = ch;
        self.phase = Phase::Idle;
        self.eoc = true;
    }

    fn dsim(&mut self) -> Option<&mut dyn DsimModel> {
        Some(self)
    }
}

impl DsimModel for Adc0808 {
    /// Advance `time_ps`; returns `true` on EOC edges.
    fn evaluate(&mut self, time_ps: u64) -> bool {
        match self.phase {
            Phase::Converting { left_ps } => {
                if time_ps >= left_ps {
                    self.phase = Phase::Done;
                    let frac = (self.vin / self.vref).clamp(0.0, 1.0);
                    // frac in [0,1] so this is 0..=255; the range is
                    // enforced by construction, not by the type system.
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let code = (frac * 255.0).round() as u8;
                    self.code = code;
                    self.eoc = true;
                    true
                } else {
                    self.phase = Phase::Converting {
                        left_ps: left_ps - time_ps,
                    };
                    false
                }
            }
            _ => false,
        }
    }

    fn pin_state(&self, pin: &str) -> LogicState {
        if pin.eq_ignore_ascii_case("EOC") {
            return if self.eoc {
                LogicState::High
            } else {
                LogicState::Low
            };
        }
        if pin.len() == 2 && pin.starts_with(['D', 'd']) {
            if !self.oe {
                return LogicState::Floating;
            }
            let bit: u8 = pin[1..].parse().unwrap_or(9);
            if bit > 7 || !matches!(self.phase, Phase::Done) {
                return LogicState::Low;
            }
            return if self.code & (1 << bit) != 0 {
                LogicState::High
            } else {
                LogicState::Low
            };
        }
        LogicState::Floating
    }
}

impl Adc0808 {
    /// Latch channel and begin a conversion (START rising edge + ALE).
    pub fn start(&mut self, channel: u8) {
        self.channel = channel & 7;
        self.phase = Phase::Converting {
            left_ps: self.convert_time_ps(),
        };
        self.eoc = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adc() -> Adc0808 {
        let mut m = Adc0808::default();
        m.initialize(&HashMap::from([
            ("VREF".to_owned(), "5".to_owned()),
            ("VIN".to_owned(), "2.5".to_owned()),
            ("FCLK".to_owned(), "640000".to_owned()),
            ("OE".to_owned(), "1".to_owned()),
        ]));
        m
    }

    #[test]
    fn converts_midscale() {
        let mut m = adc();
        m.start(0);
        assert_eq!(m.pin_state("EOC"), LogicState::Low);
        // Convert in chunks smaller than t_conv: still busy, no edge.
        assert!(!m.evaluate(m.convert_time_ps() / 2));
        assert_eq!(m.pin_state("EOC"), LogicState::Low);
        // Finish: EOC rises, code = round(2.5/5*255) = 128.
        assert!(m.evaluate(m.convert_time_ps()));
        assert_eq!(m.pin_state("EOC"), LogicState::High);
        assert_eq!(m.code, 128);
        assert_eq!(m.pin_state("D7"), LogicState::High);
        assert_eq!(m.pin_state("D0"), LogicState::Low);
    }

    #[test]
    fn oe_low_floats_bus() {
        let mut m = adc();
        m.initialize(&HashMap::from([("OE".to_owned(), "0".to_owned())]));
        m.start(3);
        assert!(m.evaluate(m.convert_time_ps() + 1));
        assert_eq!(m.pin_state("D7"), LogicState::Floating);
        assert_eq!(m.pin_state("EOC"), LogicState::High);
    }

    #[test]
    fn full_scale_clamps() {
        let mut m = adc();
        m.initialize(&HashMap::from([
            ("VREF".to_owned(), "5".to_owned()),
            ("VIN".to_owned(), "99".to_owned()),
        ]));
        m.start(0);
        m.evaluate(m.convert_time_ps() + 1);
        assert_eq!(m.code, 255);
    }
}
