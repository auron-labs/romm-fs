//! User-facing problem model: plain-language title and cause for the UI, with
//! the raw diagnostic text kept only for the "Details" disclosure.
//! Everything here is pure so it can be unit-tested on any platform.

use rommfs_core::error::Error;
use rommfs_core::events::{AuxiliaryWarning, AuxiliaryWarningKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserProblem {
    pub severity: Severity,
    /// Short statement of what did not work, e.g. "Couldn't finish RetroBat setup".
    pub title: String,
    /// One plain-language line explaining why and, where possible, what to do.
    pub cause: Option<String>,
    /// The raw text, only when it differs from what the user is shown.
    pub technical: Option<String>,
}

const NEEDS_ADMIN: &str = "Windows needs permission to finish this operation. Retry and approve the Windows permission prompt.";
const LINK_PRIVILEGE: &str = "Windows didn't allow RomMFS to create folder links. Retry Set up RetroBat and approve the Windows permission prompt.";
const ACCESS_DENIED: &str = "Windows denied access to a file or folder. Close RetroBat and its emulators, make sure you can change that folder, then retry.";
const FILE_IN_USE: &str =
    "A file is in use by another program. Close RetroBat and its emulators, then retry.";
const SIGN_IN: &str = "RomM rejected the sign-in or it has expired. Sign in again.";
const FORBIDDEN: &str =
    "RomM didn't allow this account to do that. Check the account's permissions on the server.";
const UNREACHABLE: &str =
    "Couldn't reach the RomM server. Check the server address and your network connection.";

impl UserProblem {
    pub fn error(
        title: impl Into<String>,
        cause: Option<String>,
        technical: Option<String>,
    ) -> Self {
        Self {
            severity: Severity::Error,
            title: title.into(),
            cause,
            technical,
        }
    }

    /// Build from raw error text (event strings, anyhow chains rendered with
    /// `{:#}`). Known technical shapes get a plain explanation; otherwise the
    /// text is shown with internal prefixes removed.
    pub fn from_text(title: impl Into<String>, raw: &str) -> Self {
        let raw = raw.trim();
        let cause = explain_text(raw);
        let technical = (cause != raw).then(|| raw.to_string());
        Self::error(title, Some(cause), technical)
    }

    /// Single-line form for compact lists: "Title: cause".
    pub fn line(&self) -> String {
        match &self.cause {
            Some(cause) => format!("{}: {cause}", self.title),
            None => self.title.clone(),
        }
    }

    pub fn from_core(title: impl Into<String>, error: &Error) -> Self {
        let raw = error.to_string();
        let cause = explain_core(error);
        let technical = (cause != raw).then_some(raw);
        Self::error(title, Some(cause), technical)
    }

    pub fn from_anyhow(title: impl Into<String>, error: &anyhow::Error) -> Self {
        let title = title.into();
        for cause in error.chain() {
            if let Some(core) = cause.downcast_ref::<Error>() {
                let mut problem = Self::from_core(title, core);
                problem.technical = Some(format!("{error:#}"));
                return problem;
            }
            if let Some(code) = cause
                .downcast_ref::<std::io::Error>()
                .and_then(|e| e.raw_os_error())
                .and_then(explain_os_code)
            {
                return Self::error(title, Some(code.into()), Some(format!("{error:#}")));
            }
        }
        Self::from_text(title, &format!("{error:#}"))
    }
}

impl From<&Error> for UserProblem {
    fn from(error: &Error) -> Self {
        Self::from_core("Something went wrong", error)
    }
}

fn explain_os_code(code: i32) -> Option<&'static str> {
    match code {
        740 => Some(NEEDS_ADMIN),
        1314 => Some(LINK_PRIVILEGE),
        5 | 13 => Some(ACCESS_DENIED),
        32 | 33 => Some(FILE_IN_USE),
        _ => None,
    }
}

