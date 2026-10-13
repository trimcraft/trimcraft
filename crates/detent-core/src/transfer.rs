//! Export and import of the configuration (`.detent.json` files).
//!
//! The file wraps a setup (`Setup`, as stored in setup.json) with the schema version, an export
//! time and a note about what was left out. Credentials are never exported. Everything works on
//! the setup as JSON where it can, so fields added later travel without changes here.
//!
//! Importing either replaces the whole setup ([`prepare_replace`]) or adds profiles to the
//! current one ([`merge_profiles`]): ids that clash are renumbered, names that clash get a
//! suffix like "DCS (2)".

use crate::backup;
use crate::profile::{Profile, same_executable};
use crate::setup::{SETUP_SCHEMA_VERSION, Setup, SetupError, next_id};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

pub const EXPORT_FORMAT: &str = "detent-export";
pub const EXPORT_FORMAT_VERSION: u32 = 1;
/// File name extension of export files, without the leading dot of the first part.
pub const EXPORT_EXTENSION: &str = "detent.json";

#[derive(Debug, Error, PartialEq)]
pub enum TransferError {
    #[error("this is not a Detent export file: {0}")]
    NotAnExport(String),
    #[error(
        "this file was exported by a newer Detent (file format {found}, supported {EXPORT_FORMAT_VERSION}); update Detent to import it"
    )]
    NewerFormat { found: u64 },
    #[error(
        "this file was written by a newer Detent (configuration schema {found}, supported {SETUP_SCHEMA_VERSION}); update Detent to import it"
    )]
    NewerSchema { found: u64 },
    #[error("the configuration in this file is not valid: {0}")]
    Invalid(String),
    #[error("no profile selected")]
    NothingSelected,
    #[error("the file has no profile with the id {0}")]
    UnknownProfile(String),
}

impl From<SetupError> for TransferError {
    fn from(e: SetupError) -> Self {
        match e {
            SetupError::NewerSchema { found } => Self::NewerSchema { found },
            SetupError::Invalid(e) => Self::Invalid(e.to_string()),
        }
    }
}

// --- Secrets ---------------------------------------------------------------------------------

/// Words in a field name that mark a secret.
const SECRET_WORDS: [&str; 7] = [
    "secret",
    "token",
    "password",
    "passwd",
    "apikey",
    "authorization",
    "credential",
];

fn is_secret_name(name: &str) -> bool {
    let name: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    SECRET_WORDS.iter().any(|w| name.contains(w))
}

/// Empties every field that looks like a secret (name contains secret, token, password, API
/// key, authorization or credential), anywhere in `value`, and the value of header entries
/// (`{"name": "Authorization", "value": ...}`) with such a name. Strings become `""`, lists
/// `[]`, anything else `null`. Returns the paths of what was emptied, e.g.
/// `connections.discord.clientSecret`. References to the credential store (they name an
/// entry, not the secret) are secrets here too: after an import the user enters them again.
pub fn strip_secrets(value: &mut Value) -> Vec<String> {
    let mut stripped = Vec::new();
    strip(value, &mut String::new(), &mut stripped);
    stripped
}

/// A reference to a credential store entry (`{"store": ..., "target": ...}`), whatever the field
/// is called.
fn is_credential_ref(v: &Value) -> bool {
    v.as_object()
        .is_some_and(|o| o.contains_key("store") && o.get("target").is_some_and(Value::is_string))
}

fn emptied(v: &Value) -> Value {
    match v {
        Value::String(_) => json!(""),
        Value::Array(_) => json!([]),
        _ => Value::Null,
    }
}

fn strip(value: &mut Value, path: &mut String, out: &mut Vec<String>) {
    let len = path.len();
    match value {
        Value::Object(map) => {
            // A header as {name, value}: the name tells whether the value is secret.
            let header_secret = ["name", "key", "header"].iter().any(|k| {
                map.get(*k)
                    .and_then(Value::as_str)
                    .is_some_and(is_secret_name)
            });
            if header_secret && let Some(v) = map.get_mut("value") {
                if !v.is_null() && v != &json!("") {
                    out.push(format!("{path}.value"));
                }
                *v = emptied(v);
            }
            for (key, v) in map.iter_mut() {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(key);
                if is_secret_name(key) {
                    if !v.is_null() && v != &json!("") && v != &json!([]) {
                        out.push(path.clone());
                    }
                    *v = emptied(v);
                } else if is_credential_ref(v) {
                    out.push(path.clone());
                    *v = Value::Null;
                } else {
                    strip(v, path, out);
                }
                path.truncate(len);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter_mut().enumerate() {
                path.push_str(&format!("[{i}]"));
                strip(v, path, out);
                path.truncate(len);
            }
        }
        _ => {}
    }
}

// --- Export ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportKind {
    /// The whole configuration.
    All,
    /// Selected profiles (with the encoders and counters they may use).
    Profiles,
    /// A bare setup.json, e.g. a backup.
    Backup,
}

