//! One downloadable executable; application files and game data stay in LocalAppData.
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, IsTerminal, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};

const PAYLOAD: &[u8] = include_bytes!(env!("HORIZON_OFFLINE_PAYLOAD"));
const PREFIX: &str = "Horizon-Offline/";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Horizon could not start: {error}\nOpen this EXE again to retry.");
            if std::io::stdin().is_terminal() {
                eprintln!("Press Enter to close.");
                let _ = std::io::stdin().read_line(&mut String::new());
            }
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !args.is_empty() && (args.len() != 1 || args[0] != "--setup-only") {
        return Err("The only supported option is --setup-only".into());
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or("Windows LOCALAPPDATA is missing or is not absolute")?;
    let root = base.join("HorizonOffline");
    reject_link(&root)?;
    fs::create_dir_all(&root).map_err(error)?;
    let lock_path = root.join(".single-exe.lock");
    reject_link(&lock_path)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(error)?;
    lock.try_lock()
        .map_err(|_| "Horizon is already running or setting up from this folder")?;
    println!("Checking Horizon's local application files...");
    install(PAYLOAD, &root)?;
    let status = Command::new(root.join("horizon-offline.exe"))
        .args(&args)
        .current_dir(&root)
        .status()
        .map_err(error)?;
    if !status.success() {
        return Err(format!("Automatic setup/game exited with {status}"));
    }
    Ok(())
}

fn install(payload: &[u8], root: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(payload)).map_err(error)?;
    let mut manifest = String::new();
    archive
        .by_name(&format!("{PREFIX}SHA256SUMS.txt"))
        .map_err(error)?
        .take(1024 * 1024 + 1)
        .read_to_string(&mut manifest)
        .map_err(error)?;
    if manifest.len() > 1024 * 1024 {
        return Err("Payload checksum list exceeds its size limit".into());
    }
    let mut expected = HashMap::new();
    for line in manifest.lines() {
        let (digest, name) = line
            .split_once("  ")
            .ok_or("Invalid payload checksum list")?;
        relative(name)?;
        if digest.len() != 64
            || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            || expected
                .insert(name.to_owned(), digest.to_ascii_lowercase())
                .is_some()
        {
            return Err("Invalid or duplicate payload checksum".into());
        }
    }
    expected.insert("SHA256SUMS.txt".into(), hash(manifest.as_bytes()));
    if archive.len() != expected.len() {
        return Err("Payload files do not match the checksum list".into());
    }
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(error)?;
        if entry.is_dir() || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err("Payload contains a link or directory entry".into());
        }
        let entry_name = entry.name().map_err(error)?.into_owned();
        let name = entry_name
            .strip_prefix(PREFIX)
            .ok_or("Invalid payload folder")?;
        let path = relative(name)?;
        let digest = expected
            .remove(name)
            .ok_or("Unexpected or duplicate payload file")?;
        let target = destination(root, &path)?;
        if fs::read(&target).is_ok_and(|bytes| hash(&bytes) == digest) {
            continue;
        }
        const MAX: u64 = 512 * 1024 * 1024;
        if entry.size() > MAX {
            return Err("Payload file exceeds its size limit".into());
        }
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(MAX + 1)
            .read_to_end(&mut bytes)
            .map_err(error)?;
        if bytes.len() as u64 > MAX || hash(&bytes) != digest {
            return Err("Payload file failed its checksum".into());
        }
        fs::create_dir_all(target.parent().ok_or("No payload parent folder")?).map_err(error)?;
        let partial = target.with_file_name(format!(
            "{}.partial",
            target
                .file_name()
                .ok_or("No payload file name")?
                .to_string_lossy()
        ));
        reject_link(&partial)?;
        fs::write(&partial, bytes)
            .and_then(|_| fs::rename(partial, &target))
            .map_err(error)?;
    }
    if !expected.is_empty() {
        return Err("Payload is missing required files".into());
    }
    Ok(())
}

