//! Title-screen states and their stable native screen identifiers.
use super::*;

#[derive(Clone, Debug)]
pub enum Mode {
    Main,
    Select {
        worlds: Vec<WorldEntry>,
        selected: Option<usize>,
    },
    Create {
        name: String,
        seed: String,
        creative: bool,
        focus: u8,
        options: WorldOptions,
    },
    Edit {
        world: WorldEntry,
        name: String,
        focused: bool,
    },
    Backup {
        world: WorldEntry,
    },
    /// BackupConfirmScreen over a world whose file fixes are due, or, with `downgrade`
    /// (the saved version's name), over a world saved by a newer version.
    BackupBeforeUpgrade {
        world: WorldEntry,
        downgrade: Option<String>,
        /// With neither of the above: `WorldOpenFlows.askForBackup` over a customized or
        /// experimental world (`openWorldCheckWorldStemCompatibility`).
        stem: Option<StemIssue>,
    },
    /// EditWorldScreen's Optimize World: BackupConfirmScreen with the Erase Cached Data box.
    OptimizeConfirm {
        world: WorldEntry,
        /// The Edit World screen's name field, restored by Cancel.
        name: String,
        erase_cache: bool,
    },
    /// `OptimizeWorldScreen`: the progress of `Title.optimize`.
    Optimize {
        world: WorldEntry,
    },
    /// `openWorldCheckDiskSpace`'s ConfirmScreen: little space is left on the device.
    LowDiskSpace {
        world: WorldEntry,
    },
    Delete {
        world: WorldEntry,
    },
    /// RecoverWorldDataScreen: opening a world whose level.dat does not read
    /// (WorldOpenFlows.openWorldLoadLevelData) lists what is wrong with
    /// level.dat and level.dat_old and offers to go back to the old one.
    Recover {
        world: WorldEntry,
        /// `collectIssue` for level.dat, then level.dat_old.
        issues: [Option<crate::save::DatIssue>; 2],
    },
    /// Where `attemptRestore` ends: the "Recovery done" ConfirmScreen
    /// (Continue opens the world), or its AlertScreen when it failed.
    Recovered {
        world: WorldEntry,
        success: bool,
    },
    Error {
        message: String,
        return_to: Box<Mode>,
        /// An AlertScreen, not the access failure.
        alert: Option<Alert>,
    },
    Multiplayer(crate::multiplayer::Screen),
    Realms(crate::realms::Screen),
    /// SafetyScreen: the warning before the first Multiplayer click, with its
    /// "Do not show this screen again" box.
    MultiplayerWarning {
        check: bool,
    },
    /// CreateFlatWorldScreen over Create New World: the fields that screen
    /// keeps and the superflat editor on top of them.
    CreateFlat {
        name: String,
        seed: String,
        creative: bool,
        focus: u8,
        options: WorldOptions,
        flat: flat::State,
    },
    /// CreateBuffetWorldScreen over Create New World: the single-biome
    /// preset's biome list, over the fields that screen keeps.
    CreateBuffet {
        name: String,
        seed: String,
        creative: bool,
        focus: u8,
        options: WorldOptions,
        buffet: buffet::State,
    },
    CreateCustom {
        return_to: Box<Mode>,
        state: custom_preset::State,
    },
    /// AccessibilityOnboardingScreen: the first screen of a first launch.
    Onboarding(Onboarding),
    /// `BanNoticeScreens`: a ConfirmLinkScreen `Gui.addInitialScreens` queued.
    BanNotice(crate::ban_notice::Notice),
}

impl Mode {
    /// The id native mods see for this screen (`NativeScreenView.id`, the
    /// screen hooks): one per variant, a hook's contract, so it stays the
    /// same from release to release. A new variant must add one.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Main => "title",
            Self::Realms(_) => "realms",
            Self::Select { .. } => "world_select",
            Self::Create { .. } => "world_create",
            Self::CreateFlat { .. } => "world_create_flat",
            Self::CreateBuffet { .. } => "world_create_buffet",
            Self::CreateCustom { .. } => "world_create_custom",
            Self::Edit { .. } => "world_edit",
            Self::Backup { .. } => "world_backup",
            Self::BackupBeforeUpgrade { .. } => "world_backup_confirm",
            Self::OptimizeConfirm { .. } => "world_optimize_confirm",
            Self::Optimize { .. } => "world_optimize",
            Self::LowDiskSpace { .. } => "low_disk_space",
            Self::Delete { .. } => "world_delete",
            Self::Recover { .. } => "world_recover",
            Self::Recovered { .. } => "world_recovered",
            Self::Error { .. } => "error",
            Self::Multiplayer(screen) => screen.id(),
            Self::MultiplayerWarning { .. } => "multiplayer_warning",
            Self::Onboarding(_) => "onboarding",
            Self::BanNotice(_) => "ban_notice",
        }
    }
}
