// SPDX-License-Identifier: GPL-3.0-or-later

//! Structured UI copy: a message key plus interpolation params (FE-5).
//!
//! The frontend runs `t(key, params)`. Legacy on-disk strings deserialize as
//! [`UiMessage::raw`].

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Key used when a stored string has no translation key (legacy `last_error`).
pub const UI_RAW_KEY: &str = "ui.raw";

/// User-visible message with a stable i18n key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UiMessage {
    pub key: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

impl UiMessage {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            params: BTreeMap::new(),
        }
    }

    pub fn with(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(name.into(), value.into());
        self
    }

    /// Untranslated leftover (legacy files, sing-box log excerpts).
    pub fn raw(text: impl Into<String>) -> Self {
        Self::new(UI_RAW_KEY).with("text", text.into())
    }

    pub fn is_raw(&self) -> bool {
        self.key == UI_RAW_KEY
    }
}

impl fmt::Display for UiMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_raw() {
            return write!(
                f,
                "{}",
                self.params.get("text").map(String::as_str).unwrap_or("")
            );
        }
        write!(f, "{}", self.key)?;
        if !self.params.is_empty() {
            write!(f, " (")?;
            let mut first = true;
            for (k, v) in &self.params {
                if !first {
                    write!(f, ", ")?;
                }
                first = false;
                write!(f, "{k}={v}")?;
            }
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl From<&str> for UiMessage {
    fn from(text: &str) -> Self {
        UiMessage::raw(text)
    }
}

impl From<String> for UiMessage {
    fn from(text: String) -> Self {
        UiMessage::raw(text)
    }
}

impl<'de> Deserialize<'de> for UiMessage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UiMessageVisitor;
        impl<'de> Visitor<'de> for UiMessageVisitor {
            type Value = UiMessage;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a UI message object or a legacy string")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UiMessage::raw(v))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(UiMessage::raw(v))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut key = None;
                let mut params = BTreeMap::new();
                while let Some(k) = map.next_key::<String>()? {
                    match k.as_str() {
                        "key" => key = Some(map.next_value::<String>()?),
                        "params" => params = map.next_value()?,
                        _ => {
                            let _: de::IgnoredAny = map.next_value()?;
                        }
                    }
                }
                let key = key.ok_or_else(|| de::Error::missing_field("key"))?;
                Ok(UiMessage { key, params })
            }
        }
        deserializer.deserialize_any(UiMessageVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_string_deserializes_as_raw() {
        let msg: UiMessage = serde_json::from_str("\"network down\"").unwrap();
        assert_eq!(msg.key, UI_RAW_KEY);
        assert_eq!(
            msg.params.get("text").map(String::as_str),
            Some("network down")
        );
    }

    #[test]
    fn structured_roundtrip() {
        let msg = UiMessage::new("parse.truncated")
            .with("kind", "nodes")
            .with("dropped", "3");
        let json = serde_json::to_string(&msg).unwrap();
        let back: UiMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }
}
