// SPDX-License-Identifier: GPL-3.0-or-later

//! Generated sing-box config whose outbound trees are not deep-copied on emit (PERF-1).

use serde::ser::{SerializeSeq, SerializeStruct, Serializer};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

/// Final runtime config written to `config.json`.
///
/// `outbounds` keep the profile's `Arc<Value>` when the object is unchanged
/// (tag already set, no selector default patch). Serialization walks each
/// `Arc` by reference so the production path never builds a second outbound
/// `Value` tree.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub log: Value,
    pub dns: Value,
    pub inbounds: Vec<Value>,
    pub outbounds: Vec<Arc<Value>>,
    pub route: Value,
    pub experimental: Value,
}

impl RuntimeConfig {
    /// Pretty-print without first materializing a root `Value`.
    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Test / debug helper: clone into a `Value` tree.
    pub fn to_json_value(&self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self)
    }
}

impl Serialize for RuntimeConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("RuntimeConfig", 6)?;
        state.serialize_field("log", &self.log)?;
        state.serialize_field("dns", &self.dns)?;
        state.serialize_field("inbounds", &self.inbounds)?;
        state.serialize_field("outbounds", &OutboundList(&self.outbounds))?;
        state.serialize_field("route", &self.route)?;
        state.serialize_field("experimental", &self.experimental)?;
        state.end()
    }
}

struct OutboundList<'a>(&'a [Arc<Value>]);

impl Serialize for OutboundList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for outbound in self.0 {
            seq.serialize_element(outbound.as_ref())?;
        }
        seq.end()
    }
}

/// Cheap `Arc` clone when `tag` is already correct; otherwise clone-on-write.
pub(crate) fn tagged_outbound(src: &Arc<Value>, tag: &str) -> Arc<Value> {
    if src.get("tag").and_then(|v| v.as_str()) == Some(tag) {
        return Arc::clone(src);
    }
    let mut value = (**src).clone();
    if let Some(obj) = value.as_object_mut() {
        obj.insert("tag".into(), Value::String(tag.to_string()));
    }
    Arc::new(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tagged_outbound_reuses_arc_when_tag_matches() {
        let src = Arc::new(json!({"type": "socks", "tag": "n1", "server": "127.0.0.1"}));
        let out = tagged_outbound(&src, "n1");
        assert!(Arc::ptr_eq(&src, &out));
    }

    #[test]
    fn serialize_pretty_walks_arc_outbounds() {
        let cfg = RuntimeConfig {
            log: json!({"level": "warn"}),
            dns: json!({"final": "local"}),
            inbounds: vec![json!({"type": "mixed", "tag": "mixed-in"})],
            outbounds: vec![Arc::new(json!({"type": "direct", "tag": "direct"}))],
            route: json!({"final": "direct"}),
            experimental: json!({"clash_api": {}}),
        };
        let text = cfg.to_pretty_json().expect("pretty");
        assert!(text.contains("\"tag\": \"direct\""));
        let value = cfg.to_json_value().expect("value");
        assert_eq!(value["outbounds"][0]["tag"], "direct");
    }
}
