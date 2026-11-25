//! Capturing single apps through WASAPI, and their volume (Windows).
//!
//! Every app that plays audio has sessions in the Windows audio engine: one for each
//! process and output device. cpal can't record a session, so this module uses
//! WASAPI's process loopback directly: one client for each process that plays the
//! wanted app's audio, whatever device it plays on, added together. The sessions are
//! listed again every [`RESCAN`], so an app is followed as it starts, stops or moves
//! its playback to another process (browsers).
//!
//! Apps are identified by their name (see [`App`]), not by process, so a choice
//! survives the app being restarted.

use super::{PlayingApp, Target, player_matches, squash};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{
    ERROR_NO_MORE_FILES, ERROR_SUCCESS, HANDLE, PROPERTYKEY, S_OK, WAIT_OBJECT_0,
};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    ActivateAudioInterfaceAsync, AudioSessionStateExpired, DEVICE_STATE_ACTIVE,
    IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume,
    MMDeviceEnumerator, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
    VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX, eRender,
};
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{
    CreateEventW, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW, SetEvent, WaitForSingleObject,
};
use windows::Win32::System::Variant::VT_BLOB;
use windows::Win32::UI::Shell::{
    FOLDERID_AppsFolder, IShellItem2, KF_FLAG_DONT_VERIFY, SHCreateItemFromParsingName,
    SHCreateItemInKnownFolder, SIGDN_NORMALDISPLAY,
};
use windows::core::{
    Error, GUID, HRESULT, HSTRING, IUnknown, Interface, Owned, PWSTR, Ref, Result,
};

/// App captures always deliver this rate; Windows resamples what plays at another one.
pub const RATE: u32 = 48_000;
/// How long listing the playing apps may take before giving up.
const LIST_TIMEOUT: Duration = Duration::from_millis(500);
/// How often an app capture looks for the app's sessions.
const RESCAN: Duration = Duration::from_millis(500);
/// How long Windows may take to open a capture.
const OPEN_TIMEOUT: Duration = Duration::from_secs(2);
/// The first Windows build with process loopback: Windows 10 version 2004.
const FIRST_BUILD: u32 = 19041;
/// How far an app's processes may drift apart before the ones behind count as silent:
/// 100 ms of left/right pairs.
const MAX_DRIFT: usize = 2 * RATE as usize / 10;

/// The "File description" of an executable.
const FILE_DESCRIPTION: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x0CEF7D53_FA64_11D1_A203_0000F81FEDEE),
    pid: 3,
};
/// What a Start menu entry starts.
const LINK_TARGET: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xB9B4B3FC_2B51_4A42_B5D8_324146AFCF25),
    pid: 2,
};

/// Whether this version of Windows can capture single apps.
pub fn supported() -> bool {
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: the struct says how big it is.
    let status = unsafe { RtlGetVersion(&mut version) };
    status.is_ok() && version.dwBuildNumber >= FIRST_BUILD
}

/// COM, for the thread that holds this. (Not `Send`: it ends where it began.)
pub(crate) struct Com(bool, PhantomData<*const ()>);

impl Com {
    /// Set COM up for a thread of our own (multithreaded).
    pub(crate) fn init() -> Self {
        // SAFETY: undone when dropped, on the same thread.
        let started = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        Self(started, PhantomData)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: matches the successful call in `init`.
            unsafe { CoUninitialize() }
        }
    }
}

// ---------------------------------------------------------------------------
// Apps

/// The app behind a process that plays audio.
#[derive(Clone, Debug, PartialEq, Eq)]
struct App {
    /// Its name: "Spotify", "Google Chrome".
    label: String,
    /// Its executable without the extension: "Spotify", "chrome".
    exe: String,
}

impl App {
    /// Identifies the app across restarts.
    fn key(&self) -> String {
        self.label.to_lowercase()
    }

    /// Whether this is the player itself, and not just something called like it.
    fn is(&self, bus: &str, identity: &str) -> bool {
        let same = |a: &str, b: &str| !a.is_empty() && a == b;
        same(&squash(&self.exe), bus) || same(&squash(&self.label), &squash(identity))
    }

