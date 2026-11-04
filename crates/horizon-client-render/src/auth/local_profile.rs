//! Explicit, executable-local account binding for a personal Horizon installation.
//! The configuration remembers a Prism profile reference, never credentials. Prism's
//! account store is read-only, and its current Minecraft token stays in memory.
use super::{AuthSession, parse_uuid};
use serde_json::Value;
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

const CONFIG_FILE: &str = "horizon-local.json";
const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const CONFIG_LIMIT: u64 = 64 * 1024;
const ACCOUNTS_LIMIT: u64 = 4 * 1024 * 1024;

pub struct LocalProfile {
    pub resources: Option<PathBuf>,
    pub game_dir: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub older_servers: bool,
    accounts_file: PathBuf,
    bound_uuid: u128,
}

static CONFIGURED: OnceLock<Result<Option<LocalProfile>, String>> = OnceLock::new();

/// Explicit sessions, offline identities and automated runs must not acquire a personal account.
pub fn implicit_session_allowed(args: &[String]) -> bool {
    !args.iter().any(|argument| {
        matches!(
            argument.split('=').next().unwrap_or_default(),
            "--name"
                | "--dedicated"
                | "--session-stdin"
                | "--session-file"
                | "--help"
                | "-h"
                | "--launch-api"
                | "--offscreen"
                | "--headless"
                | "--benchmark"
                | "--frames"
                | "--world-frames"
        )
    })
}

/// Only this executable's adjacent opt-in configuration is considered.
pub fn configured() -> Result<Option<&'static LocalProfile>, String> {
    match CONFIGURED.get_or_init(|| {
        let executable = std::env::current_exe()
            .map_err(|_| "Cannot find the personal Horizon executable.".to_owned())?;
        load_for_executable(&executable)
    }) {
        Ok(profile) => Ok(profile.as_ref()),
        Err(error) => Err(error.clone()),
    }
}

/// Absence is the ordinary Horizon behavior, and never reads any account store.
pub fn load_for_executable(executable: &Path) -> Result<Option<LocalProfile>, String> {
    let directory = executable
        .parent()
        .ok_or("Cannot find the personal Horizon configuration directory.")?;
    let path = directory.join(CONFIG_FILE);
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Cannot read horizon-local.json.".into()),
    };
    if !metadata.is_file() || metadata.len() > CONFIG_LIMIT {
        return Err("Invalid horizon-local.json.".into());
    }
    let bytes = fs::read(path).map_err(|_| "Cannot read horizon-local.json.")?;
    parse_config(&bytes).map(Some)
}

fn parse_config(bytes: &[u8]) -> Result<LocalProfile, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "Invalid horizon-local.json.")?;
    let path = |key: &str, required: bool| -> Result<Option<PathBuf>, String> {
        match value.get(key) {
            None | Some(Value::Null) if !required => Ok(None),
            Some(Value::String(text)) if !text.is_empty() && Path::new(text).is_absolute() => {
                Ok(Some(PathBuf::from(text)))
            }
            _ => Err("Personal Horizon paths must be absolute paths.".into()),
        }
    };
    let accounts_file = path("prism_accounts_file", true)?
        .ok_or("The personal Horizon account binding is incomplete.")?;
    let bound_uuid = value
        .get("minecraft_uuid")
        .and_then(Value::as_str)
        .and_then(parse_uuid)
        .ok_or("The personal Horizon account binding has no valid Minecraft profile.")?;
    let older_servers = match value.get("older_servers") {
        None => false,
        Some(Value::Bool(enabled)) => *enabled,
        _ => return Err("Invalid horizon-local.json.".into()),
    };
    Ok(LocalProfile {
        resources: path("resources", false)?,
        game_dir: path("game_dir", false)?,
        home: path("home", false)?,
        older_servers,
        accounts_file,
        bound_uuid,
    })
}

