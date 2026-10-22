//! Native behavioral NE555 timer (astable / monostable).
//!
//! Datasheet timing, no SPICE internals: astable
//! `t_high = 0.693·(RA+RB)·C`, `t_low = 0.693·RB·C`,
//! monostable `t_w = 1.1·R·C`. `RA`/`RB`/`R`/`C` come from
//! `initialize` properties (`CLOCK`, `RES`, `CAP` style values like
//! `9k1`/`4u7` are normalized the same way as
//! [`asim`](https://github.com/Diez111/Atenea) does).

use std::collections::HashMap;
use vsm::{ActiveModel, DsimModel, LogicState, SpiceModel};

/// Operating mode selected by wiring (simplified): astable when both
/// `RA` and `RB` are present, monostable with `R` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Free-running oscillator.
    #[default]
    Astable,
    /// One-shot on trigger (simplified: retriggers every `t_w`).
    Monostable,
}

/// Behavioral NE555.
#[derive(Debug, Clone, Default)]
pub struct Ne555 {
    /// Timing resistor(s) in ohms.
    pub ra: f64,
    /// Second timing resistor in ohms (astable).
    pub rb: f64,
    /// Timing capacitor in farads.
    pub c: f64,
    /// Selected mode.
    pub mode: Mode,
    /// Accumulated time in ps.
    pub elapsed_ps: u64,
    /// Current output state.
    pub out: bool,
    /// Period in ps (computed at `initialize`).
    pub period_ps: u64,
    /// High time in ps (astable) or pulse width (monostable).
    pub high_ps: u64,
}

/// Parse a value with optional scale suffix (`9k1`, `4u7`, `100n`,
/// `10u`, `5`, `1MEG`, `2m2`).
///
/// Uppercase `M` is mega (Proteus style); lowercase `m` is milli (SPICE
/// style). Returns `None` on malformed input.
fn parse_value(s: &str) -> Option<f64> {
    let s = s.trim();
    // Long `MEG` suffix first.
    if let Some(num) = s.strip_suffix("MEG").or_else(|| s.strip_suffix("meg")) {
        return num.trim().parse::<f64>().ok().map(|v| v * 1e6);
    }
    let pos = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (head, tail) = s.split_at(pos);
    if head.is_empty() || !head.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut chars = tail.chars();
    let letter = chars.next()?;
    let rest: String = chars.collect();
    if !rest.is_empty() && !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mult = match letter {
        'R' | 'r' => 1.0,
        'K' | 'k' => 1e3,
        'M' => 1e6,
        'm' => 1e-3,
        'U' | 'u' => 1e-6,
        'N' | 'n' => 1e-9,
        'P' | 'p' => 1e-12,
        'F' | 'f' => 1e-15,
        'G' | 'g' => 1e9,
        'T' | 't' => 1e12,
        _ => return None,
    };
    let whole: f64 = head.parse().ok()?;
    let frac: f64 = if rest.is_empty() {
        0.0
    } else {
        format!("0.{rest}").parse().ok()?
    };
    Some((whole + frac) * mult)
}

impl Ne555 {
    /// Recompute `high_ps`/`period_ps` from RA/RB/C (seconds → ps).
    /// Casts are safe: timing values are sub-second and clamped >= 1 ps,
    /// far below `u64::MAX`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn recompute(&mut self) {
        use std::f64::consts::LN_2;
        let (high_s, period_s) = match self.mode {
            Mode::Astable if self.c > 0.0 => {
                let high = LN_2 * (self.ra + self.rb) * self.c;
                let low = LN_2 * self.rb * self.c;
                (high, high + low)
            }
            _ => {
                let w = 1.1 * self.ra.max(self.rb) * self.c;
                (w, w)
            }
        };
        self.high_ps = (high_s * 1e12).max(1.0) as u64;
        self.period_ps = (period_s * 1e12).max(1.0) as u64;
    }
}

impl SpiceModel for Ne555 {
    fn primitive(&self) -> &'static str {
        "555"
    }
}

impl ActiveModel for Ne555 {
    fn initialize(&mut self, props: &HashMap<String, String>) {
        let get = |k: &str| props.get(k).and_then(|v| parse_value(v));
        self.rb = get("RB").unwrap_or(0.0);
        self.c = get("C").unwrap_or(0.0);
        if let Some(ra) = get("RA") {
            self.ra = ra;
            self.mode = Mode::Astable;
        } else if let Some(r) = get("R") {
            self.ra = r;
            self.mode = Mode::Monostable;
        }
        self.elapsed_ps = 0;
        self.out = false;
        self.recompute();
    }