    fn names(&self) -> Vec<String> {
        vec![self.label.clone(), self.exe.clone()]
    }
}

/// Which of these apps `target` means.
fn wanted(target: &Target, apps: &[App]) -> Vec<bool> {
    let which = |test: &dyn Fn(&App) -> bool| apps.iter().map(test).collect::<Vec<_>>();
    match target {
        Target::None => which(&|_| false),
        Target::App(key) => which(&|a| a.key() == *key),
        Target::Player { bus, identity } => {
            let exactly = which(&|a| a.is(bus, identity));
            if exactly.contains(&true) {
                exactly
            } else {
                which(&|a| player_matches(bus, identity, &a.names()))
            }
        }
    }
}

/// `pids` without the ones started by another one of them (however indirectly):
/// capturing a process includes what it started.
fn outermost(pids: &BTreeSet<u32>, parents: &HashMap<u32, u32>) -> BTreeSet<u32> {
    let inside = |pid: u32| {
        let mut up = pid;
        // Process ids get reused, so in theory this could go in circles.
        for _ in 0..parents.len() {
            match parents.get(&up) {
                Some(&parent) if parent != 0 && parent != pid => {
                    if pids.contains(&parent) {
                        return true;
                    }
                    up = parent;
                }
                _ => break,
            }
        }
        false
    };
    pids.iter().copied().filter(|&pid| !inside(pid)).collect()
}

/// Every process's parent.
fn parents() -> HashMap<u32, u32> {
    let mut out = HashMap::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: the entry says how big it is, and the snapshot is closed when dropped.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let snapshot = Owned::new(snapshot);
        let mut next = Process32FirstW(*snapshot, &mut entry);
        while next.is_ok() {
            out.insert(entry.th32ProcessID, entry.th32ParentProcessID);
            next = Process32NextW(*snapshot, &mut entry);
        }
        debug_assert!(next.is_err_and(|e| e.code() == ERROR_NO_MORE_FILES.to_hresult()));
    }
    out
}

/// Take a string Windows allocated.
fn take(s: PWSTR) -> Option<String> {
    // SAFETY: called with what a shell function returned: a terminated string that's
    // ours to free.
    unsafe {
        let text = s.to_string().ok();
        CoTaskMemFree(Some(s.0.cast()));
        text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
    }
}

fn property(item: &IShellItem2, key: &PROPERTYKEY) -> Option<String> {
    // SAFETY: plain calls; `take` frees the string.
    unsafe { item.GetString(key).ok().and_then(take) }
}

/// An executable's name without the extension.
fn stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// What the Start menu knows about the app with this app ID: its name, and the
/// executable it starts (not for packaged apps).
fn start_menu(app_id: &str) -> Option<(String, Option<String>)> {
    // SAFETY: plain calls; `take` frees the strings.
    unsafe {
        let item: IShellItem2 = SHCreateItemInKnownFolder(
            &FOLDERID_AppsFolder,
            KF_FLAG_DONT_VERIFY,
            &HSTRING::from(app_id),
        )
        .ok()?;
        let name = item
            .GetDisplayName(SIGDN_NORMALDISPLAY)
            .ok()
            .and_then(take)?;
        Some((name, property(&item, &LINK_TARGET)))
    }
}

/// The app's name, from the Start menu for packaged apps and from the executable for
/// the rest.
fn label(app_id: Option<&str>, exe: &str) -> String {
    /// Names already looked up, by app ID or executable.
    static KNOWN: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

    let mut known = KNOWN.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(label) = known.get(app_id.unwrap_or(exe)) {
        return label.clone();
    }
    let described = || {
        // SAFETY: a plain call.
        let item = unsafe { SHCreateItemFromParsingName(&HSTRING::from(exe), None) };
        property(&item.ok()?, &FILE_DESCRIPTION)
    };
    let label = app_id
        .and_then(|id| start_menu(id).map(|(name, _)| name))
        .or_else(described)
        .unwrap_or_else(|| stem(exe));
    known.insert(app_id.unwrap_or(exe).to_string(), label.clone());
    label
}

