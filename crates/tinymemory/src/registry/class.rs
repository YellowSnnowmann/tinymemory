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

/// Error returned when a driver class is not recognized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverClassParseError {
    /// The raw class value is unsupported.
    Unknown {
        /// The unrecognized input.
        raw: String,
    },
}

impl fmt::Display for DriverClassParseError {
    /// Renders the offending value.
    ///
    /// A config typo (`class = "embeded"`) is the only way to reach this, and
    /// the message is what the operator sees. Without the raw value it says
    /// only that *some* class was unrecognized, which does not point at the
    /// line to fix — and this error carries it already.
    ///
    /// The value comes from the host's own config file, not from a driver or
    /// the network, so echoing it discloses nothing the reader did not write.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::Unknown { raw } = self;
        write!(f, "unknown driver class: {raw}")
    }
}

impl std::error::Error for DriverClassParseError {}

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
    /// A loadable native module: a `cdylib` admitted through a module host's
    /// ABI, manifest and digest gates and reached over an in-process bus.
    ///
    /// Distinct from both neighbours, and the distinction decides host policy:
    ///
    /// - not [`Self::Embedded`], because the code is **not compiled into the
    ///   host binary**. Whether it is present is a runtime fact, so a capability
    ///   set derived from it can be empty on a platform no artifact targets.
    /// - not [`Self::External`], because there is **no egress and no process
    ///   boundary**. It shares the host's address space, privileges and crash
    ///   domain, so endpoint allowlisting and credential scoping are neither
    ///   applicable nor sufficient — what protects the host is admission, not
    ///   isolation.
    ///
    /// A host must therefore not apply egress redaction to a module driver (the
    /// content is not leaving the device) and must not treat it as a
    /// compile-time guarantee either.
    Module,
    /// A stub advertising zero optional capabilities — what a compiled-out or
    /// unconfigured memory subsystem binds to.
    Null,
}

impl DriverClass {
    /// Every class, in declaration order.
    pub const ALL: [DriverClass; 4] = [
        DriverClass::Embedded,
        DriverClass::External,
        DriverClass::Module,
        DriverClass::Null,
    ];

    /// Stable snake_case identifier used in config, on the wire, and in logs.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::External => "external",
            Self::Module => "module",
            Self::Null => "null",
        }
    }

    /// Parse back from the config / wire form.
    ///
    /// # Errors
    ///
    /// Returns the unrecognised input in the message, so a typo in a
    /// `class = …` line is self-explaining.
    pub fn parse(raw: &str) -> Result<Self, DriverClassParseError> {
        Self::ALL
            .iter()
            .copied()
            .find(|class| class.as_str() == raw)
            .ok_or_else(|| DriverClassParseError::Unknown {
                raw: raw.to_string(),
            })
    }
}

impl fmt::Display for DriverClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DriverClass {
    type Err = DriverClassParseError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::parse(raw)
    }
}
