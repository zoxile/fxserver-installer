#[cfg(windows)]
use std::fs::File;
use std::path::{Path, PathBuf};

#[must_use = "Keep the credential file alive until the MariaDB child has exited"]
pub(crate) struct CredentialFile {
    path: PathBuf,
    #[cfg(windows)]
    file: Option<File>,
    #[cfg(windows)]
    _directories: Vec<File>,
}

impl CredentialFile {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(windows)]
    pub(super) fn create(contents: &str) -> Result<Self, String> {
        use crate::commands::backup_manager::storage::{pin_directories, secure_token};
        use std::io::Write;
        let directory = std::env::temp_dir();
        let directories = pin_directories(&directory).map_err(|error| {
            format!("Cannot secure the local client credential directory: {error}")
        })?;
        let path = directory.join(format!("fxi-client-{}.cnf", secure_token()?));
        let file = create_private_file(&path)?;
        let mut guard = Self {
            path,
            file: Some(file),
            _directories: directories,
        };
        // The protected DACL is installed atomically by CREATE_NEW, before any secret bytes.
        let file = guard.file.as_mut().expect("new credential file");
        file.write_all(contents.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot write the protected client credential file.")?;
        Ok(guard)
    }

    #[cfg(not(windows))]
    pub(super) fn create(_: &str) -> Result<Self, String> {
        Err("Protected MariaDB credential files currently require Windows.".into())
    }
}

#[cfg(windows)]
impl Drop for CredentialFile {
    fn drop(&mut self) {
        // MariaDB's CRT reader does not share DELETE access. Release our file lock only
        // after child exit, then verify identity on the handle used for deletion.
        let Some(file) = self.file.take() else { return };
        let identity = file_identity(&file);
        drop(file);
        if identity
            .and_then(|identity| remove_matching_file(&self.path, identity))
            .is_err()
        {
            log::error!("Could not remove the protected MariaDB credential file.");
        }
    }
}

#[cfg(windows)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    volume: u32,
    high: u32,
    low: u32,
}

#[cfg(windows)]
fn file_identity(file: &File) -> Result<FileIdentity, String> {
    use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect credential file.")?;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
        return Err("Credential file must be a regular, unlinked file.".into());
    }
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err("Cannot identify credential file.".into());
    }
    Ok(FileIdentity {
        volume: info.dwVolumeSerialNumber,
        high: info.nFileIndexHigh,
        low: info.nFileIndexLow,
    })
}

#[cfg(windows)]
fn remove_matching_file(path: &Path, expected: FileIdentity) -> Result<(), String> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FileDispositionInfo, SetFileInformationByHandle, DELETE, FILE_DISPOSITION_INFO,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
    };
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES | DELETE)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| "Cannot lock credential file for cleanup.")?;
    if file_identity(&file)? != expected {
        return Err("Credential file identity changed; cleanup refused.".into());
    }
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            &disposition as *const _ as *const _,
            std::mem::size_of_val(&disposition) as u32,
        )
    } == 0
    {
        return Err("Cannot remove verified credential file.".into());
    }
    Ok(())
}

#[cfg(windows)]
fn current_user_sid() -> Result<String, String> {
    use std::{
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
            TOKEN_USER,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err("Cannot identify the Windows user for client credentials.".into());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut size = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut size,
        );
    }
    if size == 0 {
        return Err("Cannot inspect the Windows user token.".into());
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } == 0
    {
        return Err("Cannot inspect the Windows user token.".into());
    }
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } == 0 {
        return Err("Cannot encode the Windows user SID.".into());
    }
    let mut length = 0;
    unsafe {
        while *sid.add(length) != 0 {
            length += 1;
        }
    }
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(sid, length) });
    unsafe {
        LocalFree(sid.cast());
    }
    result.map_err(|_| "Invalid Windows user SID.".into())
}