impl LocalProfile {
    /// Loads only the remembered identity, even if Prism's active selection changed.
    pub fn session(&self) -> Result<AuthSession, String> {
        let metadata = fs::metadata(&self.accounts_file)
            .map_err(|_| "Cannot read the remembered Prism account. Open Prism and try again.")?;
        if !metadata.is_file() || metadata.len() > ACCOUNTS_LIMIT {
            return Err("Cannot read the remembered Prism account store.".into());
        }
        let bytes = fs::read(&self.accounts_file)
            .map_err(|_| "Cannot read the remembered Prism account store.")?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "Cannot check the remembered Prism session time.")?
            .as_secs();
        let session = account_session(&bytes, self.bound_uuid, now)?;
        let response = super::agent()
            .get(PROFILE_URL)
            .set("Authorization", &format!("Bearer {}", session.access_token))
            .call()
            .map_err(|error| match error {
                ureq::Error::Status(401 | 403, _) => {
                    "The remembered Prism session needs renewal. Refresh that account in Prism."
                }
                _ => "Cannot verify the remembered Prism account with Minecraft services.",
            })?;
        if response.status() != 200 {
            return Err(
                "Cannot verify the remembered Prism account with Minecraft services.".into(),
            );
        }
        let body = response
            .into_string()
            .map_err(|_| "Cannot read the Minecraft profile verification response.")?;
        verified_profile(session, body.as_bytes())
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().count() <= 16
}

fn account_session(bytes: &[u8], bound_uuid: u128, now: u64) -> Result<AuthSession, String> {
    let store: Value = serde_json::from_slice(bytes)
        .map_err(|_| "Cannot read the remembered Prism account store.")?;
    let accounts = store
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or("Cannot read the remembered Prism account store.")?;
    let mut matching = accounts.iter().filter(|account| {
        account
            .pointer("/profile/id")
            .and_then(Value::as_str)
            .and_then(parse_uuid)
            == Some(bound_uuid)
    });
    let account = matching
        .next()
        .ok_or("The remembered Minecraft account is no longer in Prism.")?;
    if matching.next().is_some() {
        return Err("Prism contains multiple entries for the remembered Minecraft account.".into());
    }
    if account.get("type").and_then(Value::as_str) != Some("MSA")
        || account
            .pointer("/entitlement/ownsMinecraft")
            .and_then(Value::as_bool)
            != Some(true)
        || account
            .pointer("/entitlement/canPlayMinecraft")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(
            "The remembered Prism account is not ready to play Minecraft: Java Edition.".into(),
        );
    }
    let expires = account.pointer("/ygg/exp").and_then(Value::as_u64);
    if !expires.is_some_and(|expires| expires > now.saturating_add(120)) {
        return Err("The remembered Prism session expired. Refresh that account in Prism.".into());
    }
    let token = account
        .pointer("/ygg/token")
        .and_then(Value::as_str)
        .filter(|token| !token.trim().is_empty())
        .ok_or("The remembered Prism account has no Minecraft session. Refresh it in Prism.")?;
    let name = account
        .pointer("/profile/name")
        .and_then(Value::as_str)
        .filter(|name| valid_name(name))
        .ok_or("The remembered Prism account has no valid Minecraft profile name.")?;
    Ok(AuthSession {
        name: name.to_owned(),
        uuid: bound_uuid,
        access_token: token.to_owned(),
        xuid: None,
    })
}

