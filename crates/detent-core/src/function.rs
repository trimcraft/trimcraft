//! Functions: named things the user cares about, like "Radio channel".
//!
//! Stateful functions hold a value with a range and are shown in the overlay. Stage 1 has the
//! counter; selectors (one of several named positions) and modes (selectors that switch
//! bindings) follow with mapping in stage 2.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EndBehavior {
    /// Past the maximum continue at the minimum and vice versa.
    #[default]
    Wrap,
    /// Stop at the ends.
    Clamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CounterSpec {
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub initial: f64,
    pub end: EndBehavior,
    /// Decimal places shown, e.g. 3 for 118.025.
    pub decimals: u8,
}

impl Default for CounterSpec {
    fn default() -> Self {
        Self {
            min: 1.0,
            max: 20.0,
            step: 1.0,
            initial: 1.0,
            end: EndBehavior::Wrap,
            decimals: 0,
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum SpecError {
    #[error("step must be greater than zero")]
    Step,
    #[error("maximum must be greater than minimum")]
    Range,
    #[error("range holds too many steps ({0}); use a larger step")]
    TooManySteps(f64),
}

/// Upper bound on positions, so a typo like step 0.0000001 cannot produce absurd ranges.
const MAX_POSITIONS: f64 = 1_000_000.0;

impl CounterSpec {
    pub fn validate(&self) -> Result<(), SpecError> {
        if !self.step.is_finite() || self.step <= 0.0 {
            return Err(SpecError::Step);
        }
        if !self.min.is_finite() || !self.max.is_finite() || self.max <= self.min {
            return Err(SpecError::Range);
        }
        let positions = (self.max - self.min) / self.step;
        if positions > MAX_POSITIONS {
            return Err(SpecError::TooManySteps(positions));
        }
        Ok(())
    }

    /// Highest position index; positions are `min + i * step` for `i` in `0..=last`.
    /// Values are kept as integer positions so repeated steps never accumulate float error.
    pub fn last_position(&self) -> i64 {
        ((self.max - self.min) / self.step + 1e-9).floor() as i64
    }

    pub fn position_of(&self, value: f64) -> i64 {
        (((value - self.min) / self.step).round() as i64).clamp(0, self.last_position())
    }

    pub fn value_at(&self, position: i64) -> f64 {
        self.min + position as f64 * self.step
    }

    /// Moves `steps` positions (negative = down) from `position`.
    pub fn advance(&self, position: i64, steps: i64) -> i64 {
        let last = self.last_position();
        let target = position.saturating_add(steps);
        match self.end {
            EndBehavior::Clamp => target.clamp(0, last),
            EndBehavior::Wrap => target.rem_euclid(last + 1),
        }
    }

    pub fn format(&self, value: f64) -> String {
        format!("{value:.prec$}", prec = usize::from(self.decimals))
    }
}

/// Where a counter gets its steps from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum CounterInput {
    /// One step per encoder detent.
    Encoder {
        encoder: String,
    },
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum FunctionKind {
    Counter {
        spec: CounterSpec,
        input: CounterInput,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Function {
    pub id: String,
    /// Shown in the overlay, e.g. "Channel".
    pub name: String,
    pub kind: FunctionKind,
}

impl Function {
    pub fn validate(&self) -> Result<(), SpecError> {
        match &self.kind {
            FunctionKind::Counter { spec, .. } => spec.validate(),
        }
    }

    pub fn initial_position(&self) -> i64 {
        match &self.kind {
            FunctionKind::Counter { spec, .. } => spec.position_of(spec.initial),
        }
    }

    pub fn encoder(&self) -> Option<&str> {
        match &self.kind {
            FunctionKind::Counter {
                input: CounterInput::Encoder { encoder },
                ..
            } => Some(encoder),
            FunctionKind::Counter {
                input: CounterInput::None,
                ..
            } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(end: EndBehavior) -> CounterSpec {
        CounterSpec {
            min: 1.0,
            max: 5.0,
            step: 1.0,
            initial: 1.0,
            end,
            decimals: 0,
        }
    }

    #[test]
    fn wraps_both_ways() {
        let s = channel(EndBehavior::Wrap);
        assert_eq!(s.last_position(), 4);
        assert_eq!(s.advance(4, 1), 0);
        assert_eq!(s.advance(0, -1), 4);
        assert_eq!(s.advance(1, 13), 4); // 1 + 13 = 14 -> 14 mod 5
    }

    #[test]
    fn clamps_at_ends() {
        let s = channel(EndBehavior::Clamp);
        assert_eq!(s.advance(4, 3), 4);
        assert_eq!(s.advance(0, -3), 0);
    }

    #[test]
    fn decimal_steps_do_not_drift() {
        let s = CounterSpec {
            min: 118.0,
            max: 136.975,
            step: 0.025,
            initial: 121.5,
            end: EndBehavior::Wrap,
            decimals: 3,
        };
        assert_eq!(s.validate(), Ok(()));
        let mut p = s.position_of(s.initial);
        for _ in 0..1000 {
            p = s.advance(p, 1);
        }
        for _ in 0..1000 {
            p = s.advance(p, -1);
        }
        assert_eq!(s.format(s.value_at(p)), "121.500");
        assert_eq!(s.format(s.value_at(s.last_position())), "136.975");
    }

    #[test]
    fn rejects_bad_specs() {
        let mut s = channel(EndBehavior::Wrap);
        s.step = 0.0;
        assert_eq!(s.validate(), Err(SpecError::Step));
        s.step = 1.0;
        s.max = 1.0;
        assert_eq!(s.validate(), Err(SpecError::Range));
        s.max = 1e9;
        s.step = 1.0;
        assert!(matches!(s.validate(), Err(SpecError::TooManySteps(_))));
    }

    #[test]
    fn json_shape_is_stable() {
        let f = Function {
            id: "radio".into(),
            name: "Channel".into(),
            kind: FunctionKind::Counter {
                spec: channel(EndBehavior::Wrap),
                input: CounterInput::Encoder {
                    encoder: "enc1".into(),
                },
            },
        };
        let json = serde_json::to_value(&f).unwrap();
        assert_eq!(json["kind"]["type"], "counter");
        assert_eq!(json["kind"]["input"]["type"], "encoder");
        assert_eq!(json["kind"]["spec"]["end"], "wrap");
        assert_eq!(serde_json::from_value::<Function>(json).unwrap(), f);
    }
}