impl ExportKind {
    fn from_value(v: Option<&Value>) -> Self {
        match v.and_then(Value::as_str) {
            Some("profiles") => Self::Profiles,
            Some("all") => Self::All,
            _ => Self::Backup,
        }
    }
}

/// The export file's text. `profiles` selects profiles by id (in the order of the setup);
/// `None` exports everything. Encoders and counters are always included whole, so profiles that
/// use them can find them again.
pub fn export_json(
    setup: &Setup,
    profiles: Option<&[String]>,
    now_secs: u64,
    app_version: &str,
) -> Result<String, TransferError> {
    let mut value =
        serde_json::to_value(setup).map_err(|e| TransferError::Invalid(e.to_string()))?;
    let kind = match profiles {
        None => ExportKind::All,
        Some(ids) => {
            if ids.is_empty() {
                return Err(TransferError::NothingSelected);
            }
            if let Some(unknown) = ids
                .iter()
                .find(|id| !setup.profiles.iter().any(|p| &p.id == *id))
            {
                return Err(TransferError::UnknownProfile(unknown.clone()));
            }
            let list = value["profiles"]
                .as_array_mut()
                .expect("profiles is a list");
            list.retain(|p| {
                p["id"]
                    .as_str()
                    .is_some_and(|id| ids.iter().any(|i| i == id))
            });
            let first = list[0]["id"].clone();
            value["activeProfile"] = first;
            ExportKind::Profiles
        }
    };
    let stripped = strip_secrets(&mut value);
    let file = json!({
        "format": EXPORT_FORMAT,
        "formatVersion": EXPORT_FORMAT_VERSION,
        "schemaVersion": SETUP_SCHEMA_VERSION,
        "exportedAt": backup::iso(now_secs),
        "detentVersion": app_version,
        "kind": kind,
        "credentialsRemoved": !stripped.is_empty(),
        "strippedFields": stripped,
        "note": "Credentials are never exported. Connections that need them (Discord, ...) must have their credentials entered again after importing.",
        "setup": value,
    });
    Ok(serde_json::to_string_pretty(&file).expect("export serialises"))
}

// --- Import ----------------------------------------------------------------------------------

/// A parsed, validated export file (or setup.json).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportFile {
    /// Migrated to the current schema.
    pub setup: Setup,
    pub kind: ExportKind,
    pub exported_at: Option<String>,
    pub detent_version: Option<String>,
    /// Schema the file was written with.
    pub schema_version: u64,
    /// Fields left out when exporting.
    pub stripped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub controls: usize,
    pub executables: Vec<String>,
}

/// What the import dialog shows about a file.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub kind: ExportKind,
    pub exported_at: Option<String>,
    pub detent_version: Option<String>,
    pub schema_version: u64,
    pub profiles: Vec<ProfileSummary>,
    /// Connections whose credentials have to be entered again.
    pub stripped_fields: Vec<String>,
}

impl ImportFile {
    pub fn summary(&self) -> ImportSummary {
        ImportSummary {
            kind: self.kind,
            exported_at: self.exported_at.clone(),
            detent_version: self.detent_version.clone(),
            schema_version: self.schema_version,
            profiles: self
                .setup
                .profiles
                .iter()
                .map(|p| ProfileSummary {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    controls: p.virtual_controls.len(),
                    executables: p.executables.clone(),
                })
                .collect(),
            stripped_fields: self.stripped.clone(),
        }
    }
}