/// The app a process belongs to.
fn app_of(pid: u32) -> Option<App> {
    let mut buffer = [0u16; 1024];
    // SAFETY: the buffer is as long as it says, and the process is closed when dropped.
    let (exe, app_id) = unsafe {
        let process = Owned::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
        let text = PWSTR(buffer.as_mut_ptr());
        let mut len = buffer.len() as u32;
        QueryFullProcessImageNameW(*process, PROCESS_NAME_WIN32, text, &mut len).ok()?;
        let exe = String::from_utf16_lossy(&buffer[..len as usize]);
        // Only packaged apps have one here.
        let mut len = buffer.len() as u32;
        let found = GetApplicationUserModelId(*process, &mut len, Some(text)) == ERROR_SUCCESS;
        let app_id = found.then(|| text.to_string().ok()).flatten();
        (exe, app_id)
    };
    Some(App {
        label: label(app_id.as_deref(), &exe),
        exe: stem(&exe),
    })
}

/// The names of the player with this app ID, the way [`Target::Player`] takes them:
/// its executable's name (or what's closest to it) squashed, and its display name.
pub(crate) fn player_names(app_id: &str) -> (String, String) {
    // Without an app ID of its own, an app is identified by its executable.
    if let Some(exe) = app_id.strip_suffix(".exe") {
        let exe = stem(exe);
        return (squash(&exe), exe);
    }
    match start_menu(app_id) {
        Some((name, target)) => {
            let exe = target
                .filter(|t| t.to_lowercase().ends_with(".exe"))
                .map(|t| squash(&stem(&t)))
                // (Some apps start through their updater.)
                .filter(|exe| !exe.is_empty() && exe != "update");
            (exe.unwrap_or_else(|| squash(&name)), name)
        }
        None => (squash(app_id), String::new()),
    }
}

/// What one app plays on one output device.
struct Session {
    pid: u32,
    app: App,
    volume: ISimpleAudioVolume,
}

/// Every session with a playback stream (playing or not), other than the system's
/// own sounds.
fn sessions() -> Result<Vec<Session>> {
    let mut out = Vec::new();
    // SAFETY: plain COM calls, on a thread that holds a `Com`.
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = devices.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        for d in 0..devices.GetCount()? {
            // Devices and sessions can go away at any moment.
            let list = devices
                .Item(d)
                .and_then(|device| device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None))
                .and_then(|manager| manager.GetSessionEnumerator());
            let Ok(list) = list else { continue };
            for s in 0..list.GetCount().unwrap_or(0) {
                let session = list
                    .GetSession(s)
                    .and_then(|s| s.cast::<IAudioSessionControl2>());
                let Ok(session) = session else { continue };
                let pid = session.GetProcessId().unwrap_or(0);
                let ours = pid == std::process::id();
                if pid == 0
                    || ours
                    || session.IsSystemSoundsSession() == S_OK
                    || session.GetState() == Ok(AudioSessionStateExpired)
                {
                    continue;
                }
                let Some(app) = app_of(pid) else { continue };
                // The audio engine has one for each loopback, ours included.
                if app.exe.eq_ignore_ascii_case("audiodg") {
                    continue;
                }
                if let Ok(volume) = session.cast() {
                    out.push(Session { pid, app, volume });
                }
            }
        }
    }
    Ok(out)
}

/// The sessions `target` means.
fn sessions_of(target: &Target) -> Result<Vec<Session>> {
    if *target == Target::None {
        return Ok(Vec::new());
    }
    let sessions = sessions()?;
    let apps: Vec<App> = sessions.iter().map(|s| s.app.clone()).collect();
    let wanted = wanted(target, &apps);
    Ok(sessions
        .into_iter()
        .zip(wanted)
        .filter_map(|(s, wanted)| wanted.then_some(s))
        .collect())
}

