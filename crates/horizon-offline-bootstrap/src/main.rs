//! First-run setup for the portable offline client. No account stores are read.
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::Duration;

// Share the launcher's checked, resumable downloads without its UI or account backend.
#[allow(dead_code)]
#[path = "../../../launcher/src/mojang.rs"]
mod mojang;
#[allow(dead_code)]
#[path = "../../../launcher/src/process.rs"]
mod process;
#[allow(dead_code)]
#[path = "../../../launcher/src/sources.rs"]
mod sources;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!(
                "Setup could not finish: {error}\nRun Start-Horizon-Offline.cmd again to resume."
            );
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let setup_only = match args.as_slice() {
        [] => false,
        [arg] if arg == "--setup-only" => true,
        _ => return Err("The only supported option is --setup-only".into()),
    };
    let root = std::env::current_exe()
        .map_err(error)?
        .parent()
        .ok_or("No program folder")?
        .to_path_buf();
    fs::create_dir_all(root.join("data")).map_err(error)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("data/setup.lock"))
        .map_err(error)?;
    lock.try_lock()
        .map_err(|_| "Another copy is already setting up or running from this folder")?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout(Duration::from_secs(300))
        .user_agent("Horizon-Offline-Setup/0.1.1")
        .build();
    let fetch = |url: &str| fetch(&agent, url);
    let resources = root.join("resources");
    let prepare = root.join("horizon-prepare.exe");
    let ready = child(&prepare, &root)
        .args(["--check", "--root"])
        .arg(&root)
        .arg("--output")
        .arg(&resources)
        .output()
        .map_err(error)?
        .status
        .success()
        && runtime_ready(&root)
        && root.join("data/setup-complete").is_file();
    if !ready {
        let marker = root.join("data/setup-complete");
        if marker.exists() {
            fs::remove_file(marker).map_err(error)?;
        }
        println!(
            "First-run setup: downloading Minecraft 26.3 resources and portable Java.\nThis may take several minutes. Completed downloads are reused if interrupted."
        );
        setup(&root, &prepare, &resources, &fetch)?;
    } else {
        println!("Resources are ready; no resource downloads needed.");
    }
    if setup_only {
        println!("Resource setup complete.");
        return Ok(());
    }
    let status = child(&root.join("horizon-client.exe"), &root)
        .arg("--resources")
        .arg(&resources)
        .arg("--game-dir")
        .arg(root.join("data/game"))
        .arg("--home")
        .arg(root.join("data/home"))
        .args(["--name", "TestPlayer"])
        .status()
        .map_err(error)?;
    if !status.success() {
        return Err(format!("The game exited with {status}; see data/home/logs"));
    }
    Ok(())
}

fn setup(
    root: &Path,
    prepare: &Path,
    resources: &Path,
    fetch: sources::Fetch<'_>,
) -> Result<(), String> {
    let lock: Value =
        serde_json::from_slice(&fs::read(root.join("research-lock.json")).map_err(error)?)
            .map_err(error)?;
    let pins = sources::Pins::read(&lock).ok_or("Invalid Minecraft pins in research-lock.json")?;
    let game = root.join("data/minecraft");
    let log = |line: String| println!("{line}");
    mojang::download(&pins, &game, "windows", 8, fetch, &mojang::CANCEL, &log)?;
    let manifest: Value = serde_json::from_slice(
        &fs::read(
            game.join("versions")
                .join(&pins.version)
                .join(format!("{}.json", pins.version)),
        )
        .map_err(error)?,
    )
    .map_err(error)?;
    let server = game.join("server.jar");
    let expected = string(&lock["minecraft"], "server_sha1")?;
    if sources::sha1_file(&server).as_deref() != Some(expected) {
        println!("Downloading the pinned server files from Mojang");
        let body = sources::fetch_checked(
            fetch,
            string(&manifest["downloads"]["server"], "url")?,
            expected,
        )?;
        sources::write_file(&server, &body)?;
    }
    let java = mojang::java_runtime(
        string(&manifest["javaVersion"], "component")?,
        &root.join("data/runtime"),
        8,
        fetch,
        &mojang::CANCEL,
        &log,
    )?;
    copy_runtime(root, &java)?;
    println!("Preparing resources locally. No account data is used.");
    let status = child(prepare, root)
        .args(["--prepare", "--complete", "--root"])
        .arg(root)
        .arg("--minecraft-dir")
        .arg(&game)
        .arg("--server-jar")
        .arg(&server)
        .arg("--java-home")
        .arg(&java)
        .arg("--output")
        .arg(resources)
        .env("JAVA_HOME", &java)
        .status()
        .map_err(error)?;
    if !status.success() {
        return Err(format!("Resource preparation exited with {status}"));
    }
    sources::write_file(&root.join("data/setup-complete"), b"offline-setup-v1\n")?;
    Ok(())
}

