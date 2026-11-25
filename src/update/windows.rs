//! Installing a release's installer or zip (Windows), whichever Undavi was installed
//! with.
//!
//! Undavi is installed for one user, into a folder that user can write to, so nothing
//! here asks for administrator rights.

use std::ffi::OsString;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;

/// How release archives call this system.
pub const PLATFORM: &str = "x86_64-windows";
/// The program's file name.
pub const PROGRAM: &str = "undavi.exe";
/// Runs a program without a console window flashing up.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What the installer's name ends with (after the platform).
const SETUP: &str = "-setup.exe";
/// The uninstaller the installer leaves in the prefix.
const UNINSTALLER: &str = "unins000.exe";
/// What `install.ps1` leaves there.
const SCRIPT: &str = "install.ps1";

/// Whether the program belongs to a package manager (none so far).
pub fn is_package(_exe: &Path) -> bool {
    false
}

/// Whether `<prefix>\bin\undavi.exe` really is an install: Windows has no folder that's
/// just for installed programs.
pub fn is_install(prefix: &Path) -> bool {
    has_uninstaller(prefix) || prefix.join(SCRIPT).is_file()
}

fn has_uninstaller(prefix: &Path) -> bool {
    prefix.join(UNINSTALLER).is_file()
}

/// What the file to download ends with: the installer for what it installed, and the
/// zip for the rest.
pub fn archive_ending(prefix: Option<&Path>) -> &'static str {
    if prefix.is_some_and(has_uninstaller) {
        SETUP
    } else {
        ".zip"
    }
}

/// A file's SHA-256, in lowercase hex.
pub fn sha256(file: &Path) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let mut hash = [0u8; 32];
    // SAFETY: a SHA-256 is 32 bytes.
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, &bytes, &mut hash) };
    status.ok().map_err(|e| e.to_string())?;
    Ok(hash.iter().map(|b| format!("{b:02x}")).collect())
}

/// A program that comes with Windows. (Not through `PATH`, which can miss the system's
/// folders or have others first.)
fn system_program(path: &str) -> Result<PathBuf, String> {
    let mut buffer = [0u16; 260];
    // SAFETY: the buffer is as long as it says.
    let len = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    match buffer.get(..len) {
        Some(dir) if len > 0 => Ok(PathBuf::from(String::from_utf16_lossy(dir)).join(path)),
        _ => Err("couldn't find the system folder".into()),
    }
}

/// Run the installer, or unpack the zip and run the install script in it (in its folder
/// `name`).
pub fn run_installer(
    archive: &Path,
    _sha256: &str,
    name: &str,
    prefix: &Path,
    read_only: bool,
) -> Result<(), String> {
    if read_only {
        return Err(format!("{} can't be written to", prefix.display()));
    }
    if archive.extension() == Some("exe".as_ref()) {
        run_setup(archive, prefix)
    } else {
        run_script(archive, name, prefix)
    }
}

fn run_setup(setup: &Path, prefix: &Path) -> Result<(), String> {
    let status = Command::new(setup)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"])
        .arg(setup_folder(prefix))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("couldn't run the installer: {e}"))?;
    match status.code() {
        Some(0) => Ok(()),
        // It has no output to show.
        Some(code) => Err(format!("the installer failed (code {code})")),
        None => Err("the installer failed".into()),
    }
}

/// The installer's option for where to.
fn setup_folder(prefix: &Path) -> OsString {
    let mut option = OsString::from("/DIR=");
    option.push(prefix);
    option
}

