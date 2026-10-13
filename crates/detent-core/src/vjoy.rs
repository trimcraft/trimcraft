//! vJoy output: Detent feeds virtual joysticks (vJoy devices 1–16) that games see as
//! controllers.
//!
//! Three things produce output, all per profile:
//! - **State outputs** of virtual controls: "while this state is active, hold vJoy button N"
//!   (or press it briefly on entering), a counter's value as an axis.
//! - **Mappings**: a physical axis (with deadzones, saturation and a response curve), button or
//!   hat passed through to a vJoy axis, button or POV.
//! - **vJoy actions**: press, release, toggle or pulse a button, set an axis, set a POV.
//!
//! The engine resolves them into one desired [`VjoyState`]; the app compares it with what it
//! last wrote ([`VjoyState::diff`]) and sends the difference through the vJoy driver. A
//! profile switch simply yields another desired state, so whatever the old profile held is
//! released and the new profile's outputs apply at once.

use crate::device::DeviceId;
use crate::trigger::HatDirection;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const MAX_DEVICES: u8 = 16;
pub const MAX_BUTTONS: u8 = 128;
pub const MAX_POVS: u8 = 4;
/// Pulse length limits (a button pressed briefly).
pub const MIN_PULSE_MS: u32 = 10;
pub const MAX_PULSE_MS: u32 = 5_000;
pub const DEFAULT_PULSE_MS: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VjoyAxis {
    X,
    Y,
    Z,
    Rx,
    Ry,
    Rz,
    Slider0,
    Slider1,
}

impl VjoyAxis {
    pub const ALL: [VjoyAxis; 8] = [
        VjoyAxis::X,
        VjoyAxis::Y,
        VjoyAxis::Z,
        VjoyAxis::Rx,
        VjoyAxis::Ry,
        VjoyAxis::Rz,
        VjoyAxis::Slider0,
        VjoyAxis::Slider1,
    ];

    /// The HID usage vJoy addresses the axis by.
    pub fn usage(self) -> u32 {
        0x30 + self as u32
    }

    pub fn name(self) -> &'static str {
        match self {
            VjoyAxis::X => "X",
            VjoyAxis::Y => "Y",
            VjoyAxis::Z => "Z",
            VjoyAxis::Rx => "Rx",
            VjoyAxis::Ry => "Ry",
            VjoyAxis::Rz => "Rz",
            VjoyAxis::Slider0 => "Slider 1",
            VjoyAxis::Slider1 => "Slider 2",
        }
    }
}

/// A vJoy button; device 1–16, button 1–128 (both as vJoy counts them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VjoyButton {
    pub device: u8,
    pub button: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VjoyAxisRef {
    pub device: u8,
    pub axis: VjoyAxis,
}

/// A vJoy POV hat, 1–4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VjoyPov {
    pub device: u8,
    pub pov: u8,
}

/// A POV direction, the same eight as a physical hat.
pub type PovDirection = HatDirection;

/// Continuous POV value in hundredths of a degree, clockwise from up.
pub fn pov_hundredths(direction: PovDirection) -> u32 {
    use HatDirection::*;
    match direction {
        Up => 0,
        UpRight => 4_500,
        Right => 9_000,
        DownRight => 13_500,
        Down => 18_000,
        DownLeft => 22_500,
        Left => 27_000,
        UpLeft => 31_500,
    }
}

/// Discrete (4-way) POV value: 0 up, 1 right, 2 down, 3 left. Diagonals use their vertical
/// part, as a 4-way hat cannot show them.
pub fn pov_discrete(direction: PovDirection) -> i32 {
    use HatDirection::*;
    match direction {
        Up | UpRight | UpLeft => 0,
        Right => 1,
        Down | DownRight | DownLeft => 2,
        Left => 3,
    }
}

/// A state output of a virtual control: hold the button while the state is active, or (with
/// `pulse_ms`) press it briefly on entering the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonOutput {
    pub button: VjoyButton,
    #[serde(default)]
    pub pulse_ms: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Curve {
    #[default]
    Linear,
    /// Flatter near the centre: `(1 - factor) * x + factor * x³`, factor 0..=1.
    Expo { factor: f32 },
}

/// How a physical axis is shaped on its way to vJoy. All amounts in percent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AxisCurve {
    pub invert: bool,
    /// Around the centre, of half the range: inside it the output is centred.
    pub deadzone_center: f32,
    /// At the low and high end, of the full range: there the output is already at its end.
    pub deadzone_low: f32,
    pub deadzone_high: f32,
    /// Largest output deflection, 100 = full.
    pub saturation: f32,
    pub curve: Curve,
}

