//! Bounded process-lifetime metadata for native custom world presets.
//! Entries contain static identifiers and function pointers, never world owners.
use horizon_world::nbt::Tag;
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

const MAX_ENTRIES: usize = 64;
const MAX_LABEL_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisteredPreset {
    id: &'static str,
    label: &'static str,
}

impl RegisteredPreset {
    pub const fn id(&self) -> &'static str {
        self.id
    }

    pub const fn label(&self) -> &'static str {
        self.label
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CustomPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub overworld: fn() -> Result<Tag, String>,
    pub recognizes: fn(&Tag) -> bool,
}

#[derive(Clone, Copy, Debug)]
pub enum SettingKind {
    Number {
        min: f64,
        max: f64,
        step: f64,
        integer: bool,
    },
    Boolean,
}

#[derive(Clone, Copy, Debug)]
pub struct PresetSetting {
    pub path: &'static str,
    pub label: &'static str,
    pub kind: SettingKind,
}

/// Optional editor callbacks; existing CustomPreset registrations stay compatible.
#[derive(Clone, Copy, Debug)]
pub struct CustomPresetEditor {
    pub fields: fn() -> Vec<PresetSetting>,
    pub defaults: fn() -> Result<Value, String>,
    pub overworld: fn(&Value) -> Result<Tag, String>,
    /// None denotes the original uncustomized stem; malformed settings return Err.
    pub settings: fn(&Tag) -> Result<Option<Value>, String>,
}

#[derive(Default)]
struct Catalog {
    entries: Vec<CustomPreset>,
    editors: Vec<(RegisteredPreset, CustomPresetEditor)>,
}

fn valid_identifier(id: &str) -> Result<(), String> {
    let mut parts = id.split(':');
    let namespace = parts.next().unwrap_or("");
    let path = parts.next();
    if parts.next().is_some() {
        return Err(format!(
            "malformed identifier {id:?}: expected exactly one ':'"
        ));
    }
    let path = match path {
        Some(p) if !p.is_empty() => p,
        _ => return Err(format!("malformed identifier {id:?}: empty path")),
    };
    if namespace.is_empty() {
        return Err(format!("malformed identifier {id:?}: empty namespace"));
    }
    let ns_ok = namespace
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-'));
    if !ns_ok {
        return Err(format!(
            "malformed identifier {id:?}: invalid namespace character"
        ));
    }
    let path_ok = path.bytes().all(|b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'/' | b'-')
    });
    if !path_ok {
        return Err(format!(
            "malformed identifier {id:?}: invalid path character"
        ));
    }
    if namespace == "minecraft" {
        return Err(format!(
            "identifier {id:?}: minecraft namespace is reserved"
        ));
    }
    Ok(())
}

impl Catalog {
    pub fn register(&mut self, preset: CustomPreset) -> Result<RegisteredPreset, String> {
        valid_identifier(preset.id)?;
        if preset.label.is_empty() {
            return Err(format!("preset {:?}: empty label", preset.id));
        }
        if preset.label.len() > MAX_LABEL_BYTES {
            return Err(format!(
                "preset {:?}: label exceeds {} bytes ({} bytes)",
                preset.id,
                MAX_LABEL_BYTES,
                preset.label.len()
            ));
        }
        if self.entries.iter().any(|e| e.id == preset.id) {
            return Err(format!("preset {:?}: already registered", preset.id));
        }
        if self.entries.len() >= MAX_ENTRIES {
            return Err(format!(
                "preset {:?}: catalog capacity {} reached",
                preset.id, MAX_ENTRIES
            ));
        }
        let registered = RegisteredPreset {
            id: preset.id,
            label: preset.label,
        };
        self.entries.push(preset);
        Ok(registered)
    }

    pub fn presets(&self) -> Vec<RegisteredPreset> {
        self.entries
            .iter()
            .map(|e| RegisteredPreset {
                id: e.id,
                label: e.label,
            })
            .collect()
    }

    pub fn by_id(&self, id: &str) -> Option<RegisteredPreset> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| RegisteredPreset {
                id: e.id,
                label: e.label,
            })
    }

    pub fn definition(&self, preset: RegisteredPreset) -> Option<CustomPreset> {
        self.entries.iter().copied().find(|e| e.id == preset.id)
    }
}

static CATALOG: OnceLock<Mutex<Catalog>> = OnceLock::new();

fn catalog() -> &'static Mutex<Catalog> {
    CATALOG.get_or_init(|| Mutex::new(Catalog::default()))
}

fn lock_catalog() -> std::sync::MutexGuard<'static, Catalog> {
    catalog()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn register(preset: CustomPreset) -> Result<RegisteredPreset, String> {
    lock_catalog().register(preset)
}

pub fn presets() -> Vec<RegisteredPreset> {
    lock_catalog().presets()
}

pub fn by_id(id: &str) -> Option<RegisteredPreset> {
    lock_catalog().by_id(id)
}

