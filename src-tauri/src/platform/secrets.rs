use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

const UNAVAILABLE: &str = "Secure storage is unavailable. Install libsecret's secret-tool and unlock your session keyring, or use session-only credentials. No plaintext secret was saved.";
static KEY: Mutex<Option<[u8; 32]>> = Mutex::new(None);

fn key() -> Result<[u8; 32], String> {
    let mut cached = KEY.lock().map_err(|_| UNAVAILABLE)?;
    if let Some(key) = *cached {
        return Ok(key);
    }
    if !super::secure_storage_supported() {
        return Err(UNAVAILABLE.into());
    }
    let root = super::data_dir()?;
    super::private_directory(&root)?;
    let namespace = format!(
        "{:x}",
        Sha256::digest(
            root.canonicalize()
                .map_err(|_| UNAVAILABLE)?
                .as_os_str()
                .as_encoded_bytes()
        )
    );
    let lock = super::private_options()
        .create(true)
        .truncate(false)
        .open(root.join("keyring.lock"))
        .map_err(|_| UNAVAILABLE)?;
    fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| "Secure storage is busy. Try again.")?;
    let found = run(
        &[
            "lookup",
            "application",
            "com.fxserver.installer",
            "purpose",
            "encryption-key-v1",
            "storage",
            &namespace,
        ],
        None,
    )?;
    let value = if found.0 {
        found.1.trim().to_string()
    } else {
        if root
            .join("keyring.initialized")
            .try_exists()
            .map_err(|_| UNAVAILABLE)?
        {
            return Err(UNAVAILABLE.into());
        }
        if !found.1.trim().is_empty() {
            return Err(UNAVAILABLE.into());
        }
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| UNAVAILABLE)?;
        let value: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let saved = run(
            &[
                "store",
                "--label=FXServer Installer encryption key",
                "application",
                "com.fxserver.installer",
                "purpose",
                "encryption-key-v1",
                "storage",
                &namespace,
            ],
            Some(&value),
        )?;
        if !saved.0 {
            return Err(UNAVAILABLE.into());
        }
        value
    };
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(UNAVAILABLE.into());
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(|_| UNAVAILABLE)?;
    }
    match super::private_options()
        .create_new(true)
        .open(root.join("keyring.initialized"))
    {
        Ok(mut marker) => marker
            .write_all(b"1")
            .and_then(|_| marker.sync_all())
            .map_err(|_| UNAVAILABLE)?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(UNAVAILABLE.into()),
    }
    *cached = Some(key);
    Ok(key)
}

fn run(args: &[&str], input: Option<&str>) -> Result<(bool, String), String> {
    let mut child = Command::new("/usr/bin/secret-tool")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| UNAVAILABLE)?;
    if let Some(mut stdin) = child.stdin.take() {
        if let Some(input) = input {
            if writeln!(stdin, "{input}").is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(UNAVAILABLE.into());
            }
        }
    }
    let stdout = child.stdout.take().ok_or(UNAVAILABLE)?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(4097).read_to_end(&mut bytes).map(|_| bytes);
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(UNAVAILABLE.into());
            }
        }
    };
    let bytes = rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| UNAVAILABLE)?
        .map_err(|_| UNAVAILABLE)?;
    if bytes.len() > 4096 {
        return Err(UNAVAILABLE.into());
    }
    Ok((
        status.success(),
        String::from_utf8(bytes).map_err(|_| UNAVAILABLE)?,
    ))
}

fn seal(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| UNAVAILABLE)?;
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|_| UNAVAILABLE)?;
    let mut bytes = b"FXI1".to_vec();
    bytes.extend_from_slice(&nonce);
    bytes.extend(
        cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .map_err(|_| "Secret encryption failed.")?,
    );
    Ok(bytes)
}

fn open(key: &[u8; 32], bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 32 || !bytes.starts_with(b"FXI1") {
        return Err("Unrecognized encrypted data.".into());
    }
    Aes256Gcm::new_from_slice(key)
        .map_err(|_| UNAVAILABLE)?
        .decrypt(Nonce::from_slice(&bytes[4..16]), &bytes[16..])
        .map_err(|_| "Encrypted data is damaged or belongs to another keyring.".into())
}

pub(crate) fn encrypt_secret(bytes: &[u8]) -> Result<Vec<u8>, String> {
    seal(&key()?, bytes)
}
pub(crate) fn decrypt_secret(bytes: &[u8]) -> Result<Vec<u8>, String> {
    open(&key()?, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an isolated CI D-Bus session and disposable GNOME keyring"]
    fn native_keyring_persists_across_processes_and_fails_closed() {
        use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        assert_eq!(
            std::env::var("RUNNER_ENVIRONMENT").as_deref(),
            Ok("github-hosted")
        );
        let sandbox = PathBuf::from(std::env::var_os("FXI_INTEGRATION_ROOT").unwrap())
            .canonicalize()
            .unwrap();
        assert_eq!(sandbox.parent(), Some(std::path::Path::new("/tmp")));
        assert!(sandbox
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("fxi-native-"));
        let root = super::super::data_dir().unwrap();
        assert_eq!(root, sandbox.join("data/com.fxserver.installer"));
        let fixture = sandbox.join("sealed.bin");
        let plaintext = b"public test fixture, not a real credential";
        match std::env::var("FXI_KEYRING_MODE").unwrap().as_str() {
            "write" => {
                let sealed = encrypt_secret(plaintext).unwrap();
                assert!(!sealed
                    .windows(plaintext.len())
                    .any(|bytes| bytes == plaintext));
                let mut file = super::super::private_options()
                    .create_new(true)
                    .open(&fixture)
                    .unwrap();
                file.write_all(&sealed).unwrap();
                assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
                assert_eq!(
                    fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
            "read" => {
                assert_eq!(
                    decrypt_secret(&fs::read(&fixture).unwrap()).unwrap(),
                    plaintext
                );
            }
            "missing" => {
                let before = fs::read(&fixture).unwrap();
                assert!(decrypt_secret(&before).is_err());
                assert!(encrypt_secret(plaintext).is_err());
                assert_eq!(fs::read(&fixture).unwrap(), before);
                assert!(root.join("keyring.initialized").is_file());
            }
            other => panic!("Unexpected integration mode {other}"),
        }
    }

    #[test]
    fn encrypted_data_is_authenticated_and_uses_fresh_nonces() {
        let key = [42u8; 32];
        let first = seal(&key, b"private fixture").unwrap();
        assert_ne!(first, seal(&key, b"private fixture").unwrap());
        assert_eq!(open(&key, &first).unwrap(), b"private fixture");
        assert!(open(&[43u8; 32], &first).is_err());
        let mut altered = first;
        altered[20] ^= 1;
        assert!(open(&key, &altered).is_err());
        assert!(open(&key, b"plaintext").is_err());
    }
}