fn child(program: &Path, root: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear();
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "SystemDrive",
        "TEMP",
        "TMP",
        "COMSPEC",
        "PATHEXT",
        "NUMBER_OF_PROCESSORS",
        "PROCESSOR_ARCHITECTURE",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "CommonProgramFiles",
        "HORIZON_NO_AUDIO",
    ] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd.current_dir(root)
        .env("APPDATA", root.join("data/appdata"))
        .env("LOCALAPPDATA", root.join("data/localappdata"))
        .env("HORIZON_HOME", root.join("data/home"))
        .env("HORIZON_LAUNCHER_HOME", root.join("data/launcher"))
        .env("XDG_DATA_HOME", root.join("data/xdg-data"))
        .env("XDG_CONFIG_HOME", root.join("data/xdg-config"))
        .env("HORIZON_LAUNCHER_NO_SELF_UPDATE", "1")
        .env("HORIZON_TELEMETRY_URL", "off");
    cmd
}

fn fetch(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") {
        return Err("Refusing a non-HTTPS download".into());
    }
    let response = agent.get(url).call().map_err(error)?;
    let mut body = Vec::new();
    const MAX: u64 = 512 * 1024 * 1024;
    response
        .into_reader()
        .take(MAX + 1)
        .read_to_end(&mut body)
        .map_err(error)?;
    if body.len() as u64 > MAX {
        return Err("Download exceeds the 512 MiB per-file limit".into());
    }
    Ok(body)
}

const RUNTIME_DLLS: [&str; 2] = ["vcruntime140.dll", "vcruntime140_1.dll"];

fn runtime_ready(root: &Path) -> bool {
    RUNTIME_DLLS.iter().all(|name| root.join(name).is_file())
}

fn copy_runtime(root: &Path, java: &Path) -> Result<(), String> {
    // These app-local DLLs were just hash-checked by Mojang's runtime downloader.
    let files: Result<Vec<_>, _> = RUNTIME_DLLS
        .iter()
        .map(|name| fs::read(java.join("bin").join(name)).map(|bytes| (name, bytes)))
        .collect();
    for (name, bytes) in files.map_err(error)? {
        sources::write_file(&root.join(name), &bytes)?;
    }
    Ok(())
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing {key} in setup metadata"))
}

fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_java_runtime_does_not_replace_existing_dlls() {
        let temp = std::env::temp_dir();
        let dir = temp.join(format!("horizon-offline-crt-{}", std::process::id()));
        let java = dir.join("java");
        fs::create_dir_all(java.join("bin")).unwrap();
        fs::write(dir.join(RUNTIME_DLLS[0]), b"existing").unwrap();
        fs::write(java.join("bin").join(RUNTIME_DLLS[0]), b"first").unwrap();
        assert!(copy_runtime(&dir, &java).is_err());
        assert_eq!(fs::read(dir.join(RUNTIME_DLLS[0])).unwrap(), b"existing");
        fs::write(java.join("bin").join(RUNTIME_DLLS[1]), b"second").unwrap();
        copy_runtime(&dir, &java).unwrap();
        assert!(runtime_ready(&dir));
        assert_eq!(fs::read(dir.join(RUNTIME_DLLS[0])).unwrap(), b"first");
        assert_eq!(dir.parent(), Some(temp.as_path()));
        fs::remove_dir_all(dir).unwrap();
    }
}
