//! Overlay layout: which inputs, functions and virtual controls are shown, and where.

use crate::device::DeviceId;
use serde::{Deserialize, Serialize};

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
    /// A virtual control and its state, e.g. "Comms  Guard".
    VirtualControl { control: String },
}

impl OverlayWidget {
    pub fn is_control(&self, device: &DeviceId, control: ControlRef) -> bool {
        matches!(self, Self::Control { device: d, control: c, .. } if d == device && *c == control)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OverlayConfig {
    pub visible: bool,
    /// Global hotkey that shows and hides the overlay, in Tauri accelerator syntax.
    pub hotkey: String,
    pub anchor: Anchor,
    /// Distance from the anchored screen corner, in logical pixels.
    pub margin: u32,
    pub scale: f32,
    pub widgets: Vec<OverlayWidget>,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            visible: true,
            hotkey: "Ctrl+Shift+F10".into(),
            anchor: Anchor::default(),
            margin: 32,
            scale: 1.0,
            widgets: Vec::new(),
        }
    }
}

impl OverlayConfig {
    /// Adds the control if it is not shown yet, removes it otherwise. Returns whether it is
    /// shown afterwards.
    pub fn toggle_control(&mut self, device: &DeviceId, control: ControlRef) -> bool {
        if let Some(pos) = self
            .widgets
            .iter()
            .position(|w| w.is_control(device, control))
        {
            self.widgets.remove(pos);
            false
        } else {
            self.widgets.push(OverlayWidget::Control {
                device: device.clone(),
                control,
                label: None,
            });
            true
        }
    }

    pub fn toggle_function(&mut self, function: &str) -> bool {
        let is_it = |w: &OverlayWidget| matches!(w, OverlayWidget::Function { function: f } if f == function);
        if let Some(pos) = self.widgets.iter().position(is_it) {
            self.widgets.remove(pos);
            false
        } else {
            self.widgets.push(OverlayWidget::Function {
                function: function.into(),
            });
            true
        }
    }

    pub fn remove_function(&mut self, function: &str) {
        self.widgets
            .retain(|w| !matches!(w, OverlayWidget::Function { function: f } if f == function));
    }

    pub fn has_virtual_control(&self, control: &str) -> bool {
        self.widgets
            .iter()
            .any(|w| matches!(w, OverlayWidget::VirtualControl { control: c } if c == control))
    }

    pub fn toggle_virtual_control(&mut self, control: &str) -> bool {
        if self.has_virtual_control(control) {
            self.remove_virtual_control(control);
            false
        } else {
            self.widgets.push(OverlayWidget::VirtualControl {
                control: control.into(),
            });
            true
        }
    }

    pub fn remove_virtual_control(&mut self, control: &str) {
        self.widgets
            .retain(|w| !matches!(w, OverlayWidget::VirtualControl { control: c } if c == control));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_adds_then_removes() {
        let mut cfg = OverlayConfig::default();
        let dev = DeviceId("a".into());
        assert!(cfg.toggle_control(&dev, ControlRef::Axis { index: 2 }));
        assert!(cfg.toggle_control(&dev, ControlRef::Button { index: 2 }));
        assert_eq!(cfg.widgets.len(), 2);
        assert!(!cfg.toggle_control(&dev, ControlRef::Axis { index: 2 }));
        assert_eq!(cfg.widgets.len(), 1);
    }

    #[test]
    fn widget_json_shape_is_stable() {
        let w = OverlayWidget::Control {
            device: DeviceId("231d:0200#1".into()),
            control: ControlRef::Button { index: 4 },
            label: None,
        };
        assert_eq!(
            serde_json::to_string(&w).unwrap(),
            r#"{"type":"control","device":"231d:0200#1","control":{"type":"button","index":4},"label":null}"#
        );
    }
}
