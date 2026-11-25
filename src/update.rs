//! Looking for a newer release on GitHub and installing it.
//!
//! On startup the app asks GitHub for the latest published release (drafts and
//! pre-releases don't count). If it's newer than this build, the app offers it.
//! Installing downloads the release tarball, checks it against its `.sha256`, and
//! runs the install script inside it with the same prefix this copy was installed
//! to, through `pkexec` when that prefix isn't writable (see [`INSTALL`]). Copies
//! installed by pacman are left to pacman, and builds run from a source checkout
//! never check.
//!
//! Everything here blocks; the app runs it on a background thread.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

use crate::about::VERSION;

/// How this copy of Undavi was installed, which decides what an update can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// Installed with `install.sh` (or a release tarball) into this prefix, so a
    /// new tarball can be installed over it.
    Prefix(PathBuf),
    /// Owned by a package manager (pacman); updates go through it.
    Package,
    /// Run from a source checkout, or a system without release builds: no checks.
    Unsupported,
}

/// A newer release than the running one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// Its version, without the leading "v" ("0.10.0").
    pub version: String,
    /// The release's GitHub page.
    pub page: String,
    /// Download addresses of the Linux tarball and its checksum, when the release has them.
    pub tarball: Option<(String, String)>,
}

/// What a check found.
#[derive(Clone, Debug)]
pub struct Found {
    pub install: Install,
    /// A newer release, if there is one.
    pub release: Option<Release>,
}

/// The repository's `owner/name` on GitHub, from Cargo.toml's `repository`.
fn github_repo() -> Option<&'static str> {
    env!("CARGO_PKG_REPOSITORY")
        .strip_prefix("https://github.com/")
        .map(|r| r.trim_end_matches('/'))
        .filter(|r| r.split('/').count() == 2)
}

/// The release tarball's name, as `release.yml` builds it.
fn tarball_name(version: &str) -> String {
    format!("undavi-{version}-x86_64-linux")
}

/// Work out how this copy was installed.
pub fn detect_install() -> Install {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Install::Unsupported;
    }
    let Ok(exe) = std::env::current_exe().and_then(|p| p.canonicalize()) else {
        return Install::Unsupported;
    };
    let owned_by_pacman = Command::new("pacman")
        .arg("-Qqo")
        .arg(&exe)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if owned_by_pacman {
        return Install::Package;
    }
    install_from_path(&exe)
}

/// `<prefix>/bin/undavi` is an install; anything else (`target/release`, a
/// `cargo install` into `~/.cargo/bin`) is a build from source.
fn install_from_path(exe: &Path) -> Install {
    let bin = exe
        .parent()
        .filter(|d| d.file_name() == Some("bin".as_ref()));
    match bin.and_then(Path::parent) {
        Some(prefix) if prefix.file_name() != Some(".cargo".as_ref()) => {
            Install::Prefix(prefix.to_path_buf())
        }
        _ => Install::Unsupported,
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(timeout))
        .user_agent(crate::about::user_agent())
        .build()
        .into()
}

/// Detect the install, then (unless it's a source build) ask GitHub for the
/// latest release.
pub fn check() -> Result<Found, String> {
    let install = detect_install();
    if install == Install::Unsupported {
        return Ok(Found {
            install,
            release: None,
        });
    }
    let repo = github_repo().ok_or("the repository isn't on GitHub")?;
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let agent = agent(Duration::from_secs(15));
    let release = match agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(mut resp) => {
            let latest: GhRelease = resp.body_mut().read_json().map_err(|e| e.to_string())?;
            newer_release(latest, repo, VERSION)
        }
        // No published release yet.
        Err(ureq::Error::StatusCode(404)) => None,
        Err(e) => return Err(e.to_string()),
    };
    Ok(Found { install, release })
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