fn verified_profile(mut session: AuthSession, bytes: &[u8]) -> Result<AuthSession, String> {
    let profile: Value = serde_json::from_slice(bytes)
        .map_err(|_| "Cannot read the Minecraft profile verification response.")?;
    if profile
        .get("id")
        .and_then(Value::as_str)
        .and_then(parse_uuid)
        != Some(session.uuid)
    {
        return Err(
            "Minecraft returned a different profile than the remembered Prism account.".into(),
        );
    }
    let name = profile
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| valid_name(name))
        .ok_or("Minecraft returned no valid profile name for the remembered account.")?;
    session.name = name.to_owned();
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const BOUND: u128 = 0x12345678123412341234123456789abc;
    const OTHER: u128 = 0xabcdefab123412341234123456789abc;
    const SECRET: &str = "synthetic-token-never-echo";

    #[test]
    fn implicit_session_is_only_for_normal_personal_client_launches() {
        assert!(implicit_session_allowed(&[]));
        assert!(implicit_session_allowed(&[
            "--control".into(),
            "--server".into(),
            "example.invalid".into(),
        ]));
        for option in [
            "--name",
            "--dedicated",
            "--session-stdin",
            "--session-file",
            "--help",
            "-h",
            "--launch-api",
            "--offscreen",
            "--headless",
            "--benchmark",
            "--frames",
            "--world-frames",
        ] {
            assert!(!implicit_session_allowed(&[option.into()]));
            assert!(!implicit_session_allowed(&[option.into(), String::new()]));
            assert!(!implicit_session_allowed(&[format!("{option}=value")]));
        }
    }

    fn account(uuid: u128, active: bool) -> Value {
        json!({
            "active": active,
            "type": "MSA",
            "profile": {"id": format!("{uuid:032x}"), "name": "BeforeRename"},
            "entitlement": {"ownsMinecraft": true, "canPlayMinecraft": true},
            "ygg": {"exp": 5000, "token": SECRET},
        })
    }

    fn store(accounts: Vec<Value>) -> Vec<u8> {
        serde_json::to_vec(&json!({"accounts": accounts})).unwrap()
    }

    #[test]
    fn remembers_bound_identity_when_another_prism_account_becomes_active() {
        let session = account_session(
            &store(vec![account(OTHER, true), account(BOUND, false)]),
            BOUND,
            1000,
        )
        .unwrap();
        assert_eq!(session.uuid, BOUND);
        assert_eq!(session.access_token, SECRET);
        assert!(!format!("{session:?}").contains(SECRET));
    }

    #[test]
    fn missing_duplicate_expired_and_unowned_accounts_never_switch_identity() {
        let mut expired = account(BOUND, false);
        expired["ygg"]["exp"] = json!(1120);
        let mut unowned = account(BOUND, false);
        unowned["entitlement"]["ownsMinecraft"] = json!(false);
        for accounts in [
            vec![account(OTHER, true)],
            vec![account(BOUND, false), account(BOUND, true)],
            vec![expired, account(OTHER, true)],
            vec![unowned, account(OTHER, true)],
        ] {
            let error = account_session(&store(accounts), BOUND, 1000)
                .err()
                .unwrap();
            assert!(!error.contains(SECRET));
        }
    }

    #[test]
    fn verified_remote_uuid_is_required_but_a_genuine_rename_is_used() {
        let session = || account_session(&store(vec![account(BOUND, true)]), BOUND, 1000).unwrap();
        let wrong =
            serde_json::to_vec(&json!({"id": format!("{OTHER:032x}"), "name": "Wrong"})).unwrap();
        assert!(verified_profile(session(), &wrong).is_err());
        let renamed =
            serde_json::to_vec(&json!({"id": format!("{BOUND:032x}"), "name": "AfterRename"}))
                .unwrap();
        assert_eq!(
            verified_profile(session(), &renamed).unwrap().name,
            "AfterRename"
        );
        let malformed = format!("invalid {SECRET}");
        assert!(
            !verified_profile(session(), malformed.as_bytes())
                .err()
                .unwrap()
                .contains(SECRET)
        );
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "horizon-local-profile-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn executable(&self) -> PathBuf {
            self.0.join("horizon-client.exe")
        }
        fn config(&self) -> Value {
            json!({"prism_accounts_file": self.0.join("synthetic-accounts.json"), "minecraft_uuid": format!("{BOUND:032x}")})
        }
        fn write(&self, value: &Value) {
            fs::write(self.0.join(CONFIG_FILE), serde_json::to_vec(value).unwrap()).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self.0.parent() == Some(std::env::temp_dir().as_path())
                && self
                    .0
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("horizon-local-profile-test-")
            {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn absent_config_is_opt_out_and_malformed_config_never_echoes_its_contents() {
        let fixture = Fixture::new();
        assert!(
            load_for_executable(&fixture.executable())
                .unwrap()
                .is_none()
        );
        fs::write(fixture.0.join(CONFIG_FILE), format!("broken {SECRET}")).unwrap();
        assert!(
            !load_for_executable(&fixture.executable())
                .err()
                .unwrap()
                .contains(SECRET)
        );
    }

    #[test]
    fn personal_paths_are_absolute_and_config_loading_does_not_open_account_store() {
        let fixture = Fixture::new();
        let mut config = fixture.config();
        config["resources"] = json!(fixture.0.join("resources"));
        config["game_dir"] = json!(fixture.0.join("game"));
        config["home"] = json!(fixture.0.join("home"));
        config["older_servers"] = json!(true);
        fixture.write(&config);
        let loaded = load_for_executable(&fixture.executable()).unwrap().unwrap();
        assert!(loaded.older_servers);
        assert_eq!(loaded.resources, Some(fixture.0.join("resources")));
        assert_eq!(loaded.game_dir, Some(fixture.0.join("game")));
        assert_eq!(loaded.home, Some(fixture.0.join("home")));
        assert!(!fixture.0.join("synthetic-accounts.json").exists());
        for key in ["prism_accounts_file", "resources", "game_dir", "home"] {
            let mut relative = config.clone();
            relative[key] = json!("relative/path");
            fixture.write(&relative);
            assert!(load_for_executable(&fixture.executable()).is_err(), "{key}");
        }
    }
}