impl Default for AxisCurve {
    fn default() -> Self {
        Self {
            invert: false,
            deadzone_center: 0.0,
            deadzone_low: 0.0,
            deadzone_high: 0.0,
            saturation: 100.0,
            curve: Curve::Linear,
        }
    }
}

impl AxisCurve {
    /// Shapes `value` (-1..=1): invert, end deadzones, centre deadzone, curve, saturation.
    pub fn apply(&self, value: f32) -> f32 {
        let mut v = if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        if self.invert {
            v = -v;
        }
        let lo = -1.0 + 2.0 * self.deadzone_low / 100.0;
        let hi = 1.0 - 2.0 * self.deadzone_high / 100.0;
        if (self.deadzone_low > 0.0 || self.deadzone_high > 0.0) && hi > lo {
            v = ((v - lo) / (hi - lo) * 2.0 - 1.0).clamp(-1.0, 1.0);
        }
        let dc = (self.deadzone_center / 100.0).clamp(0.0, 0.99);
        v = if v.abs() <= dc {
            0.0
        } else {
            v.signum() * (v.abs() - dc) / (1.0 - dc)
        };
        if let Curve::Expo { factor } = self.curve {
            let k = factor.clamp(0.0, 1.0);
            v = (1.0 - k) * v + k * v * v * v;
        }
        (v * (self.saturation / 100.0).clamp(0.0, 1.0)).clamp(-1.0, 1.0)
    }

    pub fn validate(&self) -> Result<(), VjoyError> {
        let pct = |v: f32, max: f32| v.is_finite() && (0.0..=max).contains(&v);
        let ok = pct(self.deadzone_center, 90.0)
            && pct(self.deadzone_low, 45.0)
            && pct(self.deadzone_high, 45.0)
            && pct(self.saturation, 100.0)
            && self.saturation >= 1.0
            && match self.curve {
                Curve::Linear => true,
                Curve::Expo { factor } => pct(factor, 1.0),
            };
        if ok { Ok(()) } else { Err(VjoyError::Curve) }
    }
}

/// A physical input passed through to vJoy (per profile).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum VjoyMapping {
    Axis {
        device: DeviceId,
        index: u16,
        target: VjoyAxisRef,
        #[serde(default)]
        curve: AxisCurve,
    },
    Button {
        device: DeviceId,
        index: u16,
        target: VjoyButton,
    },
    Hat {
        device: DeviceId,
        index: u16,
        target: VjoyPov,
    },
}

impl VjoyMapping {
    pub fn target_device(&self) -> u8 {
        match self {
            VjoyMapping::Axis { target, .. } => target.device,
            VjoyMapping::Button { target, .. } => target.device,
            VjoyMapping::Hat { target, .. } => target.device,
        }
    }