/// Reads an export file, or a bare setup.json (a backup). Older schemas are migrated; files of
/// a newer file format or schema are refused.
pub fn parse_import(text: &str) -> Result<ImportFile, TransferError> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| TransferError::NotAnExport(e.to_string()))?;
    let Some(obj) = value.as_object() else {
        return Err(TransferError::NotAnExport("not a JSON object".into()));
    };
    let (inner, meta): (Value, Option<&Map<String, Value>>) = match obj.get("format") {
        Some(format) => {
            if format != EXPORT_FORMAT {
                return Err(TransferError::NotAnExport(format!("format is {format}")));
            }
            let found = obj
                .get("formatVersion")
                .and_then(Value::as_u64)
                .unwrap_or(1);
            if found > u64::from(EXPORT_FORMAT_VERSION) {
                return Err(TransferError::NewerFormat { found });
            }
            let Some(setup) = obj.get("setup").filter(|s| s.is_object()) else {
                return Err(TransferError::NotAnExport("it has no configuration".into()));
            };
            (setup.clone(), Some(obj))
        }
        None if obj.contains_key("schemaVersion") || obj.contains_key("profiles") => {
            (value.clone(), None)
        }
        None => {
            return Err(TransferError::NotAnExport(
                "neither an export nor a setup file".into(),
            ));
        }
    };
    let schema_version = inner
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let setup = Setup::from_json(&inner.to_string())?;
    setup
        .connections
        .validate()
        .map_err(|e| TransferError::Invalid(e.to_string()))?;
    let text_of = |key: &str| {
        meta.and_then(|m| m.get(key))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let stripped = meta
        .and_then(|m| m.get("strippedFields"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(ImportFile {
        setup,
        kind: ExportKind::from_value(meta.and_then(|m| m.get("kind"))),
        exported_at: text_of("exportedAt"),
        detent_version: text_of("detentVersion"),
        schema_version,
        stripped,
    })
}

/// The setup that results from "Replace all": the imported one, but the settings of this
/// computer (tray, autostart) stay, and so do the credentials of a connection that the file
/// has none for and that is the same account (Discord: same Client ID), since the credential
/// store entries are still there.
pub fn prepare_replace(current: &Setup, mut imported: Setup) -> Setup {
    imported.settings = current.settings.clone();
    let (cur, new) = (
        &current.connections.discord,
        &mut imported.connections.discord,
    );
    if new.client_secret.is_none() && !new.client_id.is_empty() && new.client_id == cur.client_id {
        new.client_secret = cur.client_secret.clone();
    }
    imported
}

/// What [`merge_profiles`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeReport {
    pub added: Vec<AddedProfile>,
    /// Programs the imported profiles listed that another profile already lists; they were not
    /// taken over, so automatic switching stays unambiguous.
    pub dropped_executables: Vec<String>,
    pub added_encoders: usize,
    pub added_functions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedProfile {
    pub id: String,
    pub name: String,
    /// The name in the file, when it had to be changed.
    pub renamed_from: Option<String>,
}

/// "DCS" taken: "DCS (2)", then "DCS (3)".
pub fn unique_name(name: &str, taken: &[String]) -> String {
    let used = |n: &str| {
        taken
            .iter()
            .any(|t| t.trim().eq_ignore_ascii_case(n.trim()))
    };
    if !used(name) {
        return name.to_owned();
    }
    (2..)
        .map(|i| format!("{name} ({i})"))
        .find(|n| !used(n))
        .expect("an unused number exists")
}

fn strings_in(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::String(s) => {
            out.insert(s.clone());
        }
        Value::Array(a) => a.iter().for_each(|v| strings_in(v, out)),
        Value::Object(o) => o.values().for_each(|v| strings_in(v, out)),
        _ => {}
    }
}

/// Replaces every string that equals a key of `map` (ids are the only strings that do).
fn rename_ids(value: &mut Value, map: &HashMap<String, String>) {
    match value {
        Value::String(s) => {
            if let Some(new) = map.get(s.as_str()) {
                s.clone_from(new);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|v| rename_ids(v, map)),
        Value::Object(o) => o.values_mut().for_each(|v| rename_ids(v, map)),
        _ => {}
    }
}

/// Adds the profiles `selected` (ids in `source`) to `target`. Nothing changes on error.
///
/// Encoders and counters (functions) that the profiles use come along: with their id when it
/// is free, shared when the target has the very same definition under that id, otherwise as a
/// copy with a new id. The profiles get new ids (and their controls, when an id is taken) and
/// unique names; references between them are updated. They do not become active.
pub fn merge_profiles(
    target: &mut Setup,
    source: &Setup,
    selected: &[String],
) -> Result<MergeReport, TransferError> {
    if selected.is_empty() {
        return Err(TransferError::NothingSelected);
    }
    let mut chosen: Vec<&Profile> = Vec::new();
    for id in selected {
        let p = source
            .profiles
            .iter()
            .find(|p| &p.id == id)
            .ok_or_else(|| TransferError::UnknownProfile(id.clone()))?;
        if !chosen.iter().any(|c| c.id == p.id) {
            chosen.push(p);
        }
    }
    let mut work = target.clone();
    let mut report = MergeReport::default();
    let mut rename: HashMap<String, String> = HashMap::new();
    let mut profile_values: Vec<Value> = chosen.iter().map(|p| to_value(*p)).collect();

    // Ids the profiles mention, to find the encoders and counters they use.
    let mut mentioned = HashSet::new();
    profile_values
        .iter()
        .for_each(|v| strings_in(v, &mut mentioned));
    let functions: Vec<&crate::function::Function> = source
        .functions
        .iter()
        .filter(|f| mentioned.contains(&f.id))
        .collect();
    let mut function_values: Vec<Value> = functions.iter().map(|f| to_value(*f)).collect();
    function_values
        .iter()
        .for_each(|v| strings_in(v, &mut mentioned));

    for encoder in source.encoders.iter().filter(|e| mentioned.contains(&e.id)) {
        match work.encoders.iter().find(|e| e.id == encoder.id) {
            Some(existing) if existing == encoder => {}
            taken => {
                let mut copy = encoder.clone();
                if taken.is_some() {
                    copy.id = next_id("enc", work.encoders.iter().map(|e| e.id.as_str()));
                    rename.insert(encoder.id.clone(), copy.id.clone());
                }
                work.encoders.push(copy);
                report.added_encoders += 1;
            }
        }
    }
    for value in &mut function_values {
        rename_ids(value, &rename);
    }
    for value in &function_values {
        let mut function: crate::function::Function = serde_json::from_value(value.clone())
            .map_err(|e| TransferError::Invalid(e.to_string()))?;
        let old_id = function.id.clone();
        match work.functions.iter().find(|f| f.id == old_id) {
            Some(existing) if to_value(existing) == *value => continue,
            Some(_) => {
                function.id = next_id("fn", work.functions.iter().map(|f| f.id.as_str()));
                rename.insert(old_id, function.id.clone());
            }
            None => {}
        }
        work.functions.push(function);
        report.added_functions += 1;
    }

    let mut profile_ids: Vec<String> = work.profiles.iter().map(|p| p.id.clone()).collect();
    let mut control_ids: Vec<String> = work
        .profiles
        .iter()
        .flat_map(|p| p.virtual_controls.iter().map(|c| c.id.clone()))
        .collect();
    let mut names: Vec<String> = work.profiles.iter().map(|p| p.name.clone()).collect();
    for profile in &chosen {
        let new_id = next_id("profile", profile_ids.iter().map(String::as_str));
        profile_ids.push(new_id.clone());
        rename.insert(profile.id.clone(), new_id);
        for control in &profile.virtual_controls {
            if control_ids.contains(&control.id) {
                let new = next_id("vc", control_ids.iter().map(String::as_str));
                control_ids.push(new.clone());
                rename.insert(control.id.clone(), new);
            } else {
                control_ids.push(control.id.clone());
            }
        }
    }
    for (profile, value) in chosen.iter().zip(&mut profile_values) {
        rename_ids(value, &rename);
        let mut imported: Profile = serde_json::from_value(value.clone())
            .map_err(|e| TransferError::Invalid(e.to_string()))?;
        let name = if imported.name.trim().is_empty() {
            "Imported profile".to_owned()
        } else {
            imported.name.trim().to_owned()
        };
        imported.name = unique_name(&name, &names);
        names.push(imported.name.clone());
        let listed: Vec<String> = work
            .profiles
            .iter()
            .flat_map(|p| p.executables.clone())
            .collect();
        let (keep, drop): (Vec<String>, Vec<String>) = imported
            .executables
            .iter()
            .cloned()
            .partition(|e| !listed.iter().any(|l| same_executable(l, e)));
        imported.executables = keep;
        report.dropped_executables.extend(drop);
        report.added.push(AddedProfile {
            id: imported.id.clone(),
            renamed_from: (imported.name != profile.name).then(|| profile.name.clone()),
            name: imported.name.clone(),
        });
        work.profiles.push(imported);
    }
    // Macro ids are unique per profile, so an added profile keeps its own (and its "Stop
    // macros" targets stay right); this only fills in ids a hand-edited file lacks.
    work.assign_macro_ids();
    *target = work;
    Ok(report)
}

fn to_value<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).expect("serialises")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::SecretRef;
    use crate::device::DeviceId;
    use crate::encoder::EncoderBinding;
    use crate::function::{CounterInput, CounterSpec, EndBehavior, Function, FunctionKind};
    use crate::trigger::{StepDirection, Trigger, TriggerInput};
    use crate::virtual_control::{Radio, RadioState, VirtualControl, VirtualControlKind};

    const NOW: u64 = 1_791_892_800;

    fn control(id: &str, name: &str, encoder: &str) -> VirtualControl {
        let radio = Radio {
            states: vec![RadioState {
                id: "st-1".into(),
                name: "A".into(),
                select: vec![Trigger::from(TriggerInput::EncoderStep {
                    encoder: encoder.into(),
                    direction: StepDirection::Increment,
                })],
                ..Default::default()
            }],
            ..Default::default()
        };
        VirtualControl {
            show_toast: false,
            id: id.into(),
            name: name.into(),
            kind: VirtualControlKind::Radio(radio),
        }
    }

    fn encoder(id: &str, name: &str) -> EncoderBinding {
        EncoderBinding {
            id: id.into(),
            name: name.into(),
            device: DeviceId("3344:0001#1".into()),
            decrement: 1,
            increment: 2,
        }
    }

    fn function(id: &str, encoder: &str) -> Function {
        Function {
            id: id.into(),
            name: "Channel".into(),
            kind: FunctionKind::Counter {
                spec: CounterSpec {
                    min: 1.0,
                    max: 3.0,
                    step: 1.0,
                    initial: 1.0,
                    end: EndBehavior::Wrap,
                    decimals: 0,
                },
                input: CounterInput::Encoder {
                    encoder: encoder.into(),
                },
            },
        }
    }

    /// Profiles "Default" (vc-1) and "DCS" (vc-2, uses enc-1 and fn-1); executable DCS.exe.
    fn setup() -> Setup {
        let mut s = Setup::default();
        s.encoders.push(encoder("enc-1", "Knob"));
        s.functions.push(function("fn-1", "enc-1"));
        s.profiles[0]
            .virtual_controls
            .push(control("vc-1", "Comms", "enc-1"));
        let mut dcs = Profile::new("profile-2", "DCS");
        dcs.executables = vec!["DCS.exe".into()];
        dcs.virtual_controls.push(control("vc-2", "Gear", "enc-1"));
        s.profiles.push(dcs);
        s
    }

    #[test]
    fn secrets_are_emptied_wherever_they_are() {
        let mut v = json!({
            "connections": {"discord": {"clientId": "123", "clientSecret": {"store": "x", "target": "t"}}},
            "webhook": {
                "url": "https://example.test",
                "headers": [
                    {"name": "Authorization", "value": "Bearer abc"},
                    {"name": "Content-Type", "value": "json"}
                ],
                "apiKey": "k",
                "token": null
            },
            "list": [{"password": "p", "keep": 1}]
        });
        let mut paths = strip_secrets(&mut v);
        paths.sort();
        assert_eq!(v["connections"]["discord"]["clientSecret"], Value::Null);
        assert_eq!(v["connections"]["discord"]["clientId"], "123");
        assert_eq!(v["webhook"]["headers"][0]["value"], "");
        assert_eq!(v["webhook"]["headers"][1]["value"], "json");
        assert_eq!(v["webhook"]["apiKey"], "");
        assert_eq!(v["list"][0]["password"], "");
        assert_eq!(v["list"][0]["keep"], 1);
        assert_eq!(
            paths,
            [
                "connections.discord.clientSecret",
                "list[0].password",
                "webhook.apiKey",
                "webhook.headers[0].value",
            ]
        );
    }

    #[test]
    fn export_never_contains_the_credential_reference() {
        let mut s = setup();
        s.connections.discord.client_id = "123456789012345678".into();
        s.connections.discord.client_secret = Some(SecretRef::WindowsCredentialManager {
            target: "detent/discord/client-secret".into(),
        });
        let text = export_json(&s, None, NOW, "1.2.3").unwrap();
        assert!(!text.contains("detent/discord"), "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["format"], EXPORT_FORMAT);
        assert_eq!(v["schemaVersion"], SETUP_SCHEMA_VERSION);
        assert_eq!(v["exportedAt"], "2026-10-13T12:00:00Z");
        assert_eq!(v["detentVersion"], "1.2.3");
        assert_eq!(v["kind"], "all");
        assert_eq!(v["credentialsRemoved"], true);
        assert_eq!(v["strippedFields"][0], "connections.discord.clientSecret");
        // It reads back, without the secret and with everything else.
        let file = parse_import(&text).unwrap();
        assert_eq!(file.setup.connections.discord.client_secret, None);
        assert_eq!(
            file.setup.connections.discord.client_id,
            "123456789012345678"
        );
        assert_eq!(file.setup.profiles.len(), 2);
        assert_eq!(file.kind, ExportKind::All);
        assert_eq!(file.stripped, ["connections.discord.clientSecret"]);
    }

    #[test]
    fn selected_profiles_are_exported_alone() {
        let s = setup();
        let text = export_json(&s, Some(&["profile-2".into()]), NOW, "1").unwrap();
        let file = parse_import(&text).unwrap();
        assert_eq!(file.kind, ExportKind::Profiles);
        assert_eq!(file.setup.profiles.len(), 1);
        assert_eq!(file.setup.profiles[0].name, "DCS");
        assert_eq!(file.setup.active_profile, "profile-2");
        assert_eq!(file.setup.encoders.len(), 1);
        assert_eq!(
            export_json(&s, Some(&[]), NOW, "1"),
            Err(TransferError::NothingSelected)
        );
        assert_eq!(
            export_json(&s, Some(&["nope".into()]), NOW, "1"),
            Err(TransferError::UnknownProfile("nope".into()))
        );
    }

    #[test]
    fn version_checks() {
        let ok = export_json(&setup(), None, NOW, "1").unwrap();
        // A newer file format.
        let mut v: Value = serde_json::from_str(&ok).unwrap();
        v["formatVersion"] = 99.into();
        let err = parse_import(&v.to_string()).unwrap_err();
        assert_eq!(err, TransferError::NewerFormat { found: 99 });
        assert!(err.to_string().contains("newer Detent"));
        // A newer setup schema inside.
        let mut v: Value = serde_json::from_str(&ok).unwrap();
        v["setup"]["schemaVersion"] = 99.into();
        let err = parse_import(&v.to_string()).unwrap_err();
        assert_eq!(err, TransferError::NewerSchema { found: 99 });
        assert!(err.to_string().contains("update Detent"));
        // Somebody else's JSON, and not JSON at all.
        for text in [
            r#"{"format":"other"}"#,
            r#"{"hello":1}"#,
            "[]",
            "nonsense",
            r#"{"format":"detent-export"}"#,
        ] {
            assert!(
                matches!(parse_import(text), Err(TransferError::NotAnExport(_))),
                "{text}"
            );
        }
        // A broken configuration inside.
        let mut v: Value = serde_json::from_str(&ok).unwrap();
        v["setup"]["profiles"] = json!("x");
        assert!(matches!(
            parse_import(&v.to_string()),
            Err(TransferError::Invalid(_))
        ));
    }

    #[test]
    fn old_schema_is_migrated_and_bare_setup_files_are_accepted() {
        let old = r#"{"schemaVersion":1,"virtualControls":[{"id":"vc-1","name":"Comms","kind":{"type":"radio"}}],"overlay":{"widgets":[]}}"#;
        let file = parse_import(old).unwrap();
        assert_eq!(file.schema_version, 1);
        assert_eq!(file.kind, ExportKind::Backup);
        assert_eq!(file.setup.profiles.len(), 1);
        assert_eq!(file.setup.profiles[0].virtual_controls[0].name, "Comms");
        // Export of an old-schema export file.
        let wrapped = format!(
            r#"{{"format":"detent-export","formatVersion":1,"exportedAt":"2025-01-01T00:00:00Z","setup":{old}}}"#
        );
        let file = parse_import(&wrapped).unwrap();
        assert_eq!(file.exported_at.as_deref(), Some("2025-01-01T00:00:00Z"));
        assert_eq!(file.setup.profiles[0].name, "Default");
        let summary = file.summary();
        assert_eq!(summary.profiles[0].controls, 1);
    }

    #[test]
    fn replace_keeps_local_settings_and_matching_credentials() {
        let mut current = setup();
        current.settings.close_to_tray = false;
        current.connections.discord.client_id = "111".into();
        let secret = SecretRef::WindowsCredentialManager { target: "t".into() };
        current.connections.discord.client_secret = Some(secret.clone());

        let mut imported = Setup::default();
        imported.settings.close_to_tray = true;
        imported.connections.discord.client_id = "111".into();
        let result = prepare_replace(&current, imported.clone());
        assert!(!result.settings.close_to_tray);
        assert_eq!(result.connections.discord.client_secret, Some(secret));

        // Another Discord application: its secret does not belong to it.
        imported.connections.discord.client_id = "222".into();
        let result = prepare_replace(&current, imported);
        assert_eq!(result.connections.discord.client_secret, None);
    }

    #[test]
    fn names_get_numbered_suffixes() {
        let taken = vec!["DCS".to_owned(), "dcs (2)".to_owned()];
        assert_eq!(unique_name("DCS", &taken), "DCS (3)");
        assert_eq!(unique_name("Elite", &taken), "Elite");
    }

    #[test]
    fn merge_renames_and_remaps_everything_that_clashes() {
        let mut target = setup();
        let source = setup();
        let report = merge_profiles(&mut target, &source, &["profile-2".into()]).unwrap();
        assert_eq!(target.profiles.len(), 3);
        let added = &target.profiles[2];
        assert_eq!(added.name, "DCS (2)");
        assert_eq!(report.added[0].renamed_from.as_deref(), Some("DCS"));
        assert_eq!(added.id, "profile-3");
        // The control id was taken, so it has a new one.
        assert_eq!(added.virtual_controls[0].id, "vc-3");
        // The executable belongs to the existing DCS profile.
        assert!(added.executables.is_empty());
        assert_eq!(report.dropped_executables, ["DCS.exe"]);
        // The encoder and counter are the same definitions: shared, nothing added.
        assert_eq!((report.added_encoders, report.added_functions), (0, 0));
        assert_eq!(target.encoders.len(), 1);
        // The original profiles are untouched; the active profile does not change.
        assert_eq!(target.profiles[1].virtual_controls[0].id, "vc-2");
        assert_eq!(target.active_profile, source.active_profile);
    }

    #[test]
    fn merge_brings_missing_encoders_along_and_copies_differing_ones() {
        let source = setup();
        // An empty target gets the encoder and the counter with their ids.
        let mut target = Setup::default();
        let report = merge_profiles(&mut target, &source, &["profile-2".into()]).unwrap();
        assert_eq!((report.added_encoders, report.added_functions), (1, 0));
        // The counter is not mentioned by the profile, so it is not needed.
        assert!(target.functions.is_empty());
        assert_eq!(target.profiles[1].name, "DCS");
        assert_eq!(target.profiles[1].executables, ["DCS.exe"]);

        // Same ids, different definitions: copies with new ids, references follow.
        let mut target = Setup::default();
        target.encoders.push(encoder("enc-1", "Other knob"));
        let report = merge_profiles(&mut target, &source, &["profile-2".into()]).unwrap();
        assert_eq!(report.added_encoders, 1);
        assert_eq!(target.encoders.len(), 2);
        assert_eq!(target.encoders[1].id, "enc-2");
        assert_eq!(target.encoders[1].name, "Knob");
        let TriggerInput::EncoderStep { encoder, .. } = &target.profiles[1].virtual_controls[0]
            .radio()
            .unwrap()
            .states[0]
            .select[0]
            .input
        else {
            panic!("expected an encoder trigger");
        };
        assert_eq!(encoder, "enc-2");
    }

    #[test]
    fn merge_handles_counters_referenced_by_widgets_and_several_profiles() {
        let mut source = setup();
        // The Default profile shows the counter on its overlay.
        let widget = serde_json::json!([{"type": "function", "function": "fn-1"}]);
        source.profiles[0].overlay_widgets = serde_json::from_value(widget).unwrap();
        let mut target = Setup::default();
        target.functions.push(function("fn-1", "enc-9"));
        target.encoders.push(encoder("enc-9", "Nine"));
        let selected = ["profile-1".to_owned(), "profile-2".to_owned()];
        let report = merge_profiles(&mut target, &source, &selected).unwrap();
        assert_eq!(report.added.len(), 2);
        assert_eq!(report.added[0].name, "Default (2)");
        assert_eq!(report.added[1].name, "DCS");
        // fn-1 exists with another definition: a copy, and the widget points to it.
        assert_eq!(target.functions.len(), 2);
        let copy = &target.functions[1].id;
        assert_eq!(copy, "fn-2");
        let widgets = serde_json::to_string(&target.profiles[1].overlay_widgets).unwrap();
        assert!(widgets.contains("fn-2"), "{widgets}");
        // Both new profiles' controls are unique across the setup.
        let ids: Vec<_> = target
            .profiles
            .iter()
            .flat_map(|p| p.virtual_controls.iter().map(|c| c.id.clone()))
            .collect();
        assert_eq!(ids.len(), ids.iter().collect::<HashSet<_>>().len());
    }

    #[test]
    fn switch_profile_actions_follow_the_new_profile_ids() {
        use crate::action::Action;
        let mut source = setup();
        let switch = Action::SwitchProfile {
            target: crate::profile::ProfileTarget::Profile {
                profile: "profile-2".into(),
            },
        };
        let radio = source.profiles[0].virtual_controls[0].radio_mut().unwrap();
        radio.states[0].on_activate.push(switch);
        let mut target = Setup::default();
        let selected = ["profile-1".to_owned(), "profile-2".to_owned()];
        merge_profiles(&mut target, &source, &selected).unwrap();
        // The imported copies are profile-2 and profile-3 in the target.
        let json = serde_json::to_string(&target.profiles[1].virtual_controls[0]).unwrap();
        assert!(json.contains(r#""profile":"profile-3""#), "{json}");
    }

    fn profile_from(json: Value) -> Profile {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn http_secret_headers_are_stripped_from_an_exported_profile() {
        let mut s = setup();
        s.profiles[1] = profile_from(json!({
            "id": "profile-2", "name": "DCS", "executables": ["DCS.exe"],
            "virtualControls": [{"id": "vc-2", "name": "Alarm", "kind": {"type": "toggle",
                "onOn": [{"type": "http", "method": "POST", "url": "https://hooks.test/x", "headers": [
                    {"name": "Authorization", "value": "", "secret": {"store": "windowsCredentialManager", "target": "detent/http/1"}},
                    {"name": "X-Custom", "value": "", "secret": {"store": "windowsCredentialManager", "target": "detent/http/2"}},
                    {"name": "Content-Type", "value": "application/json"}]}]}}]
        }));
        let text = export_json(&s, Some(&["profile-2".into()]), NOW, "1").unwrap();
        assert!(!text.contains("detent/http"), "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["credentialsRemoved"], true);
        let stripped: Vec<&str> = v["strippedFields"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(stripped.len(), 2, "{stripped:?}");
        assert!(
            stripped.iter().all(|p| p.ends_with(".secret")),
            "{stripped:?}"
        );
        // The import knows credentials are missing, and the headers survive without them.
        let file = parse_import(&text).unwrap();
        assert_eq!(file.summary().stripped_fields.len(), 2);
        let control = &file.setup.profiles[0].virtual_controls[0];
        let actions = control.actions();
        let crate::action::Action::Http(request) = actions[0] else {
            panic!("expected an HTTP action");
        };
        assert_eq!(request.headers.len(), 3);
        assert!(request.headers.iter().all(|h| h.secret.is_none()));
        assert_eq!(request.headers[2].value, "application/json");
        assert_eq!(request.headers[0].name, "Authorization");
    }

    #[test]
    fn add_profiles_remaps_mode_conditions_and_every_control_type() {
        // A mode (vc-1) and controls of other types in one profile; a trigger of the radio
        // listens only while the mode is in state st-2. The target already uses vc-1..vc-2.
        let trigger = json!({"type": "button", "device": "3344:0001#1", "index": 3,
            "conditions": [{"mode": "vc-1", "state": "st-2"}]});
        let imported = profile_from(json!({
            "id": "profile-1", "name": "Flight",
            "virtualControls": [
                {"id": "vc-1", "name": "Mode", "kind": {"type": "mode", "states": [
                    {"id": "st-1", "name": "A"}, {"id": "st-2", "name": "B"}]}},
                {"id": "vc-2", "name": "Comms", "kind": {"type": "radio", "states": [
                    {"id": "st-1", "name": "X", "select": [trigger]}]}},
                {"id": "vc-3", "name": "Gear", "kind": {"type": "toggle", "toggle": [trigger]}},
                {"id": "vc-4", "name": "Boost", "kind": {"type": "momentary", "triggers": [trigger]}}
            ],
            "overlayWidgets": [{"type": "radio", "control": "vc-2"}]
        }));
        let mut source = Setup::default();
        source.profiles[0] = imported;
        let mut target = Setup::default();
        target.profiles[0]
            .virtual_controls
            .push(control("vc-1", "A", "enc-1"));
        target.profiles[0]
            .virtual_controls
            .push(control("vc-2", "B", "enc-1"));
        merge_profiles(&mut target, &source, &["profile-1".into()]).unwrap();
        let added = &target.profiles[1];
        let ids: Vec<&str> = added
            .virtual_controls
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(ids, ["vc-3", "vc-4", "vc-5", "vc-6"]);
        // Every trigger that names the mode now names its new id; the state id stays.
        let json = serde_json::to_string(added).unwrap();
        assert!(!json.contains(r#""mode":"vc-1""#), "{json}");
        assert_eq!(
            json.matches(r#"{"mode":"vc-3","state":"st-2"}"#).count(),
            3,
            "{json}"
        );
        // The overlay widget follows the radio.
        assert!(
            serde_json::to_string(&added.overlay_widgets)
                .unwrap()
                .contains(r#""control":"vc-4""#)
        );
        // The target's own controls are untouched.
        assert_eq!(target.profiles[0].virtual_controls[0].name, "A");
    }
    #[test]
    fn a_failed_merge_changes_nothing() {
        let mut target = setup();
        let before = target.clone();
        let source = setup();
        assert_eq!(
            merge_profiles(&mut target, &source, &["profile-2".into(), "ghost".into()]),
            Err(TransferError::UnknownProfile("ghost".into()))
        );
        assert_eq!(
            merge_profiles(&mut target, &source, &[]),
            Err(TransferError::NothingSelected)
        );
        assert_eq!(target, before);
    }

    #[test]
    fn imported_profile_keeps_its_macros_and_stop_macros_targets() {
        use crate::action::{Action, KeyStep, MacroTarget};
        use crate::macros::{InputMacro, Repeat};
        use crate::virtual_control::Toggle;
        // Source: profile "Turret" with a toggle (id taken in the target) whose "on" macro
        // repeats while active and whose "off" action stops it.
        let mut source = setup();
        let mut turret = Profile::new("profile-3", "Turret");
        turret.virtual_controls.push(VirtualControl {
            show_toast: false,
            id: "vc-1".into(),
            name: "Fire".into(),
            kind: VirtualControlKind::Toggle(Toggle {
                on_on: vec![Action::InputMacro(InputMacro {
                    repeat: Repeat::WhileActive,
                    ..InputMacro::new(vec![
                        KeyStep::KeyTap {
                            key: "Space".into(),
                            hold_ms: None,
                        }
                        .into(),
                    ])
                })],
                on_off: vec![Action::StopMacros {
                    target: MacroTarget::Macro { id: "mac-1".into() },
                }],
                ..Default::default()
            }),
        });
        source.profiles.push(turret);
        source.assign_macro_ids();
        let text = export_json(&source, Some(&["profile-3".into()]), NOW, "0.1.0").unwrap();
        let file = parse_import(&text).unwrap();
        assert_eq!(file.schema_version, u64::from(SETUP_SCHEMA_VERSION));

        let mut target = setup();
        merge_profiles(&mut target, &file.setup, &["profile-3".into()]).unwrap();
        let added = target.profiles.last().unwrap();
        // The control id clashed and was renamed; the macro id and its target still match.
        assert_ne!(added.virtual_controls[0].id, "vc-1");
        let actions = added.virtual_controls[0].actions();
        let id = &actions[0].input_macro().unwrap().id;
        assert_eq!(id, "mac-1");
        assert_eq!(
            actions[1],
            &Action::StopMacros {
                target: MacroTarget::Macro { id: id.clone() }
            }
        );
    }

    #[test]
    fn old_exports_with_keyboard_and_mouse_macros_import_as_input_macros() {
        let text = r#"{"format":"detent-export","formatVersion":1,"schemaVersion":3,"kind":"profiles",
            "setup":{"schemaVersion":3,"activeProfile":"profile-2","profiles":[{"id":"profile-2","name":"Old",
              "executables":[],"virtualControls":[{"id":"vc-9","name":"Keys","kind":{"type":"radio","states":[
                {"id":"st-1","name":"A","select":[],
                 "onActivate":[{"type":"keyboardMacro","steps":[{"type":"keyTap","key":"F1","holdMs":null}]}],
                 "onDeactivate":[{"type":"mouseMacro","movement":"relative","steps":[{"type":"wheel","delta":2}]}]}]}}]}]}}"#;
        let text = text.replace("detent-export", EXPORT_FORMAT);
        let file = parse_import(&text).unwrap();
        assert_eq!(file.schema_version, 3);
        let mut target = setup();
        merge_profiles(&mut target, &file.setup, &["profile-2".into()]).unwrap();
        let added = target.profiles.last().unwrap();
        let actions = added.virtual_controls[0].actions();
        let keys = actions[0].input_macro().unwrap();
        let mouse = actions[1].input_macro().unwrap();
        assert_eq!((keys.id.as_str(), mouse.id.as_str()), ("mac-1", "mac-2"));
        assert_eq!(mouse.movement, crate::action::Movement::Relative);
    }
}
