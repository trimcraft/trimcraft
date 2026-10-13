//! Maps OS fingerprints to stable [`DeviceId`]s and remembers them across restarts.
//!
//! Matching order, strongest signal first:
//! 1. serial number (survives port changes and reboots),
//! 2. OS path (same USB port as last time),
//! 3. the only missing device of that model (a single device moved to another port),
//! 4. otherwise a new id.
//!
//! Identical devices without serial numbers that change ports at the same time cannot be
//! told apart by any software; they get [`IdentityConfidence::Ambiguous`] so the UI can ask.

use crate::device::{DeviceId, Fingerprint, IdentityConfidence};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownDevice {
    pub id: DeviceId,
    pub ordinal: u32,
    pub fingerprint: Fingerprint,
    /// User-given name, e.g. "Left throttle".
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub id: DeviceId,
    pub confidence: IdentityConfidence,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRegistry {
    pub known: Vec<KnownDevice>,
}

impl DeviceRegistry {
    pub fn get(&self, id: &DeviceId) -> Option<&KnownDevice> {
        self.known.iter().find(|k| &k.id == id)
    }

    pub fn set_label(&mut self, id: &DeviceId, label: Option<String>) -> bool {
        let label = label.map(|l| l.trim().to_owned()).filter(|l| !l.is_empty());
        match self.known.iter_mut().find(|k| &k.id == id) {
            Some(k) => {
                k.label = label;
                true
            }
            None => false,
        }
    }

    /// Resolves one newly connected device. `connected` holds the ids already in use.
    pub fn resolve(&mut self, fp: &Fingerprint, connected: &HashSet<DeviceId>) -> Resolution {
        self.resolve_batch(std::slice::from_ref(fp), connected)
            .remove(0)
    }

    /// Resolves several devices that appeared together (e.g. at startup).
    ///
    /// Batch resolution matters: exact matches must be handed out before the fallbacks run,
    /// otherwise a device processed early could take the id of a twin processed later.
    pub fn resolve_batch(
        &mut self,
        fps: &[Fingerprint],
        connected: &HashSet<DeviceId>,
    ) -> Vec<Resolution> {
        let mut taken: HashSet<DeviceId> = connected.clone();
        let mut out: Vec<Option<Resolution>> = vec![None; fps.len()];

        // 1. Serial number.
        for (i, fp) in fps.iter().enumerate() {
            if fp.usable_serial().is_some()
                && let Some(k) = self.find_free(&taken, |k| {
                    k.fingerprint.same_model(fp) && serial_matches(k, fp)
                })
            {
                out[i] = Some(self.claim(k, fp, IdentityConfidence::Serial, &mut taken));
            }
        }

        // 2. OS path.
        for (i, fp) in fps.iter().enumerate() {
            if out[i].is_some() || fp.path.is_none() {
                continue;
            }
            if let Some(k) = self.find_free(&taken, |k| {
                k.fingerprint.same_model(fp)
                    && serial_matches(k, fp)
                    && k.fingerprint.path == fp.path
            }) {
                out[i] = Some(self.claim(k, fp, IdentityConfidence::Path, &mut taken));
            }
        }

        // 3. Remaining known devices of the same model. Count competitors up front: if two
        // unresolved twins share the candidates, neither assignment is certain.
        let competing: Vec<usize> = (0..fps.len())
            .map(|i| {
                (0..fps.len())
                    .filter(|&j| {
                        out[j].is_none()
                            && fps[j].same_model(&fps[i])
                            && fps[j].usable_serial() == fps[i].usable_serial()
                    })
                    .count()
            })
            .collect();
        for i in 0..fps.len() {
            if out[i].is_some() {
                continue;
            }
            let fp = &fps[i];
            let candidates = self.free_candidates(&taken, fp);
            if candidates.is_empty() {
                continue;
            }
            let competing = competing[i];
            let confidence = if candidates.len() == 1 && competing == 1 {
                IdentityConfidence::OnlyCandidate
            } else {
                IdentityConfidence::Ambiguous
            };
            out[i] = Some(self.claim(candidates[0], fp, confidence, &mut taken));
        }

        // 4. New devices.
        for (i, fp) in fps.iter().enumerate() {
            if out[i].is_none() {
                out[i] = Some(self.register_new(fp, &mut taken));
            }
        }

        out.into_iter()
            .map(|r| r.expect("every fingerprint resolved"))
            .collect()
    }

    fn find_free(
        &self,
        taken: &HashSet<DeviceId>,
        pred: impl Fn(&KnownDevice) -> bool,
    ) -> Option<usize> {
        self.known
            .iter()
            .position(|k| !taken.contains(&k.id) && pred(k))
    }

    fn free_candidates(&self, taken: &HashSet<DeviceId>, fp: &Fingerprint) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.known.len())
            .filter(|&i| {
                let k = &self.known[i];
                !taken.contains(&k.id) && k.fingerprint.same_model(fp) && serial_matches(k, fp)
            })
            .collect();
        idx.sort_by_key(|&i| self.known[i].ordinal);
        idx
    }

    fn claim(
        &mut self,
        index: usize,
        fp: &Fingerprint,
        confidence: IdentityConfidence,
        taken: &mut HashSet<DeviceId>,
    ) -> Resolution {
        let k = &mut self.known[index];
        k.fingerprint = fp.clone();
        taken.insert(k.id.clone());
        Resolution {
            id: k.id.clone(),
            confidence,
        }
    }

    fn register_new(&mut self, fp: &Fingerprint, taken: &mut HashSet<DeviceId>) -> Resolution {
        let ordinal = self
            .known
            .iter()
            .filter(|k| k.fingerprint.same_model(fp))
            .map(|k| k.ordinal)
            .max()
            .unwrap_or(0)
            + 1;
        let id = DeviceId::new(fp.vendor_id, fp.product_id, ordinal);
        self.known.push(KnownDevice {
            id: id.clone(),
            ordinal,
            fingerprint: fp.clone(),
            label: None,
        });
        taken.insert(id.clone());
        Resolution {
            id,
            confidence: IdentityConfidence::New,
        }
    }
}

