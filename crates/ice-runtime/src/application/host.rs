// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;
use std::sync::Arc;

/// Shell capabilities required by application use cases, without a UI runtime.
pub trait AppResources {
    fn resource_dir(&self) -> Option<PathBuf>;

    /// Whether `resource_dir()` should be preferred over the compile-time
    /// development tree (`third_party/sing-box`) when resolving bundled
    /// binaries. True when the caller explicitly passed `--resource-dir` or
    /// `ICE_BOX_RESOURCE_DIR`, or runs from a packaged CLI archive.
    fn prefer_resource_dir(&self) -> bool {
        false
    }
}

pub trait AppHost: AppResources {
    /// Called only after mutation locks have been released.
    fn state_changed(&self);

    /// Whether interactive OS GUI elevation prompts (Polkit, macOS
    /// AuthorizationServices, Windows UAC) are allowed. Headless daemons
    /// override this to `false` by default so automated callers fail fast with
    /// an actionable error instead of stalling on a missing GUI prompt.
    fn interactive_elevation(&self) -> bool {
        true
    }
}

impl<T: AppResources + ?Sized> AppResources for &T {
    fn resource_dir(&self) -> Option<PathBuf> {
        (**self).resource_dir()
    }

    fn prefer_resource_dir(&self) -> bool {
        (**self).prefer_resource_dir()
    }
}

impl<T: AppHost + ?Sized> AppHost for &T {
    fn state_changed(&self) {
        (**self).state_changed();
    }

    fn interactive_elevation(&self) -> bool {
        (**self).interactive_elevation()
    }
}

impl<T: AppResources + ?Sized> AppResources for Arc<T> {
    fn resource_dir(&self) -> Option<PathBuf> {
        (**self).resource_dir()
    }

    fn prefer_resource_dir(&self) -> bool {
        (**self).prefer_resource_dir()
    }
}

impl<T: AppHost + ?Sized> AppHost for Arc<T> {
    fn state_changed(&self) {
        (**self).state_changed();
    }

    fn interactive_elevation(&self) -> bool {
        (**self).interactive_elevation()
    }
}
