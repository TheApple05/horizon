# Offline testing fork

Based on horizon-rust/horizon main at c98a51a. Source branch: https://github.com/TheApple05/horizon/tree/offline-private (public). Prebuilt Windows x64 downloads are published under the offline-26.3-v0.1.1 release. No personal data or Minecraft resources are uploaded.

The client ignores Microsoft session files, stdin sessions and launcher account lists. Its default name is TestPlayer, even if HORIZON_PLAYER_NAME is inherited. Explicit --name remains available for offline multiplayer tests. Telemetry uploads are disabled in code. Default features include audio and controllers, without Horizon extras (Discord, friends, heartbeat, cosmetics and official server additions).

Download Horizon-Offline-Auto-Windows.zip from https://github.com/TheApple05/horizon/releases/tag/offline-26.3-v0.1.1, extract the whole ZIP into a writable folder, and double-click Start-Horizon-Offline.cmd. First launch downloads the pinned game files, assets and portable Java from Mojang, verifies hashes, and runs the bundled static preparer and original exporters. Java's verified Windows runtime DLLs are placed beside the game. Nothing needs to be copied from another PC; no compiler, installed Java, VC++ installer or account sign-in is required. Internet is needed for the first setup. Run the same launcher again after an interruption to resume completed downloads.

After successful preparation, local checks and a completion marker let later launches skip resource downloads. All downloaded files and generated resources stay in the extracted folder's data/, resources/ and .local/. These files, player data and account stores are excluded from the public ZIP. The downloader uses the launcher's existing mojang.rs and sources.rs without importing its account backend. Preparation and game processes receive a small allowlist of environment variables and isolated app-data directories.

This provides offline play and access to offline-mode test servers. It does not authenticate to online-mode servers. Multiplayer servers can see your IP address and the name you choose. It is not an anonymity tool, and manually adding mods or resource packs can add network behavior.

Build: cargo build --release --locked -p horizon-client --bin horizon-client
The delivered build disables LTO for faster linking, uses 16 codegen units and strips symbols. Rust paths under the build user's profile are remapped to /build. No account stores, worlds, logs, prepared game resources or debug symbols are packaged.

Crash stack traces shorten workspace paths without embedding the build machine directory. Verify with tools/test_offline_fork.py BINARY RESOURCES; telemetry endpoint coverage is in horizon-telemetry, and path-shortening coverage is in the renderer crash tests.


The automatic setup binary is crates/horizon-offline-bootstrap (horizon-offline.exe). Build it and horizon-prepare with -C target-feature=+crt-static so first-run setup needs only Windows system DLLs. Package with tools/package_offline_release.py using a clean support root containing the preparer, research-lock.json, original Java sources, exporters.rs and matching exporters/ classes. horizon-prepare compile-exporters builds that exporter bundle from verified local game files. The public package contains original exporter classes only, no Minecraft jars or prepared resources.

Validation: cargo test --release -p horizon-offline-bootstrap exercises the reused mock download tests and app-local runtime failure handling. Run horizon-offline.exe --setup-only in a fresh extracted package for an end-to-end download/preparation test, repeat it to check the local fast path, then run tools/test_offline_fork.py against the newly generated resources. A full clean Windows installation remains a separate hardware/OS test.