fn run_script(archive: &Path, name: &str, prefix: &Path) -> Result<(), String> {
    let work = archive.parent().ok_or("no folder to unpack to")?;
    run(Command::new(system_program("tar.exe")?)
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(work))
    .map_err(|e| format!("couldn't unpack the download: {e}"))?;
    let script = work.join(name).join(SCRIPT);
    if !script.is_file() {
        return Err("the download has no install script".into());
    }
    let powershell = system_program(r"WindowsPowerShell\v1.0\powershell.exe")?;
    run(Command::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .arg("-Prefix")
        .arg(prefix))
    .map_err(|e| format!("the installer failed: {e}"))
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("couldn't run {:?}: {e}", cmd.get_program()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Remove the version an update replaced: it was running then, so the install script
/// could only move it out of the way.
pub fn clean_up(prefix: &Path) {
    for file in std::fs::read_dir(prefix.join("bin"))
        .into_iter()
        .flatten()
        .flatten()
    {
        if file
            .file_name()
            .to_string_lossy()
            .starts_with("undavi.exe.old")
        {
            let _ = std::fs::remove_file(file.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip with this install script.
    fn archive(dir: &Path, name: &str, script: &str) -> PathBuf {
        let root = dir.join("from").join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("install.ps1"), script).unwrap();
        let archive = dir.join(format!("{name}.zip"));
        run(Command::new(system_program("tar.exe").unwrap())
            .args(["-a", "-cf"])
            .arg(&archive)
            .arg("-C")
            .arg(dir.join("from"))
            .arg(name))
        .unwrap();
        archive
    }

    #[test]
    fn the_install_script_runs_with_the_prefix() {
        let dir = std::env::temp_dir().join(format!("undavi update {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prefix = dir.join("a prefix");
        std::fs::create_dir_all(&prefix).unwrap();

        let script = "param($Prefix)\nSet-Content -LiteralPath \"$Prefix\\installed\" $Prefix\n";
        let zip = archive(&dir, "new", script);
        run_installer(&zip, "", "new", &prefix, false).unwrap();
        let noted = std::fs::read_to_string(prefix.join("installed")).unwrap();
        assert_eq!(noted.trim(), prefix.to_str().unwrap());

        // No script, one that fails, and something that isn't a zip.
        let err = run_installer(&zip, "", "other", &prefix, false).unwrap_err();
        assert!(err.contains("no install script"), "{err}");
        let zip = archive(
            &dir,
            "fails",
            "[Console]::Error.WriteLine('no luck')\nexit 1\n",
        );
        let err = run_installer(&zip, "", "fails", &prefix, false).unwrap_err();
        assert_eq!(err, "the installer failed: no luck");
        let broken = dir.join("broken.zip");
        std::fs::write(&broken, "not a zip").unwrap();
        let err = run_installer(&broken, "", "broken", &prefix, false).unwrap_err();
        assert!(err.contains("unpack"), "{err}");
        let err = run_installer(&zip, "", "fails", &prefix, true).unwrap_err();
        assert!(err.contains("can't be written to"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_kind_of_install_gets_its_own_kind_of_update() {
        let prefix = std::env::temp_dir().join(format!("undavi kinds {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&prefix);
        std::fs::create_dir_all(&prefix).unwrap();
        assert!(!is_install(&prefix));
        std::fs::write(prefix.join(SCRIPT), "").unwrap();
        assert!(is_install(&prefix));
        assert_eq!(archive_ending(Some(&prefix)), ".zip");
        std::fs::remove_file(prefix.join(SCRIPT)).unwrap();
        std::fs::write(prefix.join(UNINSTALLER), "").unwrap();
        assert!(is_install(&prefix));
        assert_eq!(archive_ending(Some(&prefix)), "-setup.exe");
        assert_eq!(archive_ending(None), ".zip");
        let _ = std::fs::remove_dir_all(&prefix);
    }

    #[test]
    fn the_installer_runs_quietly_and_reports_failures() {
        let folder = setup_folder(Path::new(r"C:\Some where\Undavi"));
        assert_eq!(folder, r"/DIR=C:\Some where\Undavi");

        let dir = std::env::temp_dir().join(format!("undavi setup {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Stands in for an installer: a program that doesn't know these options.
        let setup = dir.join("undavi-setup.exe");
        std::fs::copy(system_program("tar.exe").unwrap(), &setup).unwrap();
        let err = run_installer(&setup, "", "x", &dir, false).unwrap_err();
        assert!(err.starts_with("the installer failed (code "), "{err}");
        std::fs::write(&setup, "not a program").unwrap();
        let err = run_installer(&setup, "", "x", &dir, false).unwrap_err();
        assert!(err.contains("couldn't run the installer"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hashes_files() {
        let file = std::env::temp_dir().join(format!("undavi hash {}", std::process::id()));
        std::fs::write(&file, "abc").unwrap();
        assert_eq!(
            sha256(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::write(&file, "").unwrap();
        assert_eq!(
            sha256(&file).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let _ = std::fs::remove_file(&file);
        assert!(sha256(&file).is_err());
    }
}
