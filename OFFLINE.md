# Offline testing fork

Based on horizon-rust/horizon main at c98a51a. This is a local fork; nothing was uploaded.

The client ignores Microsoft session files, stdin sessions and launcher account lists. Its default name is TestPlayer, even if HORIZON_PLAYER_NAME is inherited. Explicit --name remains available for offline multiplayer tests. Telemetry uploads are disabled in code. Default features include audio and controllers, without Horizon extras (Discord, friends, heartbeat, cosmetics and official server additions).

Start dist/Horizon-Offline/Start-Horizon-Offline.cmd. Game data stays in that folder's data directory. Prepared resources are linked locally from the existing install and are not included in the ZIP. Do not share the data folder or the resource link. On another machine, supply that machine's own prepared Minecraft 26.3 resources in resources/.

This provides offline play and access to offline-mode test servers. It does not authenticate to online-mode servers. Multiplayer servers can see your IP address and the name you choose. It is not an anonymity tool, and manually adding mods or resource packs can add network behavior.

Build: cargo build --release --locked -p horizon-client --bin horizon-client
The delivered build disables LTO for faster linking, uses 16 codegen units and strips symbols. Rust paths under the build user's profile are remapped to /build. No account stores, worlds, logs, prepared game resources or debug symbols are packaged.

Crash stack traces shorten workspace paths without embedding the build machine directory. Verify with tools/test_offline_fork.py BINARY RESOURCES; telemetry endpoint coverage is in horizon-telemetry, and path-shortening coverage is in the renderer crash tests.
