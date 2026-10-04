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

/// Characters Windows forbids in file names; each maps to `_`.
const INVALID_CHARS: &[char] = &['<', '>', ':', '"', '|', '?', '*'];

/// DOS device names that must never appear as a file stem (case-insensitive).
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "COM¹", "COM²", "COM³", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
    "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// Validate and normalize one remote filename.
/// Returns `None` for names that must be rejected outright (traversal,
/// absolute, separators, empties, control chars).
pub fn sanitize_component(remote: &str) -> Option<ComponentName> {
    let s = remote;
    if s.is_empty() || s.trim().is_empty() || s == "." || s == ".." {
        return None;
    }
    // Path separators make this a multi-component string (traversal risk).
    // A drive prefix ("C:") needs no special case: the ':' maps to '_' below,
    // and any real absolute/UNC path contains a separator anyway.
    if s.contains('/') || s.contains('\\') {
        return None;
    }
    // Control characters (incl. NUL and DEL) are never usable.
    if s.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}') {
        return None;
    }

    let mut adjusted = false;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if INVALID_CHARS.contains(&c) {
            out.push('_');
            adjusted = true;
        } else {
            out.push(c);
        }
    }

    // Windows silently strips trailing dots/spaces — do it deterministically.
    let stripped = out.trim_end_matches(['.', ' ']);
    if stripped.len() != out.len() {
        adjusted = true;
    }
    let mut out = stripped.to_string();

    // Reserved DOS device names apply to the stem (before the first '.');
    // Windows ignores trailing dots/spaces on the stem for this check.
    let stem_end = out.find('.').unwrap_or(out.len());
    let stem = out[..stem_end].trim_end_matches(['.', ' ']);
    if RESERVED_NAMES.contains(&stem.to_uppercase().as_str()) {
        out.insert(stem_end, '_');
        adjusted = true;
    }

    // Nothing usable left (e.g. the input was only dots/spaces/invalid chars
    // that normalized away).
    if out.is_empty() || out.trim().is_empty() {
        return None;
    }

    Some(if adjusted {
        ComponentName::Adjusted(out)
    } else {
        ComponentName::Clean(out)
    })
}

/// Resolve a case-insensitive collision within one directory: returns a
/// deterministic distinct visible name preserving the extension
/// (`name (2).ext`, `name (3).ext`, ...) that is not in `taken_lower`.
pub fn disambiguate(desired: &str, taken_lower: &dyn Fn(&str) -> bool) -> String {
    if !taken_lower(&desired.to_lowercase()) {
        return desired.to_string();
    }
    // Preserve the final extension; a leading dot is a stem, not an ext.
    let (stem, ext) = match desired.rfind('.') {
        Some(i) if i > 0 => (&desired[..i], &desired[i..]),
        _ => (desired, ""),
    };
    for n in 2u32.. {
        let candidate = format!("{stem} ({n}){ext}");
        if !taken_lower(&candidate.to_lowercase()) {
            return candidate;
        }
    }
    unreachable!()
}
