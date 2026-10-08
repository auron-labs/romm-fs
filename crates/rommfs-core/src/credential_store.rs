//! Windows Credential Manager. There is deliberately no plaintext fallback.
use crate::{romm::PairedSession, Error, Result};
use sha2::{Digest, Sha256};

fn target(server: &str, account_id: i64) -> String {
    format!(
        "RomMFS/{:x}/{account_id}",
        Sha256::digest(server.as_bytes())
    )
}

pub fn save(server: &str, account_id: i64, session: &PairedSession) -> Result<()> {
    let bytes = serde_json::to_vec(session)
        .map_err(|_| Error::Auth("could not serialize secure session".into()))?;
    imp::save(&target(server, account_id), &bytes)
}
pub fn load(server: &str, account_id: i64) -> Result<Option<PairedSession>> {
    imp::load(&target(server, account_id))?
        .map(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|_| Error::Auth("stored secure session is invalid; sign in again".into()))
        })
        .transpose()
}
pub fn delete(server: &str, account_id: i64) -> Result<()> {
    imp::delete(&target(server, account_id))
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn save(_: &str, _: &[u8]) -> Result<()> {
        Err(Error::Unsupported(
            "Remembered sign-in requires Windows Credential Manager".into(),
        ))
    }
    pub fn load(_: &str) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    pub fn delete(_: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::{
        Foundation::ERROR_NOT_FOUND,
        Security::Credentials::{
            CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
            CRED_TYPE_GENERIC,
        },
    };
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    pub fn save(name: &str, bytes: &[u8]) -> Result<()> {
        let mut name = wide(name);
        let mut bytes = bytes.to_vec();
        // SAFETY: all pointer fields refer to buffers alive through CredWriteW;
        // other fields are zero defaults as required for a generic credential.
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: name.as_mut_ptr(),
            CredentialBlobSize: bytes
                .len()
                .try_into()
                .map_err(|_| Error::Auth("secure session too large".into()))?,
            CredentialBlob: bytes.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..unsafe { std::mem::zeroed() }
        };
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    pub fn load(name: &str) -> Result<Option<Vec<u8>>> {
        let name = wide(name);
        let mut credential = std::ptr::null_mut();
        // SAFETY: terminated target and valid output pointer; successful output
        // is allocated by Windows and freed exactly once below.
        if unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(ERROR_NOT_FOUND as i32) {
                return Ok(None);
            }
            return Err(e.into());
        }
        let bytes = unsafe {
            let c = &*credential;
            let bytes = if c.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
            };
            CredFree(credential.cast());
            bytes
        };
        Ok(Some(bytes))
    }
    pub fn delete(name: &str) -> Result<()> {
        let name = wide(name);
        // SAFETY: target is a live, terminated UTF-16 buffer.
        if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(ERROR_NOT_FOUND as i32) {
                return Err(e.into());
            }
        }
        Ok(())
    }
}
