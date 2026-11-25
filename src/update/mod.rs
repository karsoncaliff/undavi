//! Looking for a newer release on GitHub and installing it.
//!
//! On startup the app asks GitHub for the latest published release. Drafts don't count,
//! and pre-releases (unstable versions) only if the user asked for them. If it's newer
//! than this build, the app offers it.
//! Installing downloads the release archive, checks it against its `.sha256`, and
//! runs the install script inside it with the same prefix this copy was installed
//! to (on Linux, through `pkexec` when that prefix isn't writable; see
//! `linux::INSTALL`). Copies installed by pacman are left to pacman, and builds run
//! from a source checkout never check.
//!
//! Everything here blocks; the app runs it on a background thread.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;

use crate::about::VERSION;

#[cfg(not(windows))]
mod linux;
#[cfg(not(windows))]
use linux as system;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as system;

/// How this copy of Undavi was installed, which decides what an update can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// Installed with an install script (`install.sh`, `install.ps1`) or the Windows
    /// installer into this prefix, so a new release can be installed over it.
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
    /// It's a pre-release, only for trying the latest features.
    pub unstable: bool,
    /// The release's GitHub page.
    pub page: String,
    /// Download addresses of the archive for this system and its checksum, when the
    /// release has them.
    pub archive: Option<(String, String)>,
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

/// The folder in the release archive, as `release.yml` builds it.
fn archive_folder(version: &str) -> String {
    format!("undavi-{version}-{}", system::PLATFORM)
}

/// The file name of what to download for this install: an archive, or the Windows
/// installer.
fn archive_name(version: &str, install: &Install) -> String {
    let prefix = match install {
        Install::Prefix(prefix) => Some(prefix.as_path()),
        _ => None,
    };
    archive_folder(version) + system::archive_ending(prefix)
}

/// Work out how this copy was installed.
pub fn detect_install() -> Install {
    if !cfg!(all(
        any(target_os = "linux", windows),
        target_arch = "x86_64"
    )) {
        return Install::Unsupported;
    }
    let exe = std::env::current_exe().and_then(|exe| {
        if cfg!(windows) {
            // As it is: few programs take the path this would make it.
            Ok(exe)
        } else {
            exe.canonicalize()
        }
    });
    let Ok(exe) = exe else {
        return Install::Unsupported;
    };
    if system::is_package(&exe) {
        return Install::Package;
    }
    match install_from_path(&exe) {
        Install::Prefix(prefix) if !system::is_install(&prefix) => Install::Unsupported,
        install => install,
    }
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
/// latest release, or with `unstable` for the newest one of any kind.
pub fn check(unstable: bool) -> Result<Found, String> {
    let install = detect_install();
    #[cfg(windows)]
    if let Install::Prefix(prefix) = &install {
        system::clean_up(prefix);
    }
    if install == Install::Unsupported {
        return Ok(Found {
            install,
            release: None,
        });
    }
    let repo = github_repo().ok_or("the repository isn't on GitHub")?;
    // Package managers only have the stable ones.
    let unstable = unstable && install != Install::Package;
    let releases = format!("https://api.github.com/repos/{repo}/releases");
    let url = if unstable {
        format!("{releases}?per_page=30")
    } else {
        // (Pre-releases are never the latest.)
        format!("{releases}/latest")
    };
    let agent = agent(Duration::from_secs(15));
    let response = agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call();
    let latest = match response {
        Ok(mut resp) if unstable => {
            let all = resp.body_mut().read_json().map_err(|e| e.to_string())?;
            newest(all)
        }
        Ok(mut resp) => Some(resp.body_mut().read_json().map_err(|e| e.to_string())?),
        // No published release yet.
        Err(ureq::Error::StatusCode(404)) => None,
        Err(e) => return Err(e.to_string()),
    };
    let release = latest
        .and_then(|latest| newer_release(latest, repo, VERSION, &install))
        .filter(|release| unstable || !release.unstable);
    Ok(Found { install, release })
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

impl GhRelease {
    /// Its version, without the leading "v".
    fn version(&self) -> &str {
        self.tag_name.strip_prefix('v').unwrap_or(&self.tag_name)
    }
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

/// The one with the highest version. (GitHub lists them by date.)
fn newest(releases: Vec<GhRelease>) -> Option<GhRelease> {
    releases
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| Some((Version::parse(r.version())?, r)))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, r)| r)
}

