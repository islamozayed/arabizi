//! Self-updater speaking the same protocol as the Tauri updater the 0.9.x
//! releases shipped with: it reads `latest.json` from the GitHub release,
//! verifies the minisign signature against the same public key, and installs
//! the platform bundle. Keeping the format means the installed Tauri builds
//! pick up native releases, and native builds pick up the ones after.

use base64::Engine as _;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const ENDPOINT: &str = "https://github.com/islamozayed/arabizi/releases/latest/download/latest.json";

/// The minisign public key the releases are signed with (base64 of the
/// minisign `.pub` file, as stored in the old tauri.conf.json).
pub const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDk5MTkxRDBDOEEwRTE2NEMKUldSTUZnNktEQjBabWZBVkdEZnBVK2t1M1lHcStJYXp6TzVlV2RoekdMTjZ5ZDZMWXVQamZ0MnQK";

#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub url: String,
    /// Base64 of the `.sig` file.
    pub signature: String,
}

/// The `platforms` key for this build, as Tauri names them.
pub fn target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", _) => "darwin-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("windows", _) => "windows-x86_64",
        (_, "aarch64") => "linux-aarch64",
        _ => "linux-x86_64",
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .user_agent(concat!("franconvert/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Returns a newer release for this platform, `None` when up to date.
pub fn check(current: &str) -> Result<Option<Release>, String> {
    let body = agent()
        .get(ENDPOINT)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    parse_latest(&body, current, target())
}

pub fn parse_latest(json: &str, current: &str, target: &str) -> Result<Option<Release>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let version = v.get("version").and_then(|s| s.as_str()).ok_or("no version")?;
    let latest = semver::Version::parse(version.trim_start_matches('v')).map_err(|e| e.to_string())?;
    let cur = semver::Version::parse(current).map_err(|e| e.to_string())?;
    if latest <= cur {
        return Ok(None);
    }
    let p = v.get("platforms").and_then(|p| p.get(target)).ok_or("no build for this platform")?;
    let get = |k: &str| p.get(k).and_then(|s| s.as_str()).map(str::to_string).ok_or(format!("no {k}"));
    Ok(Some(Release { version: version.trim_start_matches('v').to_string(), url: get("url")?, signature: get("signature")? }))
}

/// Downloads the release, calling `progress` with the fraction done (or
/// `None` when the size is unknown), and verifies its signature.
pub fn download(release: &Release, mut progress: impl FnMut(Option<f32>)) -> Result<Vec<u8>, String> {
    let mut resp = agent().get(&release.url).call().map_err(|e| e.to_string())?;
    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = resp.body_mut().with_config().limit(500 * 1024 * 1024).reader();
    let mut data = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        progress(total.map(|t| data.len() as f32 / t as f32));
    }
    verify(&data, &release.signature)?;
    Ok(data)
}

/// Checks `data` against the base64-encoded minisign signature.
pub fn verify(data: &[u8], signature_b64: &str) -> Result<(), String> {
    verify_with(PUBKEY, data, signature_b64)
}

/// `verify` against an explicit public key (base64 of the `.pub` file).
pub fn verify_with(pubkey_b64: &str, data: &[u8], signature_b64: &str) -> Result<(), String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let key_text = String::from_utf8(b64.decode(pubkey_b64.trim()).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let key_line = key_text.lines().nth(1).ok_or("bad public key")?;
    let pk = minisign_verify::PublicKey::from_base64(key_line).map_err(|e| e.to_string())?;
    let sig_text = String::from_utf8(b64.decode(signature_b64.trim()).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let sig = minisign_verify::Signature::decode(&sig_text).map_err(|e| e.to_string())?;
    pk.verify(data, &sig, false).map_err(|e| format!("signature check failed: {e}"))
}

/// macOS: replaces the running `.app` with the one in the `.app.tar.gz`.
#[cfg(target_os = "macos")]
pub fn install(archive: &[u8], bundle: &Path) -> Result<(), String> {
    let parent = bundle.parent().ok_or("no parent folder")?;
    // Unpack next to the bundle so the final swap is a rename on one volume.
    let staging = parent.join(format!(".franconvert-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let result = (|| {
        let gz = flate2::read::GzDecoder::new(archive);
        tar::Archive::new(gz).unpack(&staging).map_err(|e| e.to_string())?;
        let new_app: PathBuf = std::fs::read_dir(&staging)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "app"))
            .ok_or("archive has no .app")?;
        let backup = staging.join("previous.app");
        std::fs::rename(bundle, &backup).map_err(|e| e.to_string())?;
        if let Err(e) = std::fs::rename(&new_app, bundle) {
            let _ = std::fs::rename(&backup, bundle);
            return Err(e.to_string());
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Windows: saves the verified installer to the temp folder; it runs when
/// the user restarts (`run_installer`).
#[cfg(windows)]
pub fn stage(installer: &[u8], release: &Release) -> Result<PathBuf, String> {
    let name = release.url.rsplit('/').next().filter(|n| n.to_ascii_lowercase().ends_with(".exe")).unwrap_or("Franconvert-setup.exe");
    let dir = std::env::temp_dir().join(format!("franconvert-update-{}", release.version));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    std::fs::write(&path, installer).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Windows: once this process has exited, runs the installer silently and
/// starts the updated app.
#[cfg(windows)]
pub fn run_installer(installer: &Path, exe: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // `ping` is the classic one-second pause without a console window.
    let script = format!(
        "/c ping -n 2 127.0.0.1 >nul & start \"\" /wait \"{}\" /S & start \"\" \"{}\"",
        installer.display(),
        exe.display()
    );
    let _ = std::process::Command::new("cmd").raw_arg(script).creation_flags(CREATE_NO_WINDOW).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tauri_latest_json() {
        let json = r#"{"version":"0.10.0","notes":"x","pub_date":"2026-11-25T00:00:00Z","platforms":{
            "darwin-aarch64":{"signature":"c2ln","url":"https://example.com/Franconvert.app.tar.gz"}}}"#;
        let r = parse_latest(json, "0.9.3", "darwin-aarch64").unwrap().unwrap();
        assert_eq!(r.version, "0.10.0");
        assert!(parse_latest(json, "0.10.0", "darwin-aarch64").unwrap().is_none());
        assert!(parse_latest(json, "0.9.3", "linux-x86_64").is_err());
    }

    /// Checks a signature made by `tauri signer sign`:
    /// `UPD_PUB=key.pub UPD_FILE=x.tar.gz cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn verifies_tauri_signer_output() {
        let pubkey = std::fs::read_to_string(std::env::var("UPD_PUB").unwrap()).unwrap();
        let file = std::env::var("UPD_FILE").unwrap();
        let data = std::fs::read(&file).unwrap();
        let sig = std::fs::read_to_string(format!("{file}.sig")).unwrap();
        verify_with(&pubkey, &data, &sig).unwrap();
        let mut tampered = data.clone();
        tampered[100] ^= 1;
        assert!(verify_with(&pubkey, &tampered, &sig).is_err());
    }

    #[test]
    fn public_key_decodes() {
        let b64 = base64::engine::general_purpose::STANDARD;
        let key_text = String::from_utf8(b64.decode(PUBKEY).unwrap()).unwrap();
        minisign_verify::PublicKey::from_base64(key_text.lines().nth(1).unwrap()).unwrap();
    }
}