    pub fn validate(&self) -> Result<(), VjoyError> {
        match self {
            VjoyMapping::Axis { target, curve, .. } => {
                check_device(target.device)?;
                curve.validate()
            }
            VjoyMapping::Button { target, .. } => check_button(*target),
            VjoyMapping::Hat { target, .. } => check_pov(*target),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ButtonOp {
    /// Held until released (by another action, a profile switch or when vJoy stops).
    Press,
    Release,
    Toggle,
    /// Pressed for `ms`, then released.
    Pulse {
        ms: u32,
    },
}

/// The "vJoy" action.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum VjoyCommand {
    Button {
        button: VjoyButton,
        op: ButtonOp,
    },
    /// Sets the axis to `percent` (0..=100) of its range.
    Axis {
        axis: VjoyAxisRef,
        percent: f32,
    },
    /// Points the POV; `None` centres it.
    Pov {
        pov: VjoyPov,
        direction: Option<PovDirection>,
    },
}

impl VjoyCommand {
    pub fn device(&self) -> u8 {
        match self {
            VjoyCommand::Button { button, .. } => button.device,
            VjoyCommand::Axis { axis, .. } => axis.device,
            VjoyCommand::Pov { pov, .. } => pov.device,
        }
    }

    pub fn validate(&self) -> Result<(), VjoyError> {
        match self {
            VjoyCommand::Button { button, op } => {
                check_button(*button)?;
                if let ButtonOp::Pulse { ms } = op {
                    check_pulse(*ms)?;
                }
                Ok(())
            }
            VjoyCommand::Axis { axis, percent } => {
                check_device(axis.device)?;
                if percent.is_finite() && (0.0..=100.0).contains(percent) {
                    Ok(())
                } else {
                    Err(VjoyError::Percent)
                }
            }
            VjoyCommand::Pov { pov, .. } => check_pov(*pov),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            VjoyCommand::Button { button, op } => {
                let op = match op {
                    ButtonOp::Press => "press".to_owned(),
                    ButtonOp::Release => "release".to_owned(),
                    ButtonOp::Toggle => "toggle".to_owned(),
                    ButtonOp::Pulse { ms } => format!("pulse {ms} ms"),
                };
                format!("{op} {}", button_text(*button))
            }
            VjoyCommand::Axis { axis, percent } => {
                format!("{} to {percent} %", axis_text(*axis))
            }
            VjoyCommand::Pov { pov, direction } => format!(
                "{} {}",
                pov_text(*pov),
                direction.map_or("centre".to_owned(), |d| format!("{d:?}").to_lowercase())
            ),
        }
    }
}

pub fn button_text(b: VjoyButton) -> String {
    format!("vJoy {} button {}", b.device, b.button)
}

pub fn axis_text(a: VjoyAxisRef) -> String {
    format!("vJoy {} axis {}", a.device, a.axis.name())
}

pub fn pov_text(p: VjoyPov) -> String {
    format!("vJoy {} POV {}", p.device, p.pov)
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VjoyError {
    #[error("vJoy devices are numbered 1 to {MAX_DEVICES}")]
    Device,
    #[error("vJoy buttons are numbered 1 to {MAX_BUTTONS}")]
    Button,
    #[error("vJoy POVs are numbered 1 to {MAX_POVS}")]
    Pov,
    #[error("a pulse lasts {MIN_PULSE_MS} to {MAX_PULSE_MS} ms")]
    Pulse,
    #[error("an axis value is 0 to 100 %")]
    Percent,
    #[error(
        "deadzones: centre up to 90 %, ends up to 45 %; saturation 1 to 100 %; expo factor 0 to 1"
    )]
    Curve,
}

fn check_device(device: u8) -> Result<(), VjoyError> {
    if (1..=MAX_DEVICES).contains(&device) {
        Ok(())
    } else {
        Err(VjoyError::Device)
    }
}

fn check_button(b: VjoyButton) -> Result<(), VjoyError> {
    check_device(b.device)?;
    if (1..=MAX_BUTTONS).contains(&b.button) {
        Ok(())
    } else {
        Err(VjoyError::Button)
    }
}

fn check_pov(p: VjoyPov) -> Result<(), VjoyError> {
    check_device(p.device)?;
    if (1..=MAX_POVS).contains(&p.pov) {
        Ok(())
    } else {
        Err(VjoyError::Pov)
    }
}

fn check_pulse(ms: u32) -> Result<(), VjoyError> {
    if (MIN_PULSE_MS..=MAX_PULSE_MS).contains(&ms) {
        Ok(())
    } else {
        Err(VjoyError::Pulse)
    }
}

impl ButtonOutput {
    pub fn validate(&self) -> Result<(), VjoyError> {
        check_button(self.button)?;
        self.pulse_ms.map_or(Ok(()), check_pulse)
    }
}

/// A counter's value as a vJoy axis position (-1..=1): the minimum at one end, the maximum
/// at the other.
pub fn counter_axis(spec: &crate::function::CounterSpec, value: f64) -> f32 {
    let span = spec.max - spec.min;
    if span.is_nan() || span <= 0.0 {
        return -1.0;
    }
    (((value - spec.min) / span).clamp(0.0, 1.0) * 2.0 - 1.0) as f32
}

/// Validates a counter's axis output.
pub fn validate_axis(axis: &VjoyAxisRef) -> Result<(), VjoyError> {
    check_device(axis.device)
}

/// Everything Detent wants on the vJoy devices right now. Buttons not listed are released,
/// POVs not listed are centred; axes not listed are left where they are.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VjoyState {
    /// Devices Detent owns (the active profile uses them).
    pub devices: BTreeSet<u8>,
    pub buttons: BTreeSet<VjoyButton>,
    /// -1..=1 across the axis' range.
    pub axes: BTreeMap<VjoyAxisRef, f32>,
    pub povs: BTreeMap<VjoyPov, PovDirection>,
}

/// One write to the driver.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VjoyWrite {
    Acquire(u8),
    Relinquish(u8),
    Button(VjoyButton, bool),
    Axis(VjoyAxisRef, f32),
    Pov(VjoyPov, Option<PovDirection>),
}

