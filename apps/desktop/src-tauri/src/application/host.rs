// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

/// Shell capabilities required by application use cases, without a UI runtime.
pub(crate) trait AppResources {
    fn resource_dir(&self) -> Option<PathBuf>;
}

pub(crate) trait AppHost: AppResources {
    /// Called only after mutation locks have been released.
    fn state_changed(&self);
}
