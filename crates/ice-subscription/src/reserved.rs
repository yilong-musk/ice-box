// SPDX-License-Identifier: GPL-3.0-or-later

//! Keep subscription outbounds off the tags the app owns (issue #114).
//!
//! Every parse path funnels through [`rename_reserved_tags`]. `direct`,
//! `block`, and `dns` always move. `proxy` moves on a group-less profile,
//! where the builder injects a selector of that name, and when two outbounds
//! already share it. One grouped outbound named `proxy` stays: that profile
//! injects no selector, and renaming the tag would drop favorites and saved
//! selections. A collision that is left in place either fails the build
//! (`duplicate tag proxy`) or silently routes "direct" traffic through the
//! subscription server. The rename itself lives in `ice-config` because the
//! builder and the share exporter need the same list.

use ice_config::{rename_reserved_outbounds, NormalizedProfile};

/// Rename colliding outbounds and record one `parse.renamedReservedTag`
/// warning per rename on the profile's parse stats. A new import copies those
/// warnings onto the subscription. A profile migrated on read does not:
/// subscription details keep the import-time snapshot.
pub(crate) fn rename_reserved_tags(profile: &mut NormalizedProfile) {
    let warnings = rename_reserved_outbounds(profile);
    profile.parse_stats.warnings.extend(warnings);
}

#[cfg(test)]
mod tests {
    use ice_config::NormalizedProfile;

    use super::rename_reserved_tags;
    use crate::NormalizedOutbound;

    fn node(tag: &str) -> NormalizedOutbound {
        NormalizedOutbound::new(
            tag,
            serde_json::json!({
                "type": "socks",
                "tag": tag,
                "server": "127.0.0.1",
                "server_port": 1080
            }),
        )
    }

    #[test]
    fn records_a_warning_for_each_rename() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![node("proxy"), node("jp")]);
        rename_reserved_tags(&mut profile);
        assert_eq!(profile.nodes[0].tag, "proxy-2");
        assert_eq!(profile.parse_stats.warnings.len(), 1);
        assert_eq!(
            profile.parse_stats.warnings[0].key,
            "parse.renamedReservedTag"
        );
        assert_eq!(profile.parse_stats.warnings[0].params["new_tag"], "proxy-2");
    }

    #[test]
    fn leaves_a_clean_profile_untouched() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![node("jp")]);
        rename_reserved_tags(&mut profile);
        assert_eq!(profile.nodes[0].tag, "jp");
        assert!(profile.parse_stats.warnings.is_empty());
    }
}