impl VjoyState {
    /// The writes that turn `self` (what was written) into `target`: devices are acquired
    /// first; outputs on devices that are given up are released and centred before the
    /// device is relinquished, so nothing stays pressed.
    pub fn diff(&self, target: &VjoyState) -> Vec<VjoyWrite> {
        let mut out: Vec<VjoyWrite> = target
            .devices
            .difference(&self.devices)
            .map(|&d| VjoyWrite::Acquire(d))
            .collect();
        for b in self.buttons.difference(&target.buttons) {
            out.push(VjoyWrite::Button(*b, false));
        }
        for b in target.buttons.difference(&self.buttons) {
            out.push(VjoyWrite::Button(*b, true));
        }
        for (a, v) in &target.axes {
            if self.axes.get(a) != Some(v) {
                out.push(VjoyWrite::Axis(*a, *v));
            }
        }
        for p in self.povs.keys() {
            if !target.povs.contains_key(p) {
                out.push(VjoyWrite::Pov(*p, None));
            }
        }
        for (p, d) in &target.povs {
            if self.povs.get(p) != Some(d) {
                out.push(VjoyWrite::Pov(*p, Some(*d)));
            }
        }
        out.extend(
            self.devices
                .difference(&target.devices)
                .map(|&d| VjoyWrite::Relinquish(d)),
        );
        out
    }

    /// Keeps only outputs on owned devices (the rest could not be written anyway).
    pub fn restrict_to_devices(&mut self) {
        let devices = self.devices.clone();
        self.buttons.retain(|b| devices.contains(&b.device));
        self.axes.retain(|a, _| devices.contains(&a.device));
        self.povs.retain(|p, _| devices.contains(&p.device));
    }

    /// Applies `write` to `self` (the record of what was written).
    pub fn record(&mut self, write: &VjoyWrite) {
        match *write {
            VjoyWrite::Acquire(d) => {
                self.devices.insert(d);
            }
            VjoyWrite::Relinquish(d) => {
                self.devices.remove(&d);
                self.buttons.retain(|b| b.device != d);
                self.axes.retain(|a, _| a.device != d);
                self.povs.retain(|p, _| p.device != d);
            }
            VjoyWrite::Button(b, true) => {
                self.buttons.insert(b);
            }
            VjoyWrite::Button(b, false) => {
                self.buttons.remove(&b);
            }
            VjoyWrite::Axis(a, v) => {
                self.axes.insert(a, v);
            }
            VjoyWrite::Pov(p, Some(d)) => {
                self.povs.insert(p, d);
            }
            VjoyWrite::Pov(p, None) => {
                self.povs.remove(&p);
            }
        }
    }

    pub fn view(&self) -> VjoyStateView {
        VjoyStateView {
            devices: self.devices.iter().copied().collect(),
            buttons: self.buttons.iter().copied().collect(),
            axes: self
                .axes
                .iter()
                .map(|(axis, value)| AxisValue {
                    axis: *axis,
                    value: *value,
                })
                .collect(),
            povs: self
                .povs
                .iter()
                .map(|(pov, direction)| PovValue {
                    pov: *pov,
                    direction: *direction,
                })
                .collect(),
        }
    }
}

/// [`VjoyState`] for the UI (JSON has no struct keys).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VjoyStateView {
    pub devices: Vec<u8>,
    pub buttons: Vec<VjoyButton>,
    pub axes: Vec<AxisValue>,
    pub povs: Vec<PovValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisValue {
    pub axis: VjoyAxisRef,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PovValue {
    pub pov: VjoyPov,
    pub direction: PovDirection,
}

/// Who has a vJoy device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceOwner {
    /// Configured and free.
    Free,
    Detent,
    /// Another feeder program.
    Other,
    /// Not configured (enable it in "Configure vJoy").
    Missing,
}

/// A vJoy device as the driver reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VjoyDeviceInfo {
    pub id: u8,
    pub owner: DeviceOwner,
    pub axes: Vec<VjoyAxis>,
    pub buttons: u8,
    /// Discrete (4-way) and continuous POVs.
    pub discrete_povs: u8,
    pub continuous_povs: u8,
}

/// What the vJoy connector found: driver version and the configured devices.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VjoyInfo {
    /// E.g. "2.1.9"; `None` if the driver is not enabled.
    pub version: Option<String>,
    pub devices: Vec<VjoyDeviceInfo>,
}

/// vJoy's version word as text: 0x219 → "2.1.9".
pub fn version_text(version: u16) -> String {
    format!("{version:x}")
        .chars()
        .map(String::from)
        .collect::<Vec<_>>()
        .join(".")
}

