//! Device identity.
//!
//! A [`Fingerprint`] is what the operating system tells us about a device. A [`DeviceId`] is
//! the stable name Trimcraft gives it; the [`crate::DeviceRegistry`] maps one to the other.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable identifier of a device, valid across restarts and re-plugging.
///
/// Format: `vvvv:pppp#n`, vendor and product id in hex plus an ordinal that tells identical
/// models apart. The ordinal is assigned once and then remembered by the registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(pub String);

impl DeviceId {
    pub fn new(vendor_id: u16, product_id: u16, ordinal: u32) -> Self {
        Self(format!("{vendor_id:04x}:{product_id:04x}#{ordinal}"))
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the OS reports about a device. Used only to recognise a device again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub vendor_id: u16,
    pub product_id: u16,
    /// USB serial number, if the device reports a non-empty one. The strongest identity signal.
    pub serial: Option<String>,
    /// OS device path. Usually tied to the USB port the device is plugged into.
    pub path: Option<String>,
    pub name: String,
}

impl Fingerprint {
    pub fn same_model(&self, other: &Fingerprint) -> bool {
        self.vendor_id == other.vendor_id && self.product_id == other.product_id
    }

    /// Serial number, ignoring empty or whitespace-only values some devices report.
    pub fn usable_serial(&self) -> Option<&str> {
        self.serial
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    pub fn kind(&self) -> DeviceKind {
        match VirtualDriver::detect(self) {
            Some(driver) => DeviceKind::Virtual { driver },
            None => DeviceKind::Physical,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum DeviceKind {
    Physical,
    /// A device created by a virtual-device driver. Trimcraft will write to these later, so
    /// they must never be confused with physical input devices.
    Virtual {
        driver: VirtualDriver,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VirtualDriver {
    VJoy,
}

impl VirtualDriver {
    /// vJoy devices (all forks) report vendor 0x1234 and product 0xBEAD.
    const VJOY: (u16, u16) = (0x1234, 0xBEAD);

    pub fn detect(fp: &Fingerprint) -> Option<Self> {
        ((fp.vendor_id, fp.product_id) == Self::VJOY).then_some(Self::VJoy)
    }
}

/// How sure the registry is that a device got the same id as last time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IdentityConfidence {
    /// Matched by serial number.
    Serial,
    /// Matched by OS path (same USB port as before).
    Path,
    /// The only known device of this model that was missing, so it must be this one.
    OnlyCandidate,
    /// First time this device is seen.
    New,
    /// Several identical devices without serial numbers changed ports at once; the id may
    /// belong to its twin. The UI asks the user to confirm.
    Ambiguous,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(vid: u16, pid: u16) -> Fingerprint {
        Fingerprint {
            vendor_id: vid,
            product_id: pid,
            serial: None,
            path: None,
            name: "x".into(),
        }
    }

    #[test]
    fn id_format_is_hex_with_ordinal() {
        assert_eq!(DeviceId::new(0x231d, 0x0200, 2).0, "231d:0200#2");
    }

    #[test]
    fn vjoy_is_virtual() {
        assert_eq!(
            fp(0x1234, 0xbead).kind(),
            DeviceKind::Virtual {
                driver: VirtualDriver::VJoy
            }
        );
        assert_eq!(fp(0x1234, 0xbeae).kind(), DeviceKind::Physical);
    }

    #[test]
    fn blank_serial_is_ignored() {
        let mut f = fp(1, 2);
        f.serial = Some("  ".into());
        assert_eq!(f.usable_serial(), None);
        f.serial = Some(" A1 ".into());
        assert_eq!(f.usable_serial(), Some("A1"));
    }
}
