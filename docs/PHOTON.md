# Original Photon on Horizon

This branch runs the original Photon shader pack through Horizon's GLSL compatibility renderer. It adds generic support for per-draw item IDs, lightning position, pack wetness settings, vertical terrain draw distance, and `rain.depth`. The original cloud, lighting, shadow, water, bloom and antialiasing programs remain in the separately distributed Photon pack.

Get the Windows renderer, original pack, checksums and validation artifacts from [photon-v0.2.0](https://github.com/TheApple05/horizon/releases/tag/photon-v0.2.0). The source branch is `codex/photon-renderer`.

## Pinned versions and licenses

- Horizon base: `b33bc70c4fd2e18a5519a13189590cb7b106c556`.
- Photon by SixthSurge: [`40c887116c70d47c9e7e36cbad3178e04e622746`](https://github.com/sixthsurge/photon/tree/40c887116c70d47c9e7e36cbad3178e04e622746).
- Windows R2 executable: `horizon-client-photon-r2.exe`, SHA-256 `bc08ca24d50bc678e21896bb465fc7a5b92a6346325a3ca8f8a75d884d7d1f72`.

The Horizon engine changes remain MIT-licensed. No Photon implementation, Minecraft game code, game assets or prepared resources are added to this source tree. The original Photon ZIP is a separate release asset under Photon's own license, with its original notices retained. All 616 files under its `shaders/` tree were checked byte-for-byte against the pinned Photon revision. The engine's MIT license does not relicense that pack.

The compatibility hooks follow public [shader-format documentation](https://shaders.properties/current/reference/) and independent runtime probes. The Iris implementation was not used as source.

## Build and run

You need Rust 1.90 or newer, Python 3.10 or newer, JDK 25, and your own official Minecraft: Java Edition 26.3 client installation and server JAR. Prepare resources from that owned installation using the [source build instructions](../README.md#build-from-source); no Minecraft resources are bundled with this branch or release.

From this branch's repository root:

```powershell
cargo run --release -p horizon-prepare -- --prepare --server-jar "C:\path\to\26.3-server.jar"
cargo build --profile fast -p horizon-client
```

The second command produces `target\fast\horizon-client.exe`. R2 used the optimized `fast` profile (`lto=false`, `codegen-units=16`). The release launcher selects the separately installed original Photon pack and reuses locally prepared 26.3 resources.

The Photon launcher sets `HORIZON_SHADER_IRIS_1_11_7=1`. This enables only the wetness smoothing behavior measured from Iris 1.11.7: the last valid `wetnessHalflife` or lowercase `drynessHalflife` assignment selects the rising half-life, while drying retains its default. Original Photon consequently wets with about a 7-second half-life and dries with about a 20-second half-life. Ordinary launches keep the documented independent settings. This is a bounded observed compatibility profile, not complete Iris version emulation.

## Recorded validation

The delivered R2 executable completed strict High and Ultra runtime audits in Overworld, Nether and End: ten captures per profile, twenty total, with normal game exits and unchanged Photon shader bytes. Required raster and compute passes have submitted GPU timing evidence. Ultra's compute-only `setup` node and its session-level initialization evidence are reported separately from steady raster requirements; retained timings do not prove initialization independently for every dimension.

Recorded checks passed: 175 shader tests, 281 renderer tests (9 ignored), 76 gameplay item tests (1 ignored), and 16 delivery audit/comparison Python tests. Additional real GPU fixtures check CPU/GPU item pixels, separate entity and item IDs, both first-person hands, non-item resets, and rain depth/occlusion for omitted, false and true `rain.depth`.

Same-world High-preset comparisons against original Photon on Minecraft 26.3 with Iris cover daylight, sky, sunset, night, rain, water, emissive blocks and underwater refraction. They provide visual comparison evidence, not a guarantee of identical pixels for every scene or setting. Animation clocks and particle positions are not synchronized between engines.

Runtime validation used Windows and an NVIDIA GeForce RTX 3090 with Vulkan. DirectX 12, OpenGL, other GPUs, and multiplayer/mod combinations were not validated for this delivery.
