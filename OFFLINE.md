# Offline testing fork

Based on horizon-rust/horizon main at c98a51a. Source branch: https://github.com/TheApple05/horizon/tree/offline-private (public). Prebuilt Windows x64 downloads are published under the offline-26.3-v0.1.0 release. No personal data or Minecraft resources are uploaded.

The client ignores Microsoft session files, stdin sessions and launcher account lists. Its default name is TestPlayer, even if HORIZON_PLAYER_NAME is inherited. Explicit --name remains available for offline multiplayer tests. Telemetry uploads are disabled in code. Default features include audio and controllers, without Horizon extras (Discord, friends, heartbeat, cosmetics and official server additions).

Download Horizon-Offline-Windows.zip from https://github.com/TheApple05/horizon/releases/tag/offline-26.3-v0.1.0 and extract it. No compiler, build tools or Java are needed on the other PC. Install the Microsoft Visual C++ x64 runtime if it is not already present (https://aka.ms/vc14/vc_redist.x64.exe). Copy the contents of your existing prepared Minecraft 26.3 resource folder into Horizon-Offline/resources/ next to the executable, so resources/horizon-resources.json exists. Then double-click Start-Horizon-Offline.cmd. Game data stays in the package's own data directory. Copy the actual resource files, not a Windows junction or shortcut; do not copy account stores, saves or logs. Resources are deliberately not included in the public ZIP.

This provides offline play and access to offline-mode test servers. It does not authenticate to online-mode servers. Multiplayer servers can see your IP address and the name you choose. It is not an anonymity tool, and manually adding mods or resource packs can add network behavior.

Build: cargo build --release --locked -p horizon-client --bin horizon-client
The delivered build disables LTO for faster linking, uses 16 codegen units and strips symbols. Rust paths under the build user's profile are remapped to /build. No account stores, worlds, logs, prepared game resources or debug symbols are packaged.

Crash stack traces shorten workspace paths without embedding the build machine directory. Verify with tools/test_offline_fork.py BINARY RESOURCES; telemetry endpoint coverage is in horizon-telemetry, and path-shortening coverage is in the renderer crash tests.
