//! Native messaging host registration: the manifests that tell each browser
//! which executable to start for [`HOST_NAME`](crate::HOST_NAME).
//!
//! The app registers itself (per user, no admin rights) on every launch, so the
//! manifests always point at the installed executable, wherever it moved.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{CHROMIUM_EXTENSION_ID, FIREFOX_EXTENSION_ID, HOST_NAME};

const DESCRIPTION: &str = "NexusDL download manager";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// Chrome, Edge, Brave, Vivaldi, Chromium: `allowed_origins`.
    Chromium,
    /// Firefox: `allowed_extensions`.
    Firefox,
}

#[must_use]
pub fn manifest(flavor: Flavor, exe: &Path) -> Value {
    let mut manifest = json!({
        "name": HOST_NAME,
        "description": DESCRIPTION,
        "path": exe,
        "type": "stdio",
    });
    match flavor {
        Flavor::Chromium => {
            manifest["allowed_origins"] =
                json!([format!("chrome-extension://{CHROMIUM_EXTENSION_ID}/")]);
        }
        Flavor::Firefox => manifest["allowed_extensions"] = json!([FIREFOX_EXTENSION_ID]),
    }
    manifest
}

/// One browser the host was registered with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub browser: &'static str,
    /// Manifest file (or registry key on Windows).
    pub location: String,
}

/// Registers the host with every supported browser found for this user.
/// Failures for one browser don't stop the others.
#[must_use]
pub fn install(exe: &Path, data_dir: &Path) -> Vec<Result<Registration, String>> {
    #[cfg(windows)]
    {
        windows::install(exe, data_dir)
    }
    #[cfg(not(windows))]
    {
        let _ = data_dir;
        match unix_targets() {
            Some(targets) => install_files(exe, &targets),
            None => vec![Err("no home directory".into())],
        }
    }
}

/// A browser's per-user manifest directory, used only if `profile_root`
/// exists (so uninstalled browsers don't get stray folders).
#[derive(Debug, Clone)]
pub struct Target {
    pub browser: &'static str,
    pub flavor: Flavor,
    pub profile_root: PathBuf,
    pub manifest_dir: PathBuf,
}

/// Writes the manifest into each present browser's directory, skipping files
/// that are already up to date.
#[must_use]
pub fn install_files(exe: &Path, targets: &[Target]) -> Vec<Result<Registration, String>> {
    targets
        .iter()
        .filter(|t| t.profile_root.is_dir())
        .map(|t| {
            let path = t.manifest_dir.join(format!("{HOST_NAME}.json"));
            write_if_changed(&path, &manifest(t.flavor, exe))
                .map(|()| Registration {
                    browser: t.browser,
                    location: path.display().to_string(),
                })
                .map_err(|e| format!("{}: {e}", t.browser))
        })
        .collect()
}

fn write_if_changed(path: &Path, manifest: &Value) -> io::Result<()> {
    let contents = serde_json::to_vec_pretty(manifest)?;
    if fs::read(path).is_ok_and(|current| current == contents) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, contents)
}

#[cfg(not(windows))]
fn unix_targets() -> Option<Vec<Target>> {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)?;
    #[cfg(target_os = "macos")]
    let targets = macos_targets(&home);
    #[cfg(not(target_os = "macos"))]
    let targets = linux_targets(
        &home,
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
    );
    Some(targets)
}

/// Per-user manifest locations on Linux (and other XDG systems).
#[must_use]
pub fn linux_targets(home: &Path, xdg_config: Option<PathBuf>) -> Vec<Target> {
    let config = xdg_config
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    let chromium = |browser, dir: &str| {
        let profile_root = config.join(dir);
        Target {
            browser,
            flavor: Flavor::Chromium,
            manifest_dir: profile_root.join("NativeMessagingHosts"),
            profile_root,
        }
    };
    vec![
        chromium("Google Chrome", "google-chrome"),
        chromium("Chromium", "chromium"),
        chromium("Microsoft Edge", "microsoft-edge"),
        chromium("Brave", "BraveSoftware/Brave-Browser"),
        chromium("Vivaldi", "vivaldi"),
        Target {
            browser: "Firefox",
            flavor: Flavor::Firefox,
            profile_root: home.join(".mozilla"),
            manifest_dir: home.join(".mozilla/native-messaging-hosts"),
        },
    ]
}

/// Per-user manifest locations on macOS.
#[must_use]
pub fn macos_targets(home: &Path) -> Vec<Target> {
    let support = home.join("Library/Application Support");
    let target = |browser, flavor, dir: &str| {
        let profile_root = support.join(dir);
        Target {
            browser,
            flavor,
            manifest_dir: profile_root.join("NativeMessagingHosts"),
            profile_root,
        }
    };
    vec![
        target("Google Chrome", Flavor::Chromium, "Google/Chrome"),
        target("Chromium", Flavor::Chromium, "Chromium"),
        target("Microsoft Edge", Flavor::Chromium, "Microsoft Edge"),
        target("Brave", Flavor::Chromium, "BraveSoftware/Brave-Browser"),
        target("Vivaldi", Flavor::Chromium, "Vivaldi"),
        target("Firefox", Flavor::Firefox, "Mozilla"),
    ]
}

#[cfg(windows)]
mod windows {
    //! Windows looks manifests up through `HKCU\Software\<vendor>\NativeMessagingHosts\<name>`,
    //! whose default value is the manifest's path.