/// The apps playing (or holding a playback stream open) right now, sorted by name.
pub(super) fn playing_apps() -> Vec<PlayingApp> {
    // On a thread of its own: the UI's has COM set up differently, and shouldn't wait
    // for long.
    let (tx, rx) = mpsc::channel();
    let listing = std::thread::Builder::new()
        .name("undavi-app-list".into())
        .spawn(move || {
            let _com = Com::init();
            let mut apps: Vec<PlayingApp> = Vec::new();
            for session in sessions().unwrap_or_default() {
                let key = session.app.key();
                if !apps.iter().any(|a| a.key == key) {
                    let label = session.app.label;
                    apps.push(PlayingApp { key, label });
                }
            }
            apps.sort_by_key(|a| a.label.to_lowercase());
            let _ = tx.send(apps);
        });
    if listing.is_err() {
        return Vec::new();
    }
    rx.recv_timeout(LIST_TIMEOUT).unwrap_or_default()
}

/// The volume of a player's app, as in the Windows volume mixer.
pub(crate) struct AppVolume(Vec<ISimpleAudioVolume>);

impl AppVolume {
    /// The volume of the app that is this player (names as in [`player_names`]). Call
    /// it on a thread that holds a [`Com`].
    pub(crate) fn of_player(bus: &str, identity: &str) -> Self {
        let target = Target::Player {
            bus: bus.into(),
            identity: identity.into(),
        };
        let sessions = sessions_of(&target).unwrap_or_default();
        Self(sessions.into_iter().map(|s| s.volume).collect())
    }

    /// 0.0..=1.0, or `None` while the app has no playback stream.
    pub(crate) fn get(&self) -> Option<f64> {
        // SAFETY: plain COM calls.
        let volume = |v: &ISimpleAudioVolume| unsafe {
            let muted = v.GetMute().ok()?.as_bool();
            let volume = v.GetMasterVolume().ok()?;
            Some(if muted { 0.0 } else { f64::from(volume) })
        };
        self.0.iter().filter_map(volume).reduce(f64::max)
    }