fn relative(name: &str) -> Result<PathBuf, String> {
    let path = Path::new(name);
    let reserved = ["data", "resources", ".local", ".single-exe.lock"];
    let normal = !name.is_empty()
        && !name.contains(['\\', ':', '\0', '<', '>', '"', '|', '?', '*'])
        && name.split('/').all(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
            let device = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
                || (stem.len() == 4
                    && (stem.starts_with("com") || stem.starts_with("lpt"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'));
            !part.is_empty() && !part.ends_with(['.', ' ']) && !device
        })
        && path.components().all(|c| matches!(c, Component::Normal(_)))
        && !reserved.contains(
            &name
                .split('/')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
        );
    if !normal {
        return Err("Unsafe path in application payload".into());
    }
    Ok(path.to_path_buf())
}

fn destination(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    reject_link(root)?;
    let mut target = root.to_path_buf();
    for part in relative.components() {
        target.push(part);
        reject_link(&target)?;
    }
    Ok(target)
}

fn reject_link(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            #[cfg(windows)]
            let link = {
                use std::os::windows::fs::MetadataExt;
                meta.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let link = meta.file_type().is_symlink();
            if link {
                return Err("Refusing an application path redirected by a link or junction".into());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(error(e)),
    }
    Ok(())
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "horizon-single-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        assert_eq!(dir.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            dir.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("horizon-single-")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    fn payload(name: &str, bytes: &[u8], digest: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(format!("{PREFIX}SHA256SUMS.txt"), options)
            .unwrap();
        writeln!(zip, "{digest}  {name}").unwrap();
        zip.start_file(format!("{PREFIX}{name}"), options).unwrap();
        zip.write_all(bytes).unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn repairs_application_files_and_preserves_user_data() {
        let dir = temp();
        for name in ["data", "resources", ".local"] {
            fs::create_dir(dir.join(name)).unwrap();
            fs::write(dir.join(name).join("keep"), b"user data").unwrap();
        }
        fs::write(dir.join("horizon-offline.exe.partial"), b"interrupted").unwrap();
        let zip = payload("horizon-offline.exe", b"app", &hash(b"app"));
        install(&zip, &dir).unwrap();
        fs::write(dir.join("horizon-offline.exe"), b"corrupt").unwrap();
        install(&zip, &dir).unwrap();
        install(&zip, &dir).unwrap();
        assert_eq!(fs::read(dir.join("horizon-offline.exe")).unwrap(), b"app");
        assert!(!dir.join("horizon-offline.exe.partial").exists());
        for name in ["data", "resources", ".local"] {
            assert_eq!(fs::read(dir.join(name).join("keep")).unwrap(), b"user data");
        }
        cleanup(&dir);
    }

    #[test]
    fn bad_checksum_does_not_replace_an_existing_file() {
        let dir = temp();
        fs::write(dir.join("horizon-offline.exe"), b"original").unwrap();
        assert!(
            install(
                &payload("horizon-offline.exe", b"bad", &hash(b"good")),
                &dir
            )
            .is_err()
        );
        assert_eq!(
            fs::read(dir.join("horizon-offline.exe")).unwrap(),
            b"original"
        );
        cleanup(&dir);
    }

    #[test]
    fn refuses_paths_that_can_escape_or_overwrite_user_data() {
        let dir = temp();
        for name in [
            "../escape",
            "/absolute",
            "C:/absolute",
            "a/../../escape",
            "a\\b",
            "data/private",
            "DATA/private",
            "resources/private",
            ".local/private",
            ".single-exe.lock",
            "data./private",
            "x:stream",
            "NUL",
            "con.txt",
        ] {
            assert!(relative(name).is_err(), "accepted {name}");
            assert!(install(&payload(name, b"x", &hash(b"x")), &dir).is_err());
        }
        cleanup(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn refuses_existing_junctions_without_touching_their_targets() {
        let dir = temp();
        let root = dir.join("install");
        let outside = dir.join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("file"), b"private").unwrap();
        let link = root.join("mods");
        let status = Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command",
            "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:HZ_TEST_LINK -Target $env:HZ_TEST_TARGET | Out-Null"])
            .env("HZ_TEST_LINK", &link).env("HZ_TEST_TARGET", &outside).status().unwrap();
        assert!(status.success());
        assert!(install(&payload("mods/file", b"bad", &hash(b"bad")), &root).is_err());
        assert_eq!(fs::read(outside.join("file")).unwrap(), b"private");
        fs::remove_dir(link).unwrap();
        cleanup(&dir);
    }
}