    use std::path::Path;

    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    use super::{Flavor, Registration, manifest, write_if_changed};
    use crate::HOST_NAME;

    const KEYS: [(&str, Flavor, &str); 5] = [
        ("Google Chrome", Flavor::Chromium, r"Software\Google\Chrome"),
        ("Chromium", Flavor::Chromium, r"Software\Chromium"),
        (
            "Microsoft Edge",
            Flavor::Chromium,
            r"Software\Microsoft\Edge",
        ),
        (
            "Brave",
            Flavor::Chromium,
            r"Software\BraveSoftware\Brave-Browser",
        ),
        ("Firefox", Flavor::Firefox, r"Software\Mozilla"),
    ];

    pub fn install(exe: &Path, data_dir: &Path) -> Vec<Result<Registration, String>> {
        let dir = data_dir.join("native-messaging");
        let chromium = dir.join(format!("{HOST_NAME}.chromium.json"));
        let firefox = dir.join(format!("{HOST_NAME}.firefox.json"));
        for (path, flavor) in [(&chromium, Flavor::Chromium), (&firefox, Flavor::Firefox)] {
            if let Err(e) = write_if_changed(path, &manifest(flavor, exe)) {
                return vec![Err(format!("writing {}: {e}", path.display()))];
            }
        }
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        KEYS.iter()
            .map(|&(browser, flavor, vendor)| {
                let key = format!(r"{vendor}\NativeMessagingHosts\{HOST_NAME}");
                let manifest = match flavor {
                    Flavor::Chromium => &chromium,
                    Flavor::Firefox => &firefox,
                };
                hkcu.create_subkey(&key)
                    .and_then(|(k, _)| k.set_value("", &manifest.display().to_string()))
                    .map(|()| Registration {
                        browser,
                        location: format!(r"HKCU\{key}"),
                    })
                    .map_err(|e| format!("{browser}: {e}"))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_name_the_host_and_its_extension() {
        let exe = Path::new("/opt/NexusDL/nexusdl");
        let chromium = manifest(Flavor::Chromium, exe);
        assert_eq!(chromium["name"], HOST_NAME);
        assert_eq!(chromium["path"], "/opt/NexusDL/nexusdl");
        assert_eq!(chromium["type"], "stdio");
        assert_eq!(
            chromium["allowed_origins"],
            json!(["chrome-extension://aiahehffjnedhfbjllinegdigmhpkghl/"])
        );
        let firefox = manifest(Flavor::Firefox, exe);
        assert_eq!(
            firefox["allowed_extensions"],
            json!(["nexusdl@nexusdl.dev"])
        );
        assert!(firefox.get("allowed_origins").is_none());
    }

    #[test]
    fn registers_only_with_installed_browsers() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".config/google-chrome")).unwrap();
        fs::create_dir_all(home.path().join(".mozilla")).unwrap();
        let exe = Path::new("/usr/bin/nexusdl");

        let targets = linux_targets(home.path(), None);
        let done: Vec<_> = install_files(exe, &targets)
            .into_iter()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            done.iter().map(|r| r.browser).collect::<Vec<_>>(),
            ["Google Chrome", "Firefox"]
        );

        let chrome = home
            .path()
            .join(".config/google-chrome/NativeMessagingHosts/dev.nexusdl.host.json");
        let written: Value = serde_json::from_slice(&fs::read(&chrome).unwrap()).unwrap();
        assert_eq!(written, manifest(Flavor::Chromium, exe));
        let firefox = home
            .path()
            .join(".mozilla/native-messaging-hosts/dev.nexusdl.host.json");
        let written: Value = serde_json::from_slice(&fs::read(&firefox).unwrap()).unwrap();
        assert_eq!(written, manifest(Flavor::Firefox, exe));
        assert!(!home.path().join(".config/chromium").exists());

        // Re-registering leaves current files alone and repoints moved installs.
        let before = fs::metadata(&chrome).unwrap().modified().unwrap();
        let _ = install_files(exe, &targets);
        assert_eq!(fs::metadata(&chrome).unwrap().modified().unwrap(), before);
        let _ = install_files(Path::new("/opt/nexusdl"), &targets);
        let moved: Value = serde_json::from_slice(&fs::read(&chrome).unwrap()).unwrap();
        assert_eq!(moved["path"], "/opt/nexusdl");
    }

    // Unix-style absolute paths aren't absolute on Windows.
    #[cfg(unix)]
    #[test]
    fn honours_xdg_config_home_and_macos_layout() {
        let home = Path::new("/home/u");
        let linux = linux_targets(home, Some("/cfg".into()));
        assert_eq!(
            linux[0].manifest_dir,
            Path::new("/cfg/google-chrome/NativeMessagingHosts")
        );
        // Relative XDG paths are invalid per the spec and ignored.
        let linux = linux_targets(home, Some("rel".into()));
        assert_eq!(
            linux[0].profile_root,
            Path::new("/home/u/.config/google-chrome")
        );

        let mac = macos_targets(home);
        assert_eq!(
            mac[0].manifest_dir,
            Path::new("/home/u/Library/Application Support/Google/Chrome/NativeMessagingHosts")
        );
        let firefox = mac.iter().find(|t| t.flavor == Flavor::Firefox).unwrap();
        assert_eq!(
            firefox.manifest_dir,
            Path::new("/home/u/Library/Application Support/Mozilla/NativeMessagingHosts")
        );
    }
}