/// `latest` as a [`Release`], if it's newer than `current`. Only files from the
/// repository's own releases count.
fn newer_release(
    latest: GhRelease,
    repo: &str,
    current: &str,
    install: &Install,
) -> Option<Release> {
    let version = latest.version().to_string();
    let tag = latest.tag_name;
    // It becomes part of file names and addresses.
    let plain = version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    if !plain || !is_newer(&version, current) {
        return None;
    }
    let unstable = latest.prerelease || Version::parse(&version).is_some_and(|v| !v.finished);
    let name = archive_name(&version, install);
    let downloads = format!("https://github.com/{repo}/releases/download/");
    let asset = |file: String| {
        latest
            .assets
            .iter()
            .find(|a| a.name == file)
            .map(|a| a.browser_download_url.clone())
            .filter(|url| url.starts_with(&downloads))
    };
    let checksum = asset(format!("{name}.sha256"));
    let archive = asset(name).zip(checksum);
    Some(Release {
        page: format!("https://github.com/{repo}/releases/tag/{tag}"),
        version,
        unstable,
        archive,
    })
}

/// A version, ordered as in Semantic Versioning: 1.0.0-alpha, 1.0.0-alpha.2,
/// 1.0.0-beta, 1.0.0. (By the fields in this order.)
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    numbers: [u64; 3],
    /// Not a pre-release.
    finished: bool,
    /// A pre-release's "alpha.2".
    stage: Vec<Identifier>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Identifier {
    Number(u64),
    Text(String),
}