    pub(crate) fn set(&self, volume: f64) -> Result<()> {
        for v in &self.0 {
            // SAFETY: plain COM calls.
            unsafe {
                v.SetMasterVolume(volume.clamp(0.0, 1.0) as f32, std::ptr::null())?;
                if volume > 0.0 {
                    v.SetMute(false, std::ptr::null())?;
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Capturing

/// An event to wait for, set from other threads and by Windows.
struct Event(Owned<HANDLE>);

// SAFETY: events are made to be used from several threads.
unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
    fn new() -> Result<Self> {
        // SAFETY: a plain call; closed when dropped.
        unsafe { Ok(Self(Owned::new(CreateEventW(None, false, false, None)?))) }
    }

    fn set(&self) {
        // SAFETY: the event is open as long as `self` is.
        let _ = unsafe { SetEvent(*self.0) };
    }

    /// Wait until it's set, at most `timeout`.
    fn wait(&self, timeout: Duration) -> bool {
        // SAFETY: as in `set`.
        unsafe { WaitForSingleObject(*self.0, timeout.as_millis() as u32) == WAIT_OBJECT_0 }
    }
}

/// Passes on what Windows answered to `ActivateAudioInterfaceAsync`.
#[windows::core::implement(IActivateAudioInterfaceCompletionHandler)]
struct Activated(mpsc::Sender<Result<IAudioClient>>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(
        &self,
        operation: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> Result<()> {
        let mut result = HRESULT(0);
        let mut client: Option<IUnknown> = None;
        let answer = operation.ok().and_then(|operation| {
            // SAFETY: plain out parameters.
            unsafe { operation.GetActivateResult(&mut result, &mut client)? };
            result.ok()?;
            client.as_ref().ok_or_else(Error::empty)?.cast()
        });
        let _ = self.0.send(answer);
        Ok(())
    }
}

/// What a process (and those it started) plays, being recorded.
struct Stream {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    /// Interleaved left/right samples that haven't been mixed yet.
    pending: VecDeque<f32>,
}

impl Stream {
    /// Start recording; `arrived` is set whenever there's something to read.
    fn open(pid: u32, arrived: &Event) -> Result<Self> {
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            ..Default::default()
        };
        params.Anonymous.ProcessLoopbackParams = AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
            TargetProcessId: pid,
            ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
        };
        // Not dropped: that would free the blob, which is `params`.
        let mut blob = ManuallyDrop::new(PROPVARIANT::default());
        // SAFETY: an empty PROPVARIANT is this variant of the union.
        let value = unsafe { &mut *blob.Anonymous.Anonymous };
        value.vt = VT_BLOB;
        value.Anonymous.blob = BLOB {
            cbSize: size_of_val(&params) as u32,
            pBlobData: (&raw mut params).cast(),
        };

        let (tx, rx) = mpsc::channel();
        let handler: IActivateAudioInterfaceCompletionHandler = Activated(tx).into();
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
            nChannels: 2,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * 8,
            nBlockAlign: 8,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        // SAFETY: `params`, `blob` and the format outlive the calls that read them, and
        // the event outlives the stream (see `run`).
        unsafe {
            let _operation = ActivateAudioInterfaceAsync(
                VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
                &IAudioClient::IID,
                Some(&*blob),
                &handler,
            )?;
            let client = rx
                .recv_timeout(OPEN_TIMEOUT)
                .map_err(|_| Error::empty())??;
            // Stereo f32 at a fixed rate; Windows converts whatever the app plays.
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                // 200 ms, in units of 100 ns.
                2_000_000,
                0,
                &format,
                None,
            )?;
            client.SetEventHandle(*arrived.0)?;
            let capture = client.GetService()?;
            client.Start()?;
            Ok(Self {
                client,
                capture,
                pending: VecDeque::new(),
            })
        }
    }

    /// Take in everything that has arrived.
    fn read(&mut self) -> Result<()> {
        // SAFETY: a packet is `frames` pairs of f32 (the format asked for), and is
        // ours until it's released.
        unsafe {
            while self.capture.GetNextPacketSize()? > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0, 0);
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let samples = 2 * frames as usize;
                if data.is_null() || flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                    self.pending.extend(std::iter::repeat_n(0.0, samples));
                } else {
                    let data = data.cast::<f32>();
                    self.pending
                        .extend(std::slice::from_raw_parts(data, samples));
                }
                self.capture.ReleaseBuffer(frames)?;
            }
        }
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: a plain COM call.
        let _ = unsafe { self.client.Stop() };
    }
}

/// Add up what's pending from every process into `out`, in step. They all deliver
/// steadily, silence included, so that's as much as every one of them has (but a
/// stuck one doesn't hold the others up for more than [`MAX_DRIFT`]).
fn mix<'a>(inputs: impl Iterator<Item = &'a mut VecDeque<f32>>, out: &mut Vec<f32>) {
    let mut inputs: Vec<_> = inputs.collect();
    let lengths = inputs.iter().map(|i| i.len());
    let shortest = lengths.clone().min().unwrap_or(0);
    let longest = lengths.max().unwrap_or(0);
    // Only whole pairs.
    let ready = shortest.max(longest.saturating_sub(MAX_DRIFT)) & !1;
    out.clear();
    out.resize(ready, 0.0);
    for input in &mut inputs {
        let n = ready.min(input.len());
        for (sum, sample) in out.iter_mut().zip(input.drain(..n)) {
            *sum += sample;
        }
    }
}

enum Command {
    Target(Target),
    Stop,
}

/// A running capture of one app (or the player's app). Dropping it stops it.
pub(super) struct AppCapture {
    commands: mpsc::Sender<Command>,
    wake: Arc<Event>,
    linked: Arc<AtomicUsize>,
    target: Target,
    thread: Option<JoinHandle<()>>,
}

