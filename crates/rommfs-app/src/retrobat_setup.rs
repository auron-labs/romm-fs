//! Local setup shared by the controller and the short-lived Windows UAC helper.
#[cfg(any(windows, test))]
use anyhow::Context;
use std::path::Path;

#[cfg(windows)]
pub mod windows;

pub(crate) fn setup(
    installation: &Path,
    managed_root: &Path,
    server: &str,
) -> anyhow::Result<SetupReport> {
    anyhow::ensure!(
        installation.is_absolute() && managed_root.is_absolute(),
        "choose absolute RetroBat and managed ROM folder paths"
    );
    crate::controller::ensure_frontend_stopped(installation)?;
    if managed_root.exists() {
        rommfs_cfapi::claim_mount_root(managed_root, server)?;
    } else {
        crate::controller::check_mount_root(managed_root, server)?;
        crate::controller::claim_mount_root(managed_root, server)?;
    }
    let outcome = rommfs_core::retrobat::setup_roms_link_with_root_guard(
        installation,
        managed_root,
        server,
        |root| rommfs_cfapi::hold_mount_root(root, server),
    )?;
    let mut messages = vec![outcome.summary()];
    messages.push(configure_switch(installation));
    Ok(SetupReport {
        messages,
        skipped: outcome.skipped,
    })
}

/// What a local setup run did: the messages to show, and the systems it could
/// not link, each with its reason (WS6.10).
pub(crate) struct SetupReport {
    pub messages: Vec<String>,
    pub skipped: Vec<rommfs_core::retrobat::SkippedSystem>,
}

/// Switch save settings are optional: a problem is reported, never a reason
/// to undo or block linking the games.
fn configure_switch(installation: &Path) -> String {
    match rommfs_core::retrobat::configure_switch_saves(installation) {
        Ok(notes) if notes.is_empty() => "Switch saves configured".into(),
        Ok(notes) => format!("Switch saves configured ({})", notes.join("; ")),
        Err(error) => format!("Switch saves not configured: {error:#}"),
    }
}

/// Read-only check after the elevated helper: is the Switch setting in place?
#[cfg(windows)]
pub(crate) fn switch_status(installation: &Path) -> String {
    let configured = (|| -> anyhow::Result<bool> {
        let (home, _) = rommfs_core::retrobat::launcher_paths(installation)?;
        let current = std::fs::read_to_string(home.join("es_settings.cfg"))?;
        Ok(rommfs_core::retrobat::switch_save_setting(&current)? == current)
    })();
    match configured {
        Ok(true) => "Switch saves configured".into(),
        Ok(false) => "Switch saves not configured".into(),
        Err(error) => format!("Switch saves not configured: {error:#}"),
    }
}

/// Retry only Windows permission failures, after the first attempt's guards drop.
/// The helper executes once and never recursively requests elevation.
#[cfg(any(windows, test))]
pub(crate) fn retry_elevated<T>(
    result: anyhow::Result<T>,
    elevate: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    match result {
        Err(error)
            if error.chain().any(|cause| {
                let io = cause
                    .downcast_ref::<std::io::Error>()
                    .or_else(|| match cause.downcast_ref::<rommfs_core::Error>() {
                        Some(rommfs_core::Error::Io(io)) => Some(io),
                        _ => None,
                    });
                io.is_some_and(|io| {
                    matches!(io.raw_os_error(), Some(5 | 740 | 1314))
                        || (io.raw_os_error().is_none()
                            && io.kind() == std::io::ErrorKind::PermissionDenied)
                })
            }) =>
        {
            elevate().context("finish RetroBat setup with Windows permission")
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_errors_retry_once_including_wrapped_core_errors() {
        for code in [5, 740, 1314] {
            let error = anyhow::Error::new(rommfs_core::Error::Io(
                std::io::Error::from_raw_os_error(code),
            ))
            .context("create link");
            let mut calls = 0;
            let result = retry_elevated::<()>(Err(error), || {
                calls += 1;
                Err(std::io::Error::from_raw_os_error(code).into())
            });
            assert!(result.is_err());
            assert_eq!(calls, 1);
        }
    }

    #[test]
    fn success_busy_files_and_ownership_errors_do_not_elevate() {
        assert_eq!(
            retry_elevated(Ok(7), || panic!("already succeeded")).unwrap(),
            7
        );
        for code in [32, 33, 1223] {
            assert!(retry_elevated::<()>(
                Err(std::io::Error::from_raw_os_error(code).into()),
                || panic!("busy files and cancellation must not elevate")
            )
            .is_err());
        }
        assert!(
            retry_elevated::<()>(Err(anyhow::anyhow!("foreign files")), || panic!(
                "ownership rejection must not elevate"
            ))
            .is_err()
        );
    }
}