impl Version {
    /// "1.2.3" or "1.2.3-beta.1", with or without build metadata ("+…", which doesn't
    /// count).
    fn parse(v: &str) -> Option<Self> {
        let v = v.split('+').next()?;
        let (core, stage) = match v.split_once('-') {
            Some((core, stage)) => (core, Some(stage)),
            None => (v, None),
        };
        let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
        let numbers = [parts.next()??, parts.next()??, parts.next()??];
        if parts.next().is_some() {
            return None;
        }
        let identifier = |s: &str| {
            let number = s.parse().map(Identifier::Number);
            (!s.is_empty()).then(|| number.unwrap_or_else(|_| Identifier::Text(s.into())))
        };
        Some(Self {
            numbers,
            finished: stage.is_none(),
            stage: match stage {
                Some(stage) => stage.split('.').map(identifier).collect::<Option<_>>()?,
                None => Vec::new(),
            },
        })
    }
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (Version::parse(latest), Version::parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Download, verify and install `release` into `prefix`. Takes effect the next
/// time Undavi starts.
pub fn install(release: &Release, prefix: &Path) -> Result<(), String> {
    let (archive_url, checksum_url) = release
        .archive
        .as_ref()
        .ok_or("this release has no download for this system")?;
    let dir = dirs::cache_dir()
        .ok_or("no cache folder")?
        .join("undavi")
        .join("update");
    // Start clean, and never leave a download behind.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let result = download_and_install(release, prefix, &dir, archive_url, checksum_url);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn download_and_install(
    release: &Release,
    prefix: &Path,
    dir: &Path,
    archive_url: &str,
    checksum_url: &str,
) -> Result<(), String> {
    let install = Install::Prefix(prefix.to_path_buf());
    let archive = dir.join(archive_name(&release.version, &install));
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
        .get(archive_url)
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

    if system::sha256(&archive)? != expected {
        return Err("the download doesn't match its checksum".into());
    }

    system::run_installer(
        &archive,
        &expected,
        &archive_folder(&release.version),
        prefix,
        !writable(&prefix.join("bin")),
    )
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
    Command::new(prefix.join("bin").join(system::PROGRAM))
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
            draft: false,
            prerelease: false,
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
        for malformed in ["1.2", "1.2.3.4", "1.2.x", "1.2.3-", "1.2.3-a..b", ""] {
            assert_eq!(Version::parse(malformed), None, "{malformed}");
        }
    }

    #[test]
    fn unstable_versions_come_before_the_finished_one() {
        // (The examples of Semantic Versioning, and the version before and after.)
        let order = [
            "0.10.0",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.2",
            "1.0.0-alpha.10",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1-alpha",
        ];
        for (i, a) in order.iter().enumerate() {
            for (j, b) in order.iter().enumerate() {
                assert_eq!(is_newer(a, b), i > j, "{a} and {b}");
            }
        }
        assert!(!is_newer("1.0.0+build.2", "1.0.0+build.1"));
    }

    #[test]
    fn the_newest_of_all_releases_is_the_highest_version() {
        let release = |tag: &str, draft| GhRelease {
            draft,
            prerelease: tag.contains('-'),
            ..gh(tag, &FILES)
        };
        let pick = |tags: &[&str]| {
            newest(tags.iter().map(|t| release(t, false)).collect()).map(|r| r.tag_name)
        };
        // A fix of an older version can come out later.
        let tags = ["v0.10.1", "v1.0.0-alpha.2", "v1.0.0-alpha", "v0.10.0"];
        assert_eq!(pick(&tags).as_deref(), Some("v1.0.0-alpha.2"));
        assert_eq!(pick(&["v1.0.0-rc.1", "v1.0.0"]).as_deref(), Some("v1.0.0"));
        assert_eq!(pick(&["nightly", "v0.9.0"]).as_deref(), Some("v0.9.0"));
        assert_eq!(pick(&["nightly"]), None);
        let found = newest(vec![release("v2.0.0", true), release("v1.0.0", false)]);
        assert_eq!(found.map(|r| r.tag_name).as_deref(), Some("v1.0.0"));

        // It says so, whether GitHub does or just its version.
        assert!(
            newer(release("v1.0.0-alpha", false), "0.10.0")
                .unwrap()
                .unstable
        );
        assert!(
            newer(gh("v1.0.0-alpha", &FILES), "0.10.0")
                .unwrap()
                .unstable
        );
        assert!(!newer(gh("v1.0.0", &FILES), "1.0.0-alpha").unwrap().unstable);
        let odd = GhRelease {
            prerelease: true,
            ..gh("v1.0.0", &FILES)
        };
        assert!(newer(odd, "0.10.0").unwrap().unstable);
    }

    /// Everything a release comes with.
    const FILES: [&str; 6] = [
        "undavi-0.10.0-x86_64-linux.tar.gz",
        "undavi-0.10.0-x86_64-linux.tar.gz.sha256",
        "undavi-0.10.0-x86_64-windows.zip",
        "undavi-0.10.0-x86_64-windows.zip.sha256",
        "undavi-0.10.0-x86_64-windows-setup.exe",
        "undavi-0.10.0-x86_64-windows-setup.exe.sha256",
    ];

    /// [`newer_release`], for an install by a script.
    fn newer(latest: GhRelease, current: &str) -> Option<Release> {
        newer_release(latest, REPO, current, &Install::Prefix("/usr".into()))
    }

    #[test]
    fn picks_this_systems_archive_and_checksum() {
        let r = newer(gh("v0.10.0", &FILES), "0.9.0").expect("newer");
        assert_eq!(r.version, "0.10.0");
        assert_eq!(
            r.page,
            "https://github.com/karsoncaliff/undavi/releases/tag/v0.10.0"
        );
        let (archive, sum) = r.archive.expect("archive");
        let (file, folder) = if cfg!(windows) {
            (FILES[2], "undavi-0.10.0-x86_64-windows")
        } else {
            (FILES[0], "undavi-0.10.0-x86_64-linux")
        };
        assert!(archive.ends_with(&format!("/{file}")), "{archive}");
        assert_eq!(sum, format!("{archive}.sha256"));
        assert_eq!(archive_folder("0.10.0"), folder);
        // Without both files there's nothing to install.
        for missing in FILES {
            let some: Vec<&str> = FILES.into_iter().filter(|f| *f != missing).collect();
            let r = newer(gh("v0.10.0", &some), "0.9.0").expect("newer");
            let ours = missing.starts_with(file);
            assert_eq!(r.archive.is_none(), ours, "without {missing}");
        }
        // Not newer: nothing to offer.
        assert!(newer(gh("v0.9.0", &[]), "0.9.0").is_none());
    }

    #[test]
    fn only_the_repositorys_own_releases_count() {
        for base in [
            "https://example.invalid",
            "http://github.com/karsoncaliff/undavi/releases/download/v0.10.0",
            "https://github.com/someone/undavi/releases/download/v0.10.0",
            "https://github.com.example.invalid/karsoncaliff/undavi/releases/download",
        ] {
            let r = newer(gh_at(base, "v0.10.0", &FILES), "0.9.0");
            assert_eq!(r.expect("newer").archive, None, "{base}");
        }
        // Versions that would end up in a path or an address.
        for tag in [
            "v0.10.0-../../x",
            "v0.10.0-a/b",
            "v0.10.0-a b",
            "v0.10.0-&x",
        ] {
            assert!(newer(gh(tag, &FILES), "0.9.0").is_none(), "{tag}");
        }
        assert!(newer(gh("v0.10.0-rc.1+2", &[]), "0.9.0").is_some());
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
        #[cfg(windows)]
        assert_eq!(
            install_from_path(Path::new(
                r"C:\Users\me\AppData\Local\Programs\Undavi\bin\undavi.exe"
            )),
            Install::Prefix(r"C:\Users\me\AppData\Local\Programs\Undavi".into())
        );
    }

    #[test]
    fn repository_is_on_github() {
        assert_eq!(github_repo(), Some("karsoncaliff/undavi"));
    }
}