impl AppCapture {
    /// Start capturing `target`. `push` receives interleaved left/right pairs at
    /// [`RATE`] on the capture thread; `dead` is set if the audio service goes away.
    pub(super) fn start(
        target: Target,
        push: impl FnMut(&[f32]) + Send + 'static,
        dead: Arc<AtomicBool>,
    ) -> std::result::Result<Self, String> {
        if !supported() {
            return Err("capturing single apps needs a newer version of Windows".into());
        }
        let (commands, receiver) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let linked = Arc::new(AtomicUsize::new(0));
        let wake = Arc::new(Event::new().map_err(|e| e.to_string())?);
        let thread = {
            let (target, linked, wake) = (target.clone(), linked.clone(), wake.clone());
            std::thread::Builder::new()
                .name("undavi-app-capture".into())
                .spawn(move || {
                    let _com = Com::init();
                    if let Err(e) = run(target, push, &linked, &receiver, &wake, &ready_tx) {
                        // Either it never got going, or the app opens a new one.
                        let _ = ready_tx.send(Err(e.to_string()));
                        dead.store(true, Ordering::Relaxed);
                    }
                })
                .map_err(|e| e.to_string())?
        };
        match ready_rx.recv_timeout(2 * OPEN_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                commands,
                wake,
                linked,
                target,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Windows didn't answer".into()),
        }
    }

    /// Follow something else (for "Music player", when the player changes).
    pub(super) fn set_target(&mut self, target: Target) {
        if target != self.target {
            self.target = target.clone();
            let _ = self.commands.send(Command::Target(target));
            self.wake.set();
        }
    }

    /// How many of the target's sessions are being captured.
    pub(super) fn linked(&self) -> usize {
        self.linked.load(Ordering::Relaxed)
    }
}

