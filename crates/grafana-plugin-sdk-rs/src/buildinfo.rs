/*! Plugin build information.

Mirrors the Go SDK's `build/buildinfo` package: a small struct describing the
plugin's identity and version, which Grafana surfaces in its plugin catalogue.

The [`build_info!`](crate::build_info) macro captures this from the calling
crate's Cargo package metadata at compile time:

```rust
let info = grafana_plugin_sdk::build_info!();
assert!(!info.version.is_empty());
```
*/
use serde::{Deserialize, Serialize};

/// Build information about a plugin.
///
/// See also `PluginBuildInfo` in Grafana's plugin models. Construct one with
/// [`BuildInfo::new`] or the [`build_info!`](crate::build_info) macro.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BuildInfo {
    /// The time the plugin was built, as Unix milliseconds (`0` if unknown).
    #[serde(default)]
    pub time: i64,
    /// The plugin ID.
    #[serde(default, rename = "pluginID")]
    pub plugin_id: String,
    /// The plugin version.
    #[serde(default)]
    pub version: String,
}

impl BuildInfo {
    /// Create a [`BuildInfo`] with the given plugin ID and version, and an
    /// unknown (`0`) build time.
    pub fn new(plugin_id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            time: 0,
            plugin_id: plugin_id.into(),
            version: version.into(),
        }
    }

    /// Set the build time, as Unix milliseconds.
    #[must_use]
    pub fn with_time(mut self, time_ms: i64) -> Self {
        self.time = time_ms;
        self
    }
}

/// Capture [`BuildInfo`] from the calling crate's Cargo package metadata.
///
/// Expands to a [`BuildInfo`] whose `plugin_id` is `CARGO_PKG_NAME` and `version`
/// is `CARGO_PKG_VERSION` of the crate in which the macro is invoked. The build
/// time is left unset; set it with [`BuildInfo::with_time`] if you capture one in
/// a build script.
#[macro_export]
macro_rules! build_info {
    () => {
        $crate::buildinfo::BuildInfo::new(
            ::core::env!("CARGO_PKG_NAME"),
            ::core::env!("CARGO_PKG_VERSION"),
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_captures_this_crate_metadata() {
        let info = build_info!();
        assert_eq!(info.plugin_id, "grafana-plugin-sdk-rs");
        assert!(!info.version.is_empty());
        assert_eq!(info.time, 0);
    }

    #[test]
    fn serializes_with_grafana_field_names() {
        let json = serde_json::to_value(BuildInfo::new("my-app", "1.2.3").with_time(42)).unwrap();
        assert_eq!(json["pluginID"], "my-app");
        assert_eq!(json["version"], "1.2.3");
        assert_eq!(json["time"], 42);
    }
}
