Horizon Offline 26.3 - Automatic Setup - Windows x64

Extract the whole ZIP into a writable folder. Double-click Start-Horizon-Offline.cmd.

On first launch, setup downloads the pinned Minecraft files and portable Java from Mojang, verifies vendor hashes, and prepares the resources locally. It also places Java's verified Windows runtime DLLs beside the game. No file copying, account sign-in, compiler, Java installation or administrator installer is needed. An internet connection is required for first-run downloads; keep the setup window open until it starts the game. Interrupted downloads resume when you run the same launcher again.

Once setup succeeds, later launches reuse the resources without contacting the download servers. A Windows 10/11 x64 PC and a GPU supporting Vulkan or DirectX 12 are required. The executables are unsigned.

The game uses TestPlayer. Saves, settings, caches and local setup logs stay in this folder's data/, resources/ and .local/ directories. Account stores are not read or copied. The download does not include Minecraft files, saved worlds, personal logs or credentials. Resource downloads contact Mojang and reveal the usual network information, including your IP address, to its servers. Telemetry, Discord presence, friends and heartbeat are disabled.

Single-player and offline-mode servers are supported. This does not authenticate to online-mode servers. Servers you join can see your IP and chosen name. Keep your downloaded game resources and data private.

Advanced: horizon-offline.exe --setup-only prepares resources without opening the game.

Source: https://github.com/TheApple05/horizon/tree/offline-private
Horizon is MIT licensed. Portable Java and game resources are downloaded from Mojang at runtime with their vendor license files; they are not part of this ZIP. Original Horizon exporters and their Java sources are included to prepare resources. Source patch and per-file SHA256SUMS.txt are included.
