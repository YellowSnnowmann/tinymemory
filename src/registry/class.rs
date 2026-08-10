//! [`DriverClass`] — how a bound driver is reached.
//!
//! Class is a fact about how the *host* bound a driver, recorded in host
//! configuration. It is deliberately absent from
//! [`MemoryProvider`](tinymemory_api::provider::MemoryProvider): a driver that
//! self-reported its class could let a misconfigured external backend claim to
//! be embedded and skip the trust checks class gates.
//!
//! A host that runs several pluggable subsystems will have its own generic
//! class enum shared across them. This one is shaped identically (three
//! variants, the same snake_case spellings) so the boundary conversion is a
//! total three-arm `match` that cannot drift.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// How a bound driver is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverClass {
    /// An in-tree / vendored Rust crate. The default: no network, no extra
    /// process.
    Embedded,
    /// An out-of-process backend reached through a transport adapter over a
    /// documented wire contract.
    External,
    /// A stub advertising zero optional capabilities — what a compiled-out or
    /// unconfigured memory subsystem binds to.
    Null,
}

impl DriverClass {
    /// Every class, in declaration order.
    pub const ALL: [DriverClass; 3] = [
        DriverClass::Embedded,
        DriverClass::External,
        DriverClass::Null,
    ];

    /// Stable snake_case identifier used in config, on the wire, and in logs.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::External => "external",
            Self::Null => "null",
        }
    }

    /// Parse back from the config / wire form.
    ///
    /// # Errors
    ///
    /// Returns the unrecognised input in the message, so a typo in a
    /// `class = …` line is self-explaining.
    pub fn parse(raw: &str) -> Result<Self, String> {
        Self::ALL
            .iter()
            .copied()
            .find(|class| class.as_str() == raw)
            .ok_or_else(|| format!("unknown driver class: {raw}"))
    }
}

impl fmt::Display for DriverClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DriverClass {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::parse(raw)
    }
}