/// "... (os error 740)" -> 740
fn os_error_code(text: &str) -> Option<i32> {
    let rest = &text[text.find("os error ")? + "os error ".len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn explain_core(error: &Error) -> String {
    match error {
        Error::Auth(_) => SIGN_IN.into(),
        Error::Forbidden(_) => FORBIDDEN.into(),
        Error::Http { status, .. } => match *status {
            404 => "RomM couldn't find what was requested. Refresh your library and try again."
                .into(),
            500..=599 => format!(
                "The RomM server reported a problem (HTTP {status}). Try again in a moment."
            ),
            _ => format!("RomM answered with an unexpected response (HTTP {status})."),
        },
        Error::Transport(_) => UNREACHABLE.into(),
        Error::Truncated { .. } | Error::SizeMismatch { .. } => {
            "A download ended early or didn't match what the server listed. Try again.".into()
        }
        Error::InvalidCatalogue(_) => {
            "RomM sent data RomMFS couldn't read. Check that the server is a supported RomM version.".into()
        }
        Error::Io(io) => io
            .raw_os_error()
            .and_then(explain_os_code)
            .map(String::from)
            .unwrap_or_else(|| match io.kind() {
                std::io::ErrorKind::PermissionDenied => ACCESS_DENIED.into(),
                std::io::ErrorKind::NotFound => "A file or folder couldn't be found.".into(),
                _ => "A file operation failed.".into(),
            }),
        Error::Unsupported(text) => sentence(text),
        Error::Cancelled => "Cancelled.".into(),
    }
}

/// Capitalize and end with a full stop.
fn sentence(text: &str) -> String {
    let text = text.trim();
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out: String = first.to_uppercase().chain(chars).collect();
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

fn explain_text(raw: &str) -> String {
    if let Some(text) = os_error_code(raw).and_then(explain_os_code) {
        return text.into();
    }
    let lower = raw.to_ascii_lowercase();
    if lower.contains("authentication failed:") {
        return SIGN_IN.into();
    }
    if lower.contains("transport error:") {
        return UNREACHABLE.into();
    }
    if lower.contains("server error 5") {
        return "The RomM server reported a problem. Try again in a moment.".into();
    }
    if lower.starts_with("permission denied:") {
        return FORBIDDEN.into();
    }
    if lower.contains("access is denied") {
        return ACCESS_DENIED.into();
    }
    // Strip internal prefixes, including inside an anyhow chain.
    let mut text = raw.to_string();
    for prefix in ["io error: ", "unsupported: ", "invalid catalogue payload: "] {
        text = text.replace(prefix, "");
    }
    sentence(&text)
}

/// Plain-language line for one support-file note.
pub fn describe_auxiliary(warning: &AuxiliaryWarning) -> String {
    match warning.kind {
        AuxiliaryWarningKind::MissingOnServer => format!(
            "{}: missing from your RomM server \u{2014} rescan firmware in RomM",
            warning.subject
        ),
        AuxiliaryWarningKind::SwitchArchive => {
            "Switch keys/firmware: upload unpacked prod.keys/title.keys or .nca files to RomM, not an archive".into()
        }
        AuxiliaryWarningKind::Duplicate => format!(
            "{}: the same file exists more than once on the server; it was installed once",
            warning.subject
        ),
        AuxiliaryWarningKind::Other => {
            let text = warning.message.replace("unsupported: ", "");
            format!("{}: {}", warning.subject, text.trim())
        }
    }
}

/// Headline and optional note for a finished support-file install. Duplicate
/// notes are informational and not counted as issues.
pub fn auxiliary_headline(
    published: usize,
    unchanged: usize,
    warnings: &[AuxiliaryWarning],
) -> (String, Option<String>) {
    let installed = published + unchanged;
    let issues = warnings.iter().filter(|w| w.is_issue()).count();
    let noun = |n: usize| {
        if n == 1 {
            "support file"
        } else {
            "support files"
        }
    };
    let headline = if installed == 0 && issues == 0 {
        "No support files to install".to_string()
    } else {
        format!("{installed} {} installed", noun(installed))
    };
    let note = (issues > 0).then(|| {
        if issues == 1 {
            "1 needs attention".to_string()
        } else {
            format!("{issues} need attention")
        }
    });
    (headline, note)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warning(kind: AuxiliaryWarningKind, subject: &str) -> AuxiliaryWarning {
        AuxiliaryWarning {
            kind,
            subject: subject.into(),
            message: format!("{subject} raw"),
        }
    }

    #[test]
    fn os_errors_are_explained_and_raw_text_is_kept_as_technical() {
        let p = UserProblem::from_text(
            "Couldn't launch the installer",
            "io error: The requested operation requires elevation. (os error 740)",
        );
        assert_eq!(p.cause.as_deref(), Some(NEEDS_ADMIN));
        assert!(p.technical.as_deref().unwrap().contains("os error 740"));
        let p = UserProblem::from_text("t", "create link: A required privilege (os error 1314)");
        assert!(p.cause.unwrap().contains("Windows permission prompt"));
        assert!(UserProblem::from_text("t", "x (os error 5)")
            .cause
            .unwrap()
            .contains("denied access"));
        assert!(UserProblem::from_text("t", "x (os error 32)")
            .cause
            .unwrap()
            .contains("Close RetroBat"));
        assert_eq!(os_error_code("failed (os error 1314)."), Some(1314));
    }

    #[test]
    fn core_errors_map_to_plain_language() {
        let p = UserProblem::from_core("t", &Error::Auth("bad token".into()));
        assert_eq!(p.cause.as_deref(), Some(SIGN_IN));
        assert_eq!(
            p.technical.as_deref(),
            Some("authentication failed: bad token")
        );
        assert_eq!(
            UserProblem::from_core("t", &Error::Forbidden("scope".into()))
                .cause
                .as_deref(),
            Some(FORBIDDEN)
        );
        assert_eq!(
            UserProblem::from_core("t", &Error::Transport("dns".into()))
                .cause
                .as_deref(),
            Some(UNREACHABLE)
        );
        let http = UserProblem::from_core(
            "t",
            &Error::Http {
                status: 502,
                message: "bad gateway".into(),
            },
        );
        assert!(http.cause.unwrap().contains("HTTP 502"));
        let io = UserProblem::from_core("t", &Error::Io(std::io::Error::from_raw_os_error(740)));
        assert_eq!(io.cause.as_deref(), Some(NEEDS_ADMIN));
        let unsupported = UserProblem::from_core(
            "t",
            &Error::Unsupported("select a RetroBat installation".into()),
        );
        assert_eq!(
            unsupported.cause.as_deref(),
            Some("Select a RetroBat installation.")
        );
    }

    #[test]
    fn anyhow_chain_keeps_the_whole_chain_as_technical() {
        let error = anyhow::Error::new(Error::Transport("timeout".into())).context("download");
        let p = UserProblem::from_anyhow("Couldn't download", &error);
        assert_eq!(p.cause.as_deref(), Some(UNREACHABLE));
        assert_eq!(
            p.technical.as_deref(),
            Some("download: transport error: timeout")
        );
    }

    #[test]
    fn plain_text_has_no_technical_lane_and_loses_internal_prefixes() {
        let p = UserProblem::from_text(
            "t",
            "Select and set up a fresh RetroBat installation first.",
        );
        assert!(p.technical.is_none());
        let p = UserProblem::from_text("t", "unsupported: stop the games folder");
        assert_eq!(p.cause.as_deref(), Some("Stop the games folder."));
        assert!(p.technical.is_some());
    }

    #[test]
    fn auxiliary_notes_are_plain_and_duplicates_are_not_issues() {
        let missing = warning(AuxiliaryWarningKind::MissingOnServer, "psx.bin");
        assert_eq!(
            describe_auxiliary(&missing),
            "psx.bin: missing from your RomM server \u{2014} rescan firmware in RomM"
        );
        assert!(
            describe_auxiliary(&warning(AuxiliaryWarningKind::SwitchArchive, "f.zip"))
                .starts_with("Switch keys/firmware: upload unpacked")
        );
        let notes = [
            missing,
            warning(AuxiliaryWarningKind::SwitchArchive, "f.zip"),
            warning(AuxiliaryWarningKind::Duplicate, "a.bin"),
        ];
        let (headline, note) = auxiliary_headline(140, 6, &notes);
        assert_eq!(headline, "146 support files installed");
        assert_eq!(note.as_deref(), Some("2 need attention"));
        assert_eq!(auxiliary_headline(1, 0, &notes[2..]).1, None);
    }
}
