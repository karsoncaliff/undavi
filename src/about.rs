//! What the About panel shows: version, credits and the licenses of what Undavi bundles.

use iced::widget::image::Handle;
use iced::widget::svg;
use std::sync::LazyLock;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How Undavi identifies itself to web services (LRCLIB asks apps to name
/// themselves, their version and a homepage): `Undavi/0.9.0 (+https://…)`, with
/// the `repository` from Cargo.toml, or a short description until one is set.
pub fn user_agent() -> String {
    match env!("CARGO_PKG_REPOSITORY") {
        "" => format!("Undavi/{VERSION} (desktop music visualizer)"),
        repo => format!("Undavi/{VERSION} (+{repo})"),
    }
}

/// The "undavi" wordmark (header and About panel), 47 × 10.
///
/// The wave's rounded ends and the top of its hook touch the edge of the file's
/// 47 × 10 canvas and would be cut off flat, so the canvas is widened by one unit
/// on every side.
pub static TITLE: LazyLock<svg::Handle> = LazyLock::new(|| {
    let source = include_str!("../assets/undavi/title.svg");
    let padded = source.replacen(
        r#"width="47" height="10" viewBox="0 0 47 10""#,
        r#"width="49" height="12" viewBox="-1 -1 49 12""#,
        1,
    );
    debug_assert_ne!(
        padded, source,
        "title.svg's canvas changed; update the padding"
    );
    svg::Handle::from_memory(padded.into_bytes())
});
pub const TITLE_ASPECT: f32 = 49.0 / 12.0;

/// The app icon, decoded once.
pub static ICON: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_bytes(&include_bytes!("../assets/icons/png/undavi-128.png")[..]));

/// Someone whose work Undavi relies on at runtime.
pub struct Credit {
    pub title: &'static str,
    pub detail: &'static str,
    /// Link label and address.
    pub link: Option<(&'static str, &'static str)>,
}

pub const CREDITS: &[Credit] = &[
    Credit {
        title: "Lyrics",
        detail: "From LRCLIB, a free, community-made lyrics database. Fetched while a song \
                 plays and never saved.",
        link: Some(("lrclib.net", "https://lrclib.net")),
    },
    Credit {
        title: "Track info and album art",
        detail: "From your music player, over MPRIS.",
        link: None,
    },
    Credit {
        title: "Open-source libraries",
        detail: "Built with iced, cpal and about 400 other Rust crates. Their licenses \
                 are installed with Undavi as THIRD-PARTY-LICENSES.txt.",
        link: Some((
            "All licenses",
            "https://github.com/karsoncaliff/undavi/blob/main/packaging/THIRD-PARTY-LICENSES.txt",
        )),
    },
];

/// Something Undavi ships whose license has to travel with it.
pub struct License {
    pub name: &'static str,
    /// License and copyright, in one line.
    pub summary: &'static str,
    pub text: &'static str,
    pub link: Option<(&'static str, &'static str)>,
}

/// Undavi's own license: the NOTICE (copyright and the GPL's additional terms)
/// followed by the GNU GPL itself.
const APP_LICENSE: &str = concat!(
    include_str!("../NOTICE"),
    "\n\n",
    include_str!("../LICENSE")
);
/// Where the source code is (the GPL's offer of source), from Cargo.toml.
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// Everything Undavi ships under its own license: Undavi itself first, then
/// bundled third-party work.
pub fn licenses() -> Vec<License> {
    let mut out = vec![License {
        name: "Undavi",
        summary: "\u{00A9} 2026 Karson Califf \u{00B7} GNU GPL 3.0 only. Free software, with no warranty.",
        text: APP_LICENSE,
        link: (!REPOSITORY.is_empty()).then_some(("Source code", REPOSITORY)),
    }];
    out.extend([License {
        name: "Inter",
        summary: "SIL Open Font License 1.1 \u{00B7} \u{00A9} 2016 The Inter Project Authors",
        text: include_str!("../assets/fonts/OFL-Inter.txt"),
        link: Some(("rsms.me/inter", "https://rsms.me/inter/")),
    }]);
    out
}

/// Open `url` in the default browser, without waiting for it.
pub fn open_url(url: &str) {
    // Tests drive the real app; never let them launch a browser.
    if cfg!(test) {
        return;
    }
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    match result {
        // Reap the launcher when it exits so it doesn't linger as a zombie.
        Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
        Err(e) => eprintln!("undavi: couldn't open {url}: {e}"),
    }
}