pub fn register_editor(preset: RegisteredPreset, editor: CustomPresetEditor) -> Result<(), String> {
    let fields = (editor.fields)();
    if fields.is_empty() || fields.len() > 64 {
        return Err("custom preset editor must expose 1..64 settings".into());
    }
    for (i, field) in fields.iter().enumerate() {
        if !field.path.starts_with('/')
            || field.path.len() > 256
            || field.label.is_empty()
            || field.label.len() > MAX_LABEL_BYTES
            || fields[..i].iter().any(|f| f.path == field.path)
        {
            return Err("invalid or duplicate custom preset editor field".into());
        }
        if let SettingKind::Number {
            min,
            max,
            step,
            integer,
        } = field.kind
            && (!min.is_finite()
                || !max.is_finite()
                || !step.is_finite()
                || min >= max
                || step <= 0.0
                || (integer && [min, max, step].iter().any(|v| v.fract() != 0.0)))
        {
            return Err("invalid custom preset editor numeric range".into());
        }
    }
    let mut guard = lock_catalog();
    if guard.definition(preset).is_none() || guard.editors.iter().any(|(p, _)| *p == preset) {
        return Err("custom preset editor requires a registered preset without an editor".into());
    }
    guard.editors.push((preset, editor));
    Ok(())
}

pub fn editor(preset: RegisteredPreset) -> Option<CustomPresetEditor> {
    lock_catalog()
        .editors
        .iter()
        .find(|(p, _)| *p == preset)
        .map(|(_, e)| *e)
}

fn check_settings(value: &Value) -> Result<(), String> {
    if !value.is_object() || value.to_string().len() > 60 * 1024 {
        return Err("custom preset settings must be an object of at most 60 KiB".into());
    }
    Ok(())
}

pub fn generate_custom(preset: RegisteredPreset, value: &Value) -> Result<Tag, String> {
    check_settings(value)?;
    let editor = editor(preset).ok_or("custom preset has no editor")?;
    let stem = validate_stem(preset, (editor.overworld)(value)?)?;
    if recognize(stem.get("generator").ok_or("custom generator missing")?) != Some(preset)
        || (editor.settings)(&stem)?.as_ref() != Some(value)
    {
        return Err(
            "custom preset editor did not preserve its settings and preset identity".into(),
        );
    }
    Ok(stem)
}

pub fn validate_custom_stem(preset: RegisteredPreset, stem: Tag) -> Result<Tag, String> {
    let stem = validate_stem(preset, stem)?;
    if recognize(stem.get("generator").ok_or("custom generator missing")?) != Some(preset) {
        return Err("custom world stem belongs to another preset".into());
    }
    if let Some(editor) = editor(preset)
        && let Some(value) = (editor.settings)(&stem)?
    {
        return generate_custom(preset, &value);
    }
    Ok(stem)
}

pub fn generate(preset: RegisteredPreset) -> Result<Tag, String> {
    let generator = {
        let guard = lock_catalog();
        match guard.definition(preset) {
            Some(def) => def.overworld,
            None => return Err(format!("preset {:?}: not registered in catalog", preset.id)),
        }
    };
    validate_stem(
        preset,
        generator()
            .map_err(|err| format!("preset {:?}: overworld generation failed: {err}", preset.id))?,
    )
}

fn validate_stem(preset: RegisteredPreset, stem: Tag) -> Result<Tag, String> {
    if !matches!(stem.get("type"), Some(Tag::String(_) | Tag::Compound(_)))
        || !matches!(
            stem.get("generator").and_then(|g| g.get("type")),
            Some(Tag::String(_))
        )
    {
        return Err(format!(
            "preset {:?}: expected an overworld level stem with a dimension type and generator.type",
            preset.id
        ));
    }
    Ok(stem)
}

pub fn recognize(tag: &Tag) -> Option<RegisteredPreset> {
    let snapshot: Vec<CustomPreset> = {
        let guard = lock_catalog();
        guard.entries.clone()
    };
    snapshot
        .into_iter()
        .find(|def| (def.recognizes)(tag))
        .map(|def| RegisteredPreset {
            id: def.id,
            label: def.label,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &'static str, label: &'static str) -> CustomPreset {
        CustomPreset {
            id,
            label,
            overworld: || Err("unused".into()),
            recognizes: |_| false,
        }
    }

    #[test]
    fn invalid_entries_do_not_change_the_catalog() {
        let mut catalog = Catalog::default();
        for id in [
            "missing",
            ":empty",
            "empty:",
            "a:b:c",
            "Bad:path",
            "a:Bad",
            "a:b c",
            "minecraft:normal",
        ] {
            assert!(catalog.register(entry(id, "Example")).is_err(), "{id}");
        }
        assert!(catalog.register(entry("example:empty", "")).is_err());
        let long_label: &'static str = Box::leak("x".repeat(MAX_LABEL_BYTES + 1).into_boxed_str());
        assert!(catalog.register(entry("example:long", long_label)).is_err());
        assert!(catalog.presets().is_empty());
        let first = catalog
            .register(entry("example:first/path", "First"))
            .unwrap();
        assert!(
            catalog
                .register(entry("example:first/path", "Duplicate"))
                .is_err()
        );
        assert_eq!(catalog.presets(), vec![first]);
        assert_eq!(catalog.by_id("example:first/path"), Some(first));
        assert_eq!(catalog.by_id("example:missing"), None);
    }

    #[test]
    fn catalog_capacity_is_bounded_and_ordered() {
        let mut catalog = Catalog::default();
        let mut expected = Vec::new();
        for i in 0..MAX_ENTRIES {
            let id: &'static str = Box::leak(format!("example:preset_{i}").into_boxed_str());
            expected.push(catalog.register(entry(id, "Example")).unwrap());
        }
        assert!(
            catalog
                .register(entry("example:overflow", "Overflow"))
                .is_err()
        );
        assert_eq!(catalog.presets(), expected);
    }
}
