//! Horizon: native Rust client for Minecraft Java Edition 26.3 gameplay.
#![forbid(unsafe_code)]

// Windows GNU mimalloc retains private commit across repeated region changes.
#[cfg(all(
    not(any(feature = "system-allocator", feature = "alloc-census")),
    not(all(target_os = "windows", target_env = "gnu"))
))]
#[global_allocator]
static ALLOCATOR: horizon_threads::Allocator = horizon_threads::ALLOCATOR;
#[cfg(all(feature = "alloc-census", not(feature = "system-allocator")))]
#[global_allocator]
static ALLOCATOR: horizon_threads::CensusAllocator = horizon_threads::CensusAllocator;

/// The mod API level this game serves, kept in the program for the launcher
/// to read without running it (`horizon_mods::api_level::MARK`). `main`
/// reads it, so the linker keeps it.
static MOD_API_LEVEL: [u8; 27] = horizon_mods::api_level::MARK;

fn main() {
    std::hint::black_box(&MOD_API_LEVEL);
    horizon_client::entry::main();
}
