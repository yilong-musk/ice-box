// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared DTOs with no I/O and no `cfg(target_os)` (architecture review ARCH-1).
//!
//! `AppPaths` / `AppSettings` stay in `ice-config` because they own disk
//! layout and load/save. Pid-file and tracing helpers stay there too so
//! `ice-core` can use them without depending on the desktop shell or
//! `ice-engine` (which would pull in `ice-subscription`).

mod error;
mod platform;

pub use error::{AppError, ErrorCode};
pub use platform::HostPlatform;

/// sing-box core version the config generator targets (architecture §12 / §22).
///
/// Re-exported by `ice-engine` as the public pin. Bundled desktop binaries
/// (`third_party/sing-box/VERSION`) must match; generated config features
/// are only tested against this version range.
pub const ENGINE_COMPAT_CORE_VERSION: &str = "1.13.19";
