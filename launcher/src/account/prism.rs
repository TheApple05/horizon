//! Import only Prism's current Minecraft session; Microsoft refresh tokens stay in Prism.
use super::*;
use crate::i18n::tr;
use std::io::Read;

pub fn import(ep: &Endpoints, path: &Path) -> Result<(Account, Secrets), String> {
    let file =
        std::fs::File::open(path).map_err(|_| tr("Could not open Prism's accounts file."))?;
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| tr("Could not read Prism's accounts file."))?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(tr("Prism's accounts file is too large.").into());
    }
    import_bytes(ep, &bytes)
}

pub(super) fn import_bytes(ep: &Endpoints, bytes: &[u8]) -> Result<(Account, Secrets), String> {
    let document: Value =
        serde_json::from_slice(bytes).map_err(|_| tr("Could not parse Prism's accounts file."))?;
    let accounts = document["accounts"]
        .as_array()
        .ok_or_else(|| tr("Prism's accounts file has no accounts."))?;
    let active: Vec<_> = accounts
        .iter()
        .filter(|a| a["active"] == true && a["type"] == "MSA")
        .collect();
    if active.len() != 1 {
        return Err(tr("Select one Microsoft account in Prism Launcher first.").into());
    }
    let entry = active[0];
    let token = entry["ygg"]["token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| tr("Prism has no Minecraft session. Sign in with Prism first."))?;
    let expires = entry["ygg"]["exp"]
        .as_u64()
        .filter(|t| *t > now() + RENEW_BEFORE)
        .ok_or_else(|| {
            tr("Open Prism Launcher, refresh its account, then import its session again.")
        })?;
    if !owns_game(ep, token)? {
        return Err(DOES_NOT_OWN.into());
    }
    let account = profile(ep, token)?.ok_or(NO_PROFILE)?;
    if entry["profile"]["id"]
        .as_str()
        .map(|id| id.replace('-', "").to_lowercase())
        .as_deref()
        != Some(&account.uuid)
    {
        return Err(OTHER_PROFILE.into());
    }
    let secrets = Secrets::new(
        String::new(),
        Minecraft {
            token: token.into(),
            expires_at: expires,
            xuid: None,
        },
    );
    Ok((account, secrets))
}