impl Drop for AppCapture {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        self.wake.set();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The capture thread: a stream for each process of the target that plays audio.
fn run(
    mut target: Target,
    mut push: impl FnMut(&[f32]),
    linked: &AtomicUsize,
    commands: &mpsc::Receiver<Command>,
    wake: &Event,
    ready: &mpsc::Sender<std::result::Result<(), String>>,
) -> Result<()> {
    let mut streams: BTreeMap<u32, Stream> = BTreeMap::new();
    let mut mixed = Vec::new();
    rescan(&target, &mut streams, linked, wake)?;
    let _ = ready.send(Ok(()));
    let mut next_scan = Instant::now() + RESCAN;
    loop {
        wake.wait(next_scan.saturating_duration_since(Instant::now()));
        loop {
            match commands.try_recv() {
                Ok(Command::Target(new)) => {
                    target = new;
                    next_scan = Instant::now();
                }
                Ok(Command::Stop) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if Instant::now() >= next_scan {
            rescan(&target, &mut streams, linked, wake)?;
            next_scan = Instant::now() + RESCAN;
        }
        // One that fails is opened again by the next scan (or that fails too).
        streams.retain(|_, stream| stream.read().is_ok());
        mix(streams.values_mut().map(|s| &mut s.pending), &mut mixed);
        if !mixed.is_empty() {
            push(&mixed);
        }
    }
}

/// Make the streams match what the target plays now.
fn rescan(
    target: &Target,
    streams: &mut BTreeMap<u32, Stream>,
    linked: &AtomicUsize,
    wake: &Event,
) -> Result<()> {
    let sessions = sessions_of(target)?;
    let mut pids: BTreeSet<u32> = sessions.iter().map(|s| s.pid).collect();
    if pids.len() > 1 {
        pids = outermost(&pids, &parents());
    }
    streams.retain(|pid, _| pids.contains(pid));
    for pid in pids {
        if let std::collections::btree_map::Entry::Vacant(entry) = streams.entry(pid) {
            entry.insert(Stream::open(pid, wake)?);
        }
    }
    linked.store(sessions.len(), Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(label: &str, exe: &str) -> App {
        App {
            label: label.into(),
            exe: exe.into(),
        }
    }

    fn player(bus: &str, identity: &str) -> Target {
        Target::Player {
            bus: bus.into(),
            identity: identity.into(),
        }
    }

    #[test]
    fn targets_pick_their_apps() {
        let apps = [
            app("Spotify", "Spotify"),
            app("Google Chrome", "chrome"),
            app("VLC media player", "vlc"),
        ];
        let none = [false, false, false];
        assert_eq!(wanted(&Target::None, &apps), none);
        let chrome = Target::App("google chrome".into());
        assert_eq!(wanted(&chrome, &apps), [false, true, false]);
        assert_eq!(
            wanted(&player("chrome", "Google Chrome"), &apps),
            [false, true, false]
        );
        assert_eq!(
            wanted(&player("spotify", "Spotify"), &apps),
            [true, false, false]
        );
        // A web app: started by chrome_proxy.exe, played by chrome.exe.
        assert_eq!(
            wanted(&player("chromeproxy", "Some Site"), &apps),
            [false, true, false]
        );
        assert_eq!(wanted(&player("foobar2000", "foobar2000"), &apps), none);
        assert_eq!(wanted(&player("", ""), &apps), none);
    }

    #[test]
    fn the_player_itself_comes_before_similar_names() {
        let media_player = player("mediaplayer", "Media Player");
        let apps = [
            app("Media Player", "Microsoft.Media.Player"),
            app("VLC media player", "vlc"),
        ];
        assert_eq!(wanted(&media_player, &apps), [true, false]);
        // Every process of the player, whatever it's called.
        let apps = [
            app("Apple Music", "AppleMusic"),
            app("Apple Music", "AMPLibraryAgent"),
        ];
        let target = player("applemusic", "Apple Music");
        assert_eq!(wanted(&target, &apps), [true, true]);
    }

    #[test]
    fn an_app_is_captured_once() {
        // 1 started 2, which started 3; 4 stands alone.
        let parents = HashMap::from([(1, 900), (2, 1), (3, 2), (4, 900)]);
        let of = |pids: &[u32]| outermost(&pids.iter().copied().collect(), &parents);
        assert_eq!(of(&[1, 3, 4]), BTreeSet::from([1, 4]));
        assert_eq!(of(&[2, 3]), BTreeSet::from([2]));
        assert_eq!(of(&[3, 4]), BTreeSet::from([3, 4]));
        // Ids that were reused can make a circle.
        let circle = HashMap::from([(5, 6), (6, 7), (7, 6)]);
        let alone = BTreeSet::from([5]);
        assert_eq!(outermost(&alone, &circle), alone);
    }

    #[test]
    fn processes_are_mixed_in_step() {
        let mut out = vec![9.0];
        let mut a = VecDeque::from([0.1, 0.2, 0.3, 0.4]);
        let mut b = VecDeque::from([0.5, 0.5, 0.5]);
        mix([&mut a, &mut b].into_iter(), &mut out);
        assert_eq!(out, [0.6, 0.7]);
        assert_eq!((a.len(), b.len()), (2, 1));
        // Nothing yet from one of them: wait for it...
        b.clear();
        mix([&mut a, &mut b].into_iter(), &mut out);
        assert!(out.is_empty());
        assert_eq!(a.len(), 2);
        // ...but not forever.
        a.extend(std::iter::repeat_n(0.25, MAX_DRIFT));
        mix([&mut a, &mut b].into_iter(), &mut out);
        assert_eq!(out, [0.3, 0.4]);
        assert_eq!(a.len(), MAX_DRIFT);
        mix(std::iter::empty(), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn players_without_an_app_id_go_by_their_executable() {
        let (bus, name) = player_names("Spotify.exe");
        assert_eq!((bus.as_str(), name.as_str()), ("spotify", "Spotify"));
        // Unknown to the Start menu.
        let (bus, name) = player_names("No.Such.App_1234567890");
        assert_eq!((bus.as_str(), name.as_str()), ("nosuchapp1234567890", ""));
    }
}
