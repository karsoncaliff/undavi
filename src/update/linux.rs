//! Installing a release's tarball (Linux).

use std::path::Path;
use std::process::Command;

/// How release archives call this system.
pub const PLATFORM: &str = "x86_64-linux";
/// The program's file name.
pub const PROGRAM: &str = "undavi";

/// Whether the program belongs to a package manager (pacman).
pub fn is_package(exe: &Path) -> bool {
    Command::new("pacman")
        .arg("-Qqo")
        .arg(exe)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Whether `<prefix>/bin/undavi` really is an install.
pub fn is_install(_prefix: &Path) -> bool {
    true
}

/// What the file to download ends with.
pub fn archive_ending(_prefix: Option<&Path>) -> &'static str {
    ".tar.gz"
}

/// A file's SHA-256, in lowercase hex.
pub fn sha256(file: &Path) -> Result<String, String> {
    let sum = run(Command::new("sha256sum").arg(file))?;
    Ok(sum.split_whitespace().next().unwrap_or("").to_string())
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
pub fn run_installer(
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A tarball whose install script notes its prefix in `<prefix>/installed`.
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
}
