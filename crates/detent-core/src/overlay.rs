//! Overlay layout: which inputs, functions and virtual controls are shown, and where.
//!
//! Each widget has its own position, stored as fractions of the overlay monitor's width and
//! height so it survives resolution changes. Widgets without a position (all widgets from
//! setups older than free positioning, and newly added ones until they are placed) are
//! stacked in the anchored corner as before; entering layout edit mode fixes their measured
//! positions.

use crate::device::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ControlRef {
    Axis { index: u16 },
    Button { index: u16 },
    Hat { index: u16 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Anchor {
    TopLeft,
    TopRight,
    #[default]
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RadioLayout {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum OverlayWidget {
    /// A raw input: axis as bar, button as lamp, hat as rose.
    Control {
        device: DeviceId,
        control: ControlRef,
        #[serde(default)]
        label: Option<String>,
    },
    /// The current value of a stateful function, e.g. "Channel 3".
    Function { function: String },
    /// A radio with all its states, the active one highlighted.
    Radio {
        control: String,
        #[serde(default)]
        layout: RadioLayout,
    },
    /// The name of the active profile.
    ProfileName,
}

impl OverlayWidget {
    pub fn is_control(&self, device: &DeviceId, control: ControlRef) -> bool {
        matches!(self, Self::Control { device: d, control: c, .. } if d == device && *c == control)
    }

    /// Shows the same thing (same input, function or control), settings aside.
    pub fn same_target(&self, other: &OverlayWidget) -> bool {
        match (self, other) {
            (
                Self::Control {
                    device, control, ..
                },
                other,
            ) => other.is_control(device, *control),
            (Self::Function { function }, other) => other.is_function(function),
            (Self::Radio { control, .. }, other) => other.is_radio(control),
            (Self::ProfileName, other) => *other == Self::ProfileName,
        }
    }

    fn is_radio(&self, id: &str) -> bool {
        matches!(self, Self::Radio { control, .. } if control == id)
    }

    fn is_function(&self, id: &str) -> bool {
        matches!(self, Self::Function { function } if function == id)
    }
}

/// Top-left corner of a widget, as fractions (0..=1) of the monitor's width and height.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WidgetPosition {
    pub x: f32,
    pub y: f32,
}

impl WidgetPosition {
    /// Clamped into the monitor; non-finite values become 0.
    pub fn clamped(self) -> Self {
        let c = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        Self {
            x: c(self.x),
            y: c(self.y),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayItem {
    #[serde(flatten)]
    pub widget: OverlayWidget,
    /// `None`: stacked in the anchored corner.
    #[serde(default)]
    pub position: Option<WidgetPosition>,
}

impl From<OverlayWidget> for OverlayItem {
    fn from(widget: OverlayWidget) -> Self {
        Self {
            widget,
            position: None,
        }
    }
}

/// A monitor as the OS reports it; also how the chosen overlay monitor is remembered.
/// Position and size are in physical pixels of the virtual desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorRef {
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Finds the saved monitor among the available ones: same name and position first, then
/// the only monitor with that name (it moved in the arrangement), then whatever monitor is
/// at the saved position (it was renamed, e.g. by a driver update). `None` if none fits.
pub fn find_monitor(saved: &MonitorRef, available: &[MonitorRef]) -> Option<usize> {
    let same_pos = |m: &MonitorRef| m.x == saved.x && m.y == saved.y;
    if saved.name.is_some() {
        if let Some(i) = available
            .iter()
            .position(|m| m.name == saved.name && same_pos(m))
        {
            return Some(i);
        }
        let named: Vec<usize> = (0..available.len())
            .filter(|&i| available[i].name == saved.name)
            .collect();
        if let [only] = named[..] {
            return Some(only);
        }
    }
    available.iter().position(same_pos)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OverlayConfig {
    pub visible: bool,
    /// Global hotkey that shows and hides the overlay, in Tauri accelerator syntax.
    pub hotkey: String,
    /// The monitor the overlay covers; `None` is the primary monitor.
    pub monitor: Option<MonitorRef>,
    /// Corner where widgets without a position are stacked.
    pub anchor: Anchor,
    /// Distance of that stack from the corner, in logical pixels.
    pub margin: u32,
    pub scale: f32,
    /// The active profile's widgets. The setup file stores them per profile, so there the
    /// list is empty and left out.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub widgets: WidgetList,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            visible: true,
            hotkey: "Ctrl+Shift+F10".into(),
            monitor: None,
            anchor: Anchor::default(),
            margin: 32,
            scale: 1.0,
            widgets: WidgetList::default(),
        }
    }
}

/// The widgets of one profile, in drawing order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WidgetList(pub Vec<OverlayItem>);

impl std::ops::Deref for WidgetList {
    type Target = Vec<OverlayItem>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for WidgetList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec<OverlayItem>> for WidgetList {
    fn from(items: Vec<OverlayItem>) -> Self {
        Self(items)
    }
}

impl WidgetList {
    /// Adds the widget if `is_it` matches none, removes the match otherwise. Returns whether
    /// it is shown afterwards.
    fn toggle(&mut self, is_it: impl Fn(&OverlayWidget) -> bool, new: OverlayWidget) -> bool {
        if let Some(pos) = self.iter().position(|w| is_it(&w.widget)) {
            self.remove(pos);
            false
        } else {
            self.push(new.into());
            true
        }
    }

    pub fn toggle_control(&mut self, device: &DeviceId, control: ControlRef) -> bool {
        self.toggle(
            |w| w.is_control(device, control),
            OverlayWidget::Control {
                device: device.clone(),
                control,
                label: None,
            },
        )
    }

    pub fn toggle_function(&mut self, function: &str) -> bool {
        self.toggle(
            |w| w.is_function(function),
            OverlayWidget::Function {
                function: function.into(),
            },
        )
    }

    pub fn remove_function(&mut self, function: &str) {
        self.retain(|w| !w.widget.is_function(function));
    }

    pub fn has_virtual_control(&self, control: &str) -> bool {
        self.iter().any(|w| w.widget.is_radio(control))
    }

    pub fn toggle_virtual_control(&mut self, control: &str) -> bool {
        self.toggle(
            |w| w.is_radio(control),
            OverlayWidget::Radio {
                control: control.into(),
                layout: RadioLayout::default(),
            },
        )
    }

    pub fn remove_virtual_control(&mut self, control: &str) {
        self.retain(|w| !w.widget.is_radio(control));
    }

    pub fn has_profile_name(&self) -> bool {
        self.iter().any(|w| w.widget == OverlayWidget::ProfileName)
    }

    pub fn toggle_profile_name(&mut self) -> bool {
        self.toggle(
            |w| *w == OverlayWidget::ProfileName,
            OverlayWidget::ProfileName,
        )
    }

    /// Points radio widgets at controls that got a new id (old id to new id).
    pub fn rename_virtual_controls(&mut self, renamed: &HashMap<String, String>) {
        for item in self.iter_mut() {
            if let OverlayWidget::Radio { control, .. } = &mut item.widget
                && let Some(new) = renamed.get(control)
            {
                control.clone_from(new);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_adds_then_removes() {
        let mut cfg = WidgetList::default();
        let dev = DeviceId("a".into());
        assert!(cfg.toggle_control(&dev, ControlRef::Axis { index: 2 }));
        assert!(cfg.toggle_control(&dev, ControlRef::Button { index: 2 }));
        assert_eq!(cfg.len(), 2);
        assert!(!cfg.toggle_control(&dev, ControlRef::Axis { index: 2 }));
        assert_eq!(cfg.len(), 1);
        assert!(cfg.toggle_virtual_control("vc-1"));
        assert!(cfg.has_virtual_control("vc-1"));
        cfg.rename_virtual_controls(&HashMap::from([("vc-1".into(), "vc-7".into())]));
        assert!(cfg.has_virtual_control("vc-7"));
        cfg.remove_virtual_control("vc-7");
        assert!(!cfg.has_virtual_control("vc-7"));
        assert!(cfg.toggle_profile_name());
        assert!(cfg.has_profile_name());
        assert!(!cfg.toggle_profile_name());
        assert!(!cfg.has_profile_name());
    }

    #[test]
    fn widget_json_shape_is_stable() {
        let w: OverlayItem = OverlayWidget::Control {
            device: DeviceId("231d:0200#1".into()),
            control: ControlRef::Button { index: 4 },
            label: None,
        }
        .into();
        assert_eq!(
            serde_json::to_string(&w).unwrap(),
            r#"{"type":"control","device":"231d:0200#1","control":{"type":"button","index":4},"label":null,"position":null}"#
        );
        let radio = OverlayItem {
            widget: OverlayWidget::Radio {
                control: "vc-1".into(),
                layout: RadioLayout::Horizontal,
            },
            position: Some(WidgetPosition { x: 0.5, y: 0.25 }),
        };
        assert_eq!(
            serde_json::to_string(&radio).unwrap(),
            r#"{"type":"radio","control":"vc-1","layout":"horizontal","position":{"x":0.5,"y":0.25}}"#
        );
        assert_eq!(
            serde_json::from_str::<OverlayItem>(&serde_json::to_string(&radio).unwrap()).unwrap(),
            radio
        );
        let name: OverlayItem = OverlayWidget::ProfileName.into();
        assert_eq!(
            serde_json::to_string(&name).unwrap(),
            r#"{"type":"profileName","position":null}"#
        );
        assert_eq!(
            serde_json::from_str::<OverlayItem>(r#"{"type":"profileName","position":null}"#)
                .unwrap(),
            name
        );
    }

    #[test]
    fn old_layout_loads_with_widgets_stacked_at_the_anchor() {
        let cfg: OverlayConfig = serde_json::from_str(
            r#"{"visible":true,"hotkey":"Ctrl+Shift+F10","anchor":"topRight","margin":40,
                "scale":1.5,"widgets":[{"type":"function","function":"fn-1"},
                {"type":"control","device":"d","control":{"type":"axis","index":0}}]}"#,
        )
        .unwrap();
        assert_eq!(cfg.anchor, Anchor::TopRight);
        assert_eq!(cfg.monitor, None);
        assert_eq!(cfg.widgets.len(), 2);
        assert!(cfg.widgets.iter().all(|w| w.position.is_none()));
    }

    #[test]
    fn positions_are_clamped() {
        let p = WidgetPosition {
            x: -0.5,
            y: f32::NAN,
        }
        .clamped();
        assert_eq!(p, WidgetPosition { x: 0.0, y: 0.0 });
        assert_eq!(
            WidgetPosition { x: 1.5, y: 0.3 }.clamped(),
            WidgetPosition { x: 1.0, y: 0.3 }
        );
    }

    fn mon(name: &str, x: i32) -> MonitorRef {
        MonitorRef {
            name: Some(name.into()),
            x,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }

    #[test]
    fn monitors_are_found_by_name_and_position() {
        let available = [mon("DELL", 0), mon("LG", 1920), mon("LG", 3840)];
        assert_eq!(find_monitor(&mon("LG", 3840), &available), Some(2));
        // Only one with that name: found although it moved.
        assert_eq!(find_monitor(&mon("DELL", -1920), &available), Some(0));
        // Two with that name, none at the saved position: ambiguous, fall back to position.
        assert_eq!(find_monitor(&mon("LG", 0), &available), Some(0));
        // Renamed, same position.
        assert_eq!(find_monitor(&mon("Generic", 1920), &available), Some(1));
        // Gone.
        assert_eq!(find_monitor(&mon("Old", 7680), &available), None);
    }
}