#[cfg(windows)]
fn create_private_file(path: &Path) -> Result<File, String> {
    use std::{
        os::windows::{ffi::OsStrExt, io::FromRawHandle},
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::{LocalFree, INVALID_HANDLE_VALUE},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SECURITY_ATTRIBUTES,
        },
        Storage::FileSystem::{
            CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_TEMPORARY, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ,
        },
    };
    let sddl: Vec<u16> = format!("D:P(A;;FA;;;{})", current_user_sid()?)
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err("Cannot construct the client credential ACL.".into());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            FILE_SHARE_READ,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    unsafe {
        LocalFree(descriptor.cast());
    }
    if handle == INVALID_HANDLE_VALUE {
        return Err("Cannot create a new, private client credential file.".into());
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{ffi::c_void, os::windows::io::AsRawHandle, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW,
                GetSecurityInfo, SE_FILE_OBJECT,
            },
            EqualSid, GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
            ACCESS_ALLOWED_ACE, DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
    };

    struct LocalBuffer(*mut c_void);

    impl Drop for LocalBuffer {
        fn drop(&mut self) {
            unsafe { LocalFree(self.0) };
        }
    }

    fn file_descriptor(file: &File) -> LocalBuffer {
        let mut descriptor = ptr::null_mut();
        assert_eq!(
            unsafe {
                GetSecurityInfo(
                    file.as_raw_handle(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut descriptor,
                )
            },
            0
        );
        LocalBuffer(descriptor)
    }

    fn descriptor_from_sddl(sddl: &str) -> LocalBuffer {
        let sddl: Vec<_> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut descriptor = ptr::null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    ptr::null_mut(),
                )
            },
            0
        );
        LocalBuffer(descriptor)
    }

    fn sid_from_string(sid: &str) -> LocalBuffer {
        let sid: Vec<_> = sid.encode_utf16().chain(Some(0)).collect();
        let mut result = ptr::null_mut();
        assert_ne!(
            unsafe { ConvertStringSidToSidW(sid.as_ptr(), &mut result) },
            0
        );
        LocalBuffer(result)
    }

    fn has_private_dacl(descriptor: &LocalBuffer, expected_sid: &LocalBuffer) -> bool {
        let (mut control, mut revision) = (0, 0);
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) },
            0
        );
        let (mut present, mut defaulted) = (0, 0);
        let mut acl = ptr::null_mut();
        assert_ne!(
            unsafe {
                GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted)
            },
            0
        );
        if control & SE_DACL_PROTECTED == 0 || present == 0 || defaulted != 0 || acl.is_null() {
            return false;
        }
        if unsafe { (*acl).AceCount } != 1 {
            return false;
        }
        let mut ace = ptr::null_mut();
        assert_ne!(unsafe { GetAce(acl, 0, &mut ace) }, 0);
        let entry = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        // ACCESS_ALLOWED_ACE_TYPE is zero. Compare the SID itself, not its SDDL alias.
        entry.Header.AceType == 0
            && entry.Header.AceFlags == 0
            && entry.Mask == FILE_ALL_ACCESS
            && unsafe {
                EqualSid(
                    ptr::addr_of!(entry.SidStart).cast_mut().cast(),
                    expected_sid.0,
                )
            } != 0
    }

    #[test]
    fn credential_acl_is_private_before_secret_writes_and_guard_cleans_up() {
        let guard = CredentialFile::create("").unwrap();
        let path = guard.path.clone();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        assert!(
            has_private_dacl(
                &file_descriptor(guard.file.as_ref().unwrap()),
                &sid_from_string(&current_user_sid().unwrap()),
            ),
            "credential ACL must allow only the current user"
        );
        assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn private_dacl_check_accepts_windows_sid_aliases() {
        for (alias, sid) in [("SY", "S-1-5-18"), ("BA", "S-1-5-32-544"), ("LA", "LA")] {
            assert!(has_private_dacl(
                &descriptor_from_sddl(&format!("D:P(A;;FA;;;{alias})")),
                &sid_from_string(sid),
            ));
        }
    }

    #[test]
    fn private_dacl_check_rejects_changed_identity_or_permissions() {
        let sid = sid_from_string("S-1-5-21-1-2-3-500");
        for sddl in [
            "D:P(A;;FA;;;S-1-5-21-1-2-4-500)",
            "D:P(A;;FA;;;S-1-5-21-1-2-3-500)(A;;FR;;;WD)",
            "D:(A;;FA;;;S-1-5-21-1-2-3-500)",
            "D:P(A;ID;FA;;;S-1-5-21-1-2-3-500)",
            "D:P(A;;FR;;;S-1-5-21-1-2-3-500)",
            "D:P(D;;FA;;;S-1-5-21-1-2-3-500)",
            "D:P",
            "D:NO_ACCESS_CONTROL",
        ] {
            assert!(
                !has_private_dacl(&descriptor_from_sddl(sddl), &sid),
                "{sddl}"
            );
        }
    }

    #[test]
    fn cleanup_preserves_a_different_file_identity() {
        let first = CredentialFile::create("first-fixture").unwrap();
        let mut second = CredentialFile::create("second-fixture").unwrap();
        let first_id = file_identity(first.file.as_ref().unwrap()).unwrap();
        let second_id = file_identity(second.file.as_ref().unwrap()).unwrap();
        drop(second.file.take());
        assert!(remove_matching_file(second.path(), first_id)
            .unwrap_err()
            .contains("identity"));
        assert_eq!(
            std::fs::read_to_string(second.path()).unwrap(),
            "second-fixture"
        );
        remove_matching_file(second.path(), second_id).unwrap();
        assert!(!second.path().exists());
    }

    #[test]
    fn private_creation_never_overwrites_an_existing_file() {
        let guard = CredentialFile::create("fixture").unwrap();
        assert!(create_private_file(guard.path()).is_err());
        assert_eq!(std::fs::read_to_string(guard.path()).unwrap(), "fixture");
    }
}