/// `latest` as a [`Release`], if it's newer than `current`. Only files from the
/// repository's own releases count.
fn newer_release(latest: GhRelease, repo: &str, current: &str) -> Option<Release> {
    let tag = latest.tag_name;
    let version = tag.strip_prefix('v').unwrap_or(&tag).to_string();
    // It becomes part of file names and addresses.
    let plain = version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    if !plain || !is_newer(&version, current) {
        return None;
    }
    let name = tarball_name(&version);
    let downloads = format!("https://github.com/{repo}/releases/download/");
    let asset = |file: String| {
        latest
            .assets
            .iter()
            .find(|a| a.name == file)
            .map(|a| a.browser_download_url.clone())
            .filter(|url| url.starts_with(&downloads))
    };
    let tarball = asset(format!("{name}.tar.gz")).zip(asset(format!("{name}.tar.gz.sha256")));
    Some(Release {
        page: format!("https://github.com/{repo}/releases/tag/{tag}"),
        version,
        tarball,
    })
}

/// "1.2.3" (pre-release and build suffixes ignored) as numbers.
fn parse_version(v: &str) -> Option<[u64; 3]> {
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let out = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(out)
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Download, verify and install `release` into `prefix`. Takes effect the next
/// time Undavi starts.
pub fn install(release: &Release, prefix: &Path) -> Result<(), String> {
    let (tarball_url, checksum_url) = release
        .tarball
        .as_ref()
        .ok_or("this release has no Linux download")?;
    let dir = dirs::cache_dir()
        .ok_or("no cache folder")?
        .join("undavi")
        .join("update");
    // Start clean, and never leave a download behind.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let result = download_and_install(release, prefix, &dir, tarball_url, checksum_url);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn download_and_install(
    release: &Release,
    prefix: &Path,
    dir: &Path,
    tarball_url: &str,
    checksum_url: &str,
) -> Result<(), String> {
    let name = tarball_name(&release.version);
    let archive = dir.join(format!("{name}.tar.gz"));
    let agent = agent(Duration::from_secs(15 * 60));

    let expected = agent
        .get(checksum_url)
        .call()
        .and_then(|mut r| r.body_mut().read_to_string())
        .map_err(|e| format!("couldn't download the checksum: {e}"))?;
    let expected = expected
        .split_whitespace()
        .next()
        .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or("the checksum file is malformed")?
        .to_ascii_lowercase();

    let mut resp = agent
        .get(tarball_url)
        .call()
        .map_err(|e| format!("couldn't download the update: {e}"))?;
    let mut file = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
    let mut body = resp
        .body_mut()
        .with_config()
        .limit(512 * 1024 * 1024)
        .reader();
    std::io::copy(&mut body, &mut file).map_err(|e| format!("download failed: {e}"))?;
    drop(file);

    let actual = run(Command::new("sha256sum").arg(&archive))?;
    if actual.split_whitespace().next() != Some(expected.as_str()) {
        return Err("the download doesn't match its checksum".into());
    }

    run_installer(
        &archive,
        &expected,
        &name,
        prefix,
        !writable(&prefix.join("bin")),
    )
}

/// Unpacks a tarball and runs the install script in it: the archive, its SHA-256,
/// its top folder and the prefix.
///
/// This may run as root while the download sits in the user's cache folder, where
/// anything running as the user could change it (say, while the password prompt is
/// open). So it works on a copy of its own that nobody else can write to, and checks
/// that copy. It's passed on the command line rather than saved to a file for the
/// same reason.
const INSTALL: &str = r#"
set -eu
umask 022
work=$(mktemp -d "${TMPDIR:-/tmp}/undavi-update.XXXXXXXXXX")
trap 'rm -rf "$work"' EXIT
cp "$1" "$work/update.tar.gz"
if ! printf '%s  %s\n' "$2" "$work/update.tar.gz" | sha256sum -c - >/dev/null 2>&1; then
    echo "the download doesn't match its checksum" >&2
    exit 3
fi
tar -xzf "$work/update.tar.gz" -C "$work" --no-same-owner
for script in "$work/$3/install.sh" "$work/$3/packaging/install.sh"; do
    if [ -f "$script" ]; then
        PREFIX=$4 /bin/sh "$script" >/dev/null
        exit 0
    fi
done
echo "the download has no install script" >&2
exit 4
"#;

/// Install the archive with [`INSTALL`], through `pkexec` if it needs to be root.
fn run_installer(
    archive: &Path,
    sha256: &str,
    name: &str,
    prefix: &Path,
    elevated: bool,
) -> Result<(), String> {
    let mut cmd = if elevated {
        let mut c = Command::new("pkexec");
        c.arg("/bin/sh");
        c
    } else {
        Command::new("/bin/sh")
    };
    let out = cmd
        .args(["-c", INSTALL, "undavi-update"])
        .arg(archive)
        .args([sha256, name])
        .arg(prefix)
        .output()
        .map_err(|e| format!("couldn't run the installer: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(()),
        // pkexec: the password dialog was dismissed, or authentication failed.
        Some(126 | 127) if elevated => Err("the update was cancelled".into()),
        _ => Err(format!(
            "the installer failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// Run `cmd`, returning its standard output.
fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("couldn't run {:?}: {e}", cmd.get_program()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{:?} failed: {}",
            cmd.get_program(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Whether this user can create files in `dir`.
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".undavi-update-{}", std::process::id()));
    let ok = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Start the installed (new) Undavi. The caller then exits.
pub fn relaunch(prefix: &Path) -> Result<(), String> {
    if cfg!(test) {
        return Ok(());
    }
    Command::new(prefix.join("bin").join("undavi"))
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "karsoncaliff/undavi";

    fn gh(tag: &str, assets: &[&str]) -> GhRelease {
        gh_at(
            &format!("https://github.com/{REPO}/releases/download/{tag}"),
            tag,
            assets,
        )
    }

    fn gh_at(base: &str, tag: &str, assets: &[&str]) -> GhRelease {
        GhRelease {
            tag_name: tag.into(),
            assets: assets
                .iter()
                .map(|n| GhAsset {
                    name: (*n).into(),
                    browser_download_url: format!("{base}/{n}"),
                })
                .collect(),
        }
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.9.0", "0.9.0"));
        assert!(!is_newer("0.8.5", "0.9.0"));
        assert!(!is_newer("garbage", "0.9.0"));
        assert_eq!(parse_version("1.2.3-beta.1"), Some([1, 2, 3]));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn picks_the_linux_tarball_and_checksum() {
        let r = newer_release(
            gh(
                "v0.10.0",
                &[
                    "undavi-0.10.0-x86_64-linux.tar.gz",
                    "undavi-0.10.0-x86_64-linux.tar.gz.sha256",
                ],
            ),
            REPO,
            "0.9.0",
        )
        .expect("newer");
        assert_eq!(r.version, "0.10.0");
        assert_eq!(
            r.page,
            "https://github.com/karsoncaliff/undavi/releases/tag/v0.10.0"
        );
        let (tar, sum) = r.tarball.expect("tarball");
        assert!(tar.ends_with(".tar.gz"));
        assert!(sum.ends_with(".sha256"));
        // Without both files there's nothing to install.
        let r = newer_release(
            gh("v0.10.0", &["undavi-0.10.0-x86_64-linux.tar.gz"]),
            REPO,
            "0.9.0",
        );
        assert_eq!(r.expect("newer").tarball, None);
        // Not newer: nothing to offer.
        assert!(newer_release(gh("v0.9.0", &[]), REPO, "0.9.0").is_none());
    }

    #[test]
    fn only_the_repositorys_own_releases_count() {
        let files = [
            "undavi-0.10.0-x86_64-linux.tar.gz",
            "undavi-0.10.0-x86_64-linux.tar.gz.sha256",
        ];
        for base in [
            "https://example.invalid",
            "http://github.com/karsoncaliff/undavi/releases/download/v0.10.0",
            "https://github.com/someone/undavi/releases/download/v0.10.0",
            "https://github.com.example.invalid/karsoncaliff/undavi/releases/download",
        ] {
            let r = newer_release(gh_at(base, "v0.10.0", &files), REPO, "0.9.0");
            assert_eq!(r.expect("newer").tarball, None, "{base}");
        }
        // Versions that would end up in a path or an address.
        for tag in [
            "v0.10.0-../../x",
            "v0.10.0-a/b",
            "v0.10.0-a b",
            "v0.10.0-&x",
        ] {
            assert!(
                newer_release(gh(tag, &files), REPO, "0.9.0").is_none(),
                "{tag}"
            );
        }
        assert!(newer_release(gh("v0.10.0-rc.1+2", &[]), REPO, "0.9.0").is_some());
    }

    /// A tarball whose install script notes its prefix in `<prefix>/installed`.
    #[cfg(target_os = "linux")]
    fn tarball(dir: &Path, name: &str, script: &str) -> (PathBuf, String) {
        let root = dir.join(name);
        std::fs::create_dir_all(root.join(script).parent().unwrap()).unwrap();
        std::fs::write(
            root.join(script),
            "set -eu\necho \"$PREFIX\" > \"$PREFIX/installed\"\n",
        )
        .unwrap();
        let archive = dir.join(format!("{name}.tar.gz"));
        run(Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(dir)
            .arg(name))
        .unwrap();
        let sum = run(Command::new("sha256sum").arg(&archive)).unwrap();
        (archive, sum.split_whitespace().next().unwrap().to_string())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_installer_only_runs_what_was_checked() {
        let dir = std::env::temp_dir().join(format!("undavi update {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prefix = dir.join("a prefix");
        std::fs::create_dir_all(&prefix).unwrap();
        let installed = prefix.join("installed");

        for (name, script) in [("new", "install.sh"), ("old", "packaging/install.sh")] {
            let (archive, sum) = tarball(&dir, name, script);
            run_installer(&archive, &sum, name, &prefix, false).unwrap();
            let noted = std::fs::read_to_string(&installed).unwrap();
            assert_eq!(noted.trim(), prefix.to_str().unwrap());
            std::fs::remove_file(&installed).unwrap();
        }

        // Changed after it was checked: nothing from it runs.
        let (_, sum) = tarball(&dir, "new", "install.sh");
        std::fs::write(
            dir.join("new/install.sh"),
            "echo changed > \"$PREFIX/installed\"\n",
        )
        .unwrap();
        let archive = dir.join("new.tar.gz");
        run(Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&dir)
            .arg("new"))
        .unwrap();
        let err = run_installer(&archive, &sum, "new", &prefix, false).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(!installed.exists());

        // No script, and a script that fails.
        let (archive, sum) = tarball(&dir, "other", "readme");
        let err = run_installer(&archive, &sum, "other", &prefix, false).unwrap_err();
        assert!(err.contains("no install script"), "{err}");
        let (archive, sum) = tarball(&dir, "fails", "install.sh");
        let missing = dir.join("missing");
        assert!(run_installer(&archive, &sum, "fails", &missing, false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_prefix_comes_from_the_binary_path() {
        assert_eq!(
            install_from_path(Path::new("/usr/bin/undavi")),
            Install::Prefix("/usr".into())
        );
        assert_eq!(
            install_from_path(Path::new("/home/me/.local/bin/undavi")),
            Install::Prefix("/home/me/.local".into())
        );
        assert_eq!(
            install_from_path(Path::new("/home/me/undavi/target/release/undavi")),
            Install::Unsupported
        );
        assert_eq!(
            install_from_path(Path::new("/home/me/.cargo/bin/undavi")),
            Install::Unsupported
        );
    }

    #[test]
    fn repository_is_on_github() {
        assert_eq!(github_repo(), Some("karsoncaliff/undavi"));
    }
}
