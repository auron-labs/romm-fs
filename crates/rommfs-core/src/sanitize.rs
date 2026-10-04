//! Remote path component validation + Windows-visible-name rules (PRD R2).
//!
//! Remote-supplied names are untrusted: reject traversal, absolute paths, and
//! empty/multi-component strings; map Windows-invalid characters, reserved
//! device names, trailing dots/spaces, and case-insensitive collisions to
//! deterministic safe names while preserving the file extension.

/// A remote path component that is valid for local use, or the reason not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentName {
    /// The name was usable unchanged.
    Clean(String),
    /// The name needed a deterministic adjustment; carries the safe name.
    Adjusted(String),
}

impl ComponentName {
    pub fn name(&self) -> &str {
        match self {
            ComponentName::Clean(n) | ComponentName::Adjusted(n) => n,
        }
    }
}

/// Validate and normalize one remote filename.
/// Returns `None` for names that must be rejected outright (traversal,
/// absolute, separators, empties, control chars).
pub fn sanitize_component(remote: &str) -> Option<ComponentName> {
    let _ = remote;
    todo!("reject ../, /, \\, drive prefixes, NUL/ctrl, then map invalid chars <>:\"|?*, trailing dot/space, reserved device names CON/PRN/AUX/NUL/COM1-9/LPT1-9")
}

/// Resolve a case-insensitive collision within one directory: returns a
/// deterministic distinct visible name preserving the extension
/// (`name (2).ext`, `name (3).ext`, ...) that is not in `taken_lower`.
pub fn disambiguate(desired: &str, taken_lower: &dyn Fn(&str) -> bool) -> String {
    let _ = (desired, taken_lower);
    todo!()
}