fn serial_matches(k: &KnownDevice, fp: &Fingerprint) -> bool {
    k.fingerprint.usable_serial() == fp.usable_serial()
}

#[cfg(test)]
mod tests {
    use super::*;
    use IdentityConfidence::*;

    fn fp(serial: Option<&str>, path: &str) -> Fingerprint {
        Fingerprint {
            vendor_id: 0x231d,
            product_id: 0x0200,
            serial: serial.map(Into::into),
            path: Some(path.into()),
            name: "Stick".into(),
        }
    }

    fn none() -> HashSet<DeviceId> {
        HashSet::new()
    }

    fn ids(r: &[Resolution]) -> Vec<&str> {
        r.iter().map(|r| r.id.0.as_str()).collect()
    }

    #[test]
    fn identical_models_get_distinct_ids() {
        let mut reg = DeviceRegistry::default();
        let r = reg.resolve_batch(&[fp(None, "p1"), fp(None, "p2")], &none());
        assert_eq!(ids(&r), ["231d:0200#1", "231d:0200#2"]);
        assert!(r.iter().all(|r| r.confidence == New));
    }

    #[test]
    fn same_port_after_restart_keeps_id() {
        let mut reg = DeviceRegistry::default();
        reg.resolve_batch(&[fp(None, "p1"), fp(None, "p2")], &none());
        // Enumeration order differs after the restart.
        let r = reg.resolve_batch(&[fp(None, "p2"), fp(None, "p1")], &none());
        assert_eq!(ids(&r), ["231d:0200#2", "231d:0200#1"]);
        assert!(r.iter().all(|r| r.confidence == Path));
    }

    #[test]
    fn serial_survives_port_swap() {
        let mut reg = DeviceRegistry::default();
        reg.resolve_batch(&[fp(Some("A"), "p1"), fp(Some("B"), "p2")], &none());
        let r = reg.resolve_batch(&[fp(Some("A"), "p2"), fp(Some("B"), "p1")], &none());
        assert_eq!(ids(&r), ["231d:0200#1", "231d:0200#2"]);
        assert!(r.iter().all(|r| r.confidence == Serial));
    }

    #[test]
    fn single_device_moved_to_other_port_keeps_id() {
        let mut reg = DeviceRegistry::default();
        reg.resolve_batch(&[fp(None, "p1")], &none());
        let r = reg.resolve(&fp(None, "p9"), &none());
        assert_eq!(
            r,
            Resolution {
                id: DeviceId("231d:0200#1".into()),
                confidence: OnlyCandidate
            }
        );
    }

    #[test]
    fn one_twin_moved_while_other_stays() {
        let mut reg = DeviceRegistry::default();
        reg.resolve_batch(&[fp(None, "p1"), fp(None, "p2")], &none());
        let r = reg.resolve_batch(&[fp(None, "p2"), fp(None, "p9")], &none());
        assert_eq!(ids(&r), ["231d:0200#2", "231d:0200#1"]);
        assert_eq!(r[1].confidence, OnlyCandidate);
    }

    #[test]
    fn twins_both_moved_are_flagged_ambiguous() {
        let mut reg = DeviceRegistry::default();
        reg.resolve_batch(&[fp(None, "p1"), fp(None, "p2")], &none());
        let r = reg.resolve_batch(&[fp(None, "p8"), fp(None, "p9")], &none());
        assert_eq!(ids(&r), ["231d:0200#1", "231d:0200#2"]);
        assert!(r.iter().all(|r| r.confidence == Ambiguous));
    }

    #[test]
    fn hotplug_does_not_reuse_connected_id() {
        let mut reg = DeviceRegistry::default();
        let first = reg.resolve(&fp(None, "p1"), &none());
        let connected = HashSet::from([first.id.clone()]);
        let second = reg.resolve(&fp(None, "p2"), &connected);
        assert_eq!(second.id.0, "231d:0200#2");
        assert_eq!(second.confidence, New);
    }

    #[test]
    fn different_serial_is_a_different_unit() {
        let mut reg = DeviceRegistry::default();
        reg.resolve(&fp(Some("A"), "p1"), &none());
        let r = reg.resolve(&fp(Some("B"), "p1"), &none());
        assert_eq!(r.id.0, "231d:0200#2");
    }

    #[test]
    fn labels_are_trimmed_and_cleared() {
        let mut reg = DeviceRegistry::default();
        let id = reg.resolve(&fp(None, "p1"), &none()).id;
        assert!(reg.set_label(&id, Some("  Left throttle ".into())));
        assert_eq!(
            reg.get(&id).unwrap().label.as_deref(),
            Some("Left throttle")
        );
        reg.set_label(&id, Some(" ".into()));
        assert_eq!(reg.get(&id).unwrap().label, None);
    }

    #[test]
    fn round_trips_through_json() {
        let mut reg = DeviceRegistry::default();
        reg.resolve(&fp(Some("A"), "p1"), &none());
        let json = serde_json::to_string(&reg).unwrap();
        assert_eq!(serde_json::from_str::<DeviceRegistry>(&json).unwrap(), reg);
    }
}