    fn dsim(&mut self) -> Option<&mut dyn DsimModel> {
        Some(self)
    }
}

impl DsimModel for Ne555 {
    fn evaluate(&mut self, time_ps: u64) -> bool {
        self.elapsed_ps = self.elapsed_ps.saturating_add(time_ps);
        let phase = self.elapsed_ps % self.period_ps.max(1);
        let next = phase < self.high_ps;
        let edge = next != self.out;
        self.out = next;
        edge
    }

    fn pin_state(&self, pin: &str) -> LogicState {
        if pin.eq_ignore_ascii_case("OUT") {
            if self.out {
                LogicState::High
            } else {
                LogicState::Low
            }
        } else {
            LogicState::Floating
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn astable() -> Ne555 {
        // RA=1k, RB=1k, C=100n -> f = 1.44/((1k+2k)*100n) ~= 4.8 kHz.
        let mut m = Ne555::default();
        m.initialize(&HashMap::from([
            ("RA".to_owned(), "1k".to_owned()),
            ("RB".to_owned(), "1k".to_owned()),
            ("C".to_owned(), "100n".to_owned()),
        ]));
        m
    }

    #[test]
    fn values_parse() {
        assert!((parse_value("9k1").unwrap() - 9100.0).abs() < 1e-9);
        assert!((parse_value("4u7").unwrap() - 4.7e-6).abs() < 1e-15);
        assert!((parse_value("4M7").unwrap() - 4.7e6).abs() < 1e-3);
        assert!((parse_value("4m7").unwrap() - 4.7e-3).abs() < 1e-12);
        assert!((parse_value("1MEG").unwrap() - 1e6).abs() < 1.0);
        assert_eq!(parse_value("nope"), None);
    }

    #[test]
    fn astable_frequency() {
        let m = astable();
        assert_eq!(m.mode, Mode::Astable);
        // 1.44/((1k+2k)*100n) = 4800 Hz -> period 208_333_333 ps nominal.
        // high = 0.693*2k*100n = 138.6 us; allow +-2%.
        assert!(
            (204_000_000..=212_000_000).contains(&m.period_ps),
            "period={}",
            m.period_ps
        );
        assert!(
            (136_000_000..=141_000_000).contains(&m.high_ps),
            "high={}",
            m.high_ps
        );
        // Duty = (RA+RB)/(RA+2RB) = 2/3 -> integer percent in [65, 68].
        let duty_pct = m.high_ps * 100 / m.period_ps.max(1);
        assert!((65..=68).contains(&duty_pct), "duty={duty_pct}%");
    }

    #[test]
    fn toggles_on_edges_only() {
        let mut m = astable();
        // Find two consecutive rising edges; distance must equal one period.
        let mut rises = Vec::new();
        let mut t = 0u64;
        while rises.len() < 2 && t < m.period_ps.saturating_mul(4) {
            let was_low = !m.out;
            let edge = m.evaluate(1000);
            t += 1000;
            if edge && was_low && m.out {
                rises.push(t);
            }
        }
        assert_eq!(rises.len(), 2, "two rising edges");
        // Edge detection quantizes to the 1000 ps step.
        let gap = rises[1].saturating_sub(rises[0]);
        assert!(
            gap >= m.period_ps.saturating_sub(1000) && gap <= m.period_ps.saturating_add(1000),
            "gap {gap} vs period {}",
            m.period_ps
        );
        assert!(matches!(
            m.pin_state("OUT"),
            LogicState::High | LogicState::Low
        ));
        assert_eq!(m.pin_state("VCC"), LogicState::Floating);
    }

    #[test]
    fn monostable_from_r() {
        let mut m = Ne555::default();
        m.initialize(&HashMap::from([
            ("R".to_owned(), "10k".to_owned()),
            ("C".to_owned(), "10u".to_owned()),
        ]));
        assert_eq!(m.mode, Mode::Monostable);
        // t_w = 1.1 * 10k * 10u = 0.11 s = 1.1e11 ps, allow +-1%.
        assert!((108_900_000_000..=111_100_000_000).contains(&m.high_ps));
    }
}