/// Problems with what Detent wants to write on a device as configured, for the UI.
pub fn capability_problems(state: &VjoyState, devices: &[VjoyDeviceInfo]) -> Vec<String> {
    let mut out = Vec::new();
    for &d in &state.devices {
        let Some(info) = devices.iter().find(|i| i.id == d) else {
            out.push(format!(
                "vJoy device {d} is not configured; enable it in Configure vJoy"
            ));
            continue;
        };
        if info.owner == DeviceOwner::Missing {
            out.push(format!(
                "vJoy device {d} is not configured; enable it in Configure vJoy"
            ));
            continue;
        }
        if let Some(b) = state
            .buttons
            .iter()
            .filter(|b| b.device == d && b.button > info.buttons)
            .map(|b| b.button)
            .max()
        {
            out.push(format!(
                "vJoy device {d} has {} buttons, button {b} is used; add buttons in Configure vJoy",
                info.buttons
            ));
        }
        for a in state.axes.keys().filter(|a| a.device == d) {
            if !info.axes.contains(&a.axis) {
                out.push(format!(
                    "vJoy device {d} has no axis {}; enable it in Configure vJoy",
                    a.axis.name()
                ));
            }
        }
        let povs = info.discrete_povs.max(info.continuous_povs);
        if let Some(p) = state
            .povs
            .keys()
            .filter(|p| p.device == d && p.pov > povs)
            .map(|p| p.pov)
            .max()
        {
            out.push(format!(
                "vJoy device {d} has {povs} POVs, POV {p} is used; add POVs in Configure vJoy"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const B1: VjoyButton = VjoyButton {
        device: 1,
        button: 1,
    };
    const B2: VjoyButton = VjoyButton {
        device: 1,
        button: 2,
    };
    const X1: VjoyAxisRef = VjoyAxisRef {
        device: 1,
        axis: VjoyAxis::X,
    };

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn linear_curve_is_identity() {
        let c = AxisCurve::default();
        for v in [-1.0, -0.5, 0.0, 0.3, 1.0] {
            assert!(close(c.apply(v), v));
        }
        assert_eq!(c.apply(f32::NAN), 0.0);
        assert_eq!(c.apply(3.0), 1.0);
    }

    #[test]
    fn invert_deadzones_curve_and_saturation() {
        let invert = AxisCurve {
            invert: true,
            ..Default::default()
        };
        assert!(close(invert.apply(0.25), -0.25));
        let center = AxisCurve {
            deadzone_center: 10.0,
            ..Default::default()
        };
        assert_eq!(center.apply(0.09), 0.0);
        assert_eq!(center.apply(-0.1), 0.0);
        assert!(close(center.apply(0.55), 0.5));
        assert!(close(center.apply(1.0), 1.0));
        // 10 % at each end: the last tenth of the travel is already full output.
        let ends = AxisCurve {
            deadzone_low: 10.0,
            deadzone_high: 10.0,
            ..Default::default()
        };
        assert!(close(ends.apply(0.8), 1.0));
        assert!(close(ends.apply(-0.8), -1.0));
        assert!(close(ends.apply(0.0), 0.0));
        assert!(close(ends.apply(0.4), 0.5));
        let expo = AxisCurve {
            curve: Curve::Expo { factor: 1.0 },
            ..Default::default()
        };
        assert!(close(expo.apply(0.5), 0.125));
        assert!(close(expo.apply(-1.0), -1.0));
        let half = AxisCurve {
            curve: Curve::Expo { factor: 0.5 },
            ..Default::default()
        };
        assert!(close(half.apply(0.5), 0.3125));
        let sat = AxisCurve {
            saturation: 80.0,
            ..Default::default()
        };
        assert!(close(sat.apply(1.0), 0.8));
        assert!(close(sat.apply(-0.5), -0.4));
        assert!(AxisCurve::default().validate().is_ok());
        assert_eq!(
            AxisCurve {
                saturation: 0.0,
                ..Default::default()
            }
            .validate(),
            Err(VjoyError::Curve)
        );
    }

    #[test]
    fn diff_acquires_first_and_releases_before_relinquishing() {
        let mut old = VjoyState::default();
        old.devices.extend([1, 2]);
        old.buttons.extend([
            B1,
            VjoyButton {
                device: 2,
                button: 5,
            },
        ]);
        old.povs
            .insert(VjoyPov { device: 2, pov: 1 }, HatDirection::Up);
        let mut new = VjoyState::default();
        new.devices.extend([1, 3]);
        new.buttons.insert(B2);
        new.axes.insert(X1, 0.5);
        let writes = old.diff(&new);
        assert_eq!(
            writes,
            [
                VjoyWrite::Acquire(3),
                VjoyWrite::Button(B1, false),
                VjoyWrite::Button(
                    VjoyButton {
                        device: 2,
                        button: 5
                    },
                    false
                ),
                VjoyWrite::Button(B2, true),
                VjoyWrite::Axis(X1, 0.5),
                VjoyWrite::Pov(VjoyPov { device: 2, pov: 1 }, None),
                VjoyWrite::Relinquish(2),
            ]
        );
        let mut written = old.clone();
        for w in &writes {
            written.record(w);
        }
        assert_eq!(written, new);
        assert!(written.diff(&new).is_empty());
    }

    #[test]
    fn counter_values_scale_onto_the_axis() {
        let spec = crate::function::CounterSpec {
            min: 1.0,
            max: 5.0,
            ..Default::default()
        };
        assert_eq!(counter_axis(&spec, 1.0), -1.0);
        assert_eq!(counter_axis(&spec, 3.0), 0.0);
        assert_eq!(counter_axis(&spec, 5.0), 1.0);
        assert_eq!(counter_axis(&spec, 9.0), 1.0);
    }

    #[test]
    fn povs_and_versions() {
        assert_eq!(pov_hundredths(HatDirection::DownLeft), 22_500);
        assert_eq!(pov_discrete(HatDirection::UpRight), 0);
        assert_eq!(pov_discrete(HatDirection::Left), 3);
        assert_eq!(version_text(0x219), "2.1.9");
        assert_eq!(VjoyAxis::Slider1.usage(), 0x37);
    }

    #[test]
    fn validation() {
        let pulse = |ms| VjoyCommand::Button {
            button: B1,
            op: ButtonOp::Pulse { ms },
        };
        assert!(pulse(100).validate().is_ok());
        assert_eq!(pulse(1).validate(), Err(VjoyError::Pulse));
        assert_eq!(
            VjoyCommand::Button {
                button: VjoyButton {
                    device: 17,
                    button: 1
                },
                op: ButtonOp::Press
            }
            .validate(),
            Err(VjoyError::Device)
        );
        assert_eq!(
            VjoyCommand::Axis {
                axis: X1,
                percent: 101.0
            }
            .validate(),
            Err(VjoyError::Percent)
        );
        assert_eq!(
            ButtonOutput {
                button: VjoyButton {
                    device: 1,
                    button: 0
                },
                pulse_ms: None
            }
            .validate(),
            Err(VjoyError::Button)
        );
    }

    #[test]
    fn capability_problems_point_to_configure_vjoy() {
        let devices = [VjoyDeviceInfo {
            id: 1,
            owner: DeviceOwner::Detent,
            axes: vec![VjoyAxis::X, VjoyAxis::Y, VjoyAxis::Z],
            buttons: 0,
            discrete_povs: 0,
            continuous_povs: 0,
        }];
        let mut s = VjoyState::default();
        s.devices.extend([1, 2]);
        s.buttons.insert(B2);
        s.axes.insert(
            VjoyAxisRef {
                device: 1,
                axis: VjoyAxis::Rz,
            },
            0.0,
        );
        let p = capability_problems(&s, &devices);
        assert_eq!(p.len(), 3, "{p:?}");
        assert!(p.iter().all(|t| t.contains("Configure vJoy")));
    }

    #[test]
    fn json_shapes() {
        let c = VjoyCommand::Button {
            button: B1,
            op: ButtonOp::Pulse { ms: 80 },
        };
        assert_eq!(
            serde_json::to_value(c).unwrap(),
            serde_json::json!({"type":"button","button":{"device":1,"button":1},"op":{"type":"pulse","ms":80}})
        );
        let m: VjoyMapping = serde_json::from_str(
            r#"{"type":"axis","device":"d","index":2,"target":{"device":1,"axis":"slider0"}}"#,
        )
        .unwrap();
        let VjoyMapping::Axis { curve, .. } = m else {
            panic!()
        };
        assert_eq!(curve, AxisCurve::default());
        let pov: VjoyCommand =
            serde_json::from_str(r#"{"type":"pov","pov":{"device":1,"pov":1},"direction":null}"#)
                .unwrap();
        assert_eq!(pov.describe(), "vJoy 1 POV 1 centre");
    }
}
