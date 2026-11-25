//! Capturing single apps through PipeWire (Linux).
//!
//! Every app that plays audio has its own playback stream in PipeWire (a node
//! of class `Stream/Output/Audio`). cpal can't record those, so this module
//! talks to PipeWire directly: it opens one capture stream of its own and links
//! the output ports of every stream that belongs to the wanted app to it.
//! PipeWire mixes several links into one input, so an app with more than one
//! stream (Spotify) or one that makes a new stream for each playback (Firefox)
//! is followed as links come and go.
//!
//! Apps are identified by a key from the stream's properties (see [`app_key`]),
//! not by stream, so a choice survives the app restarting its playback.

use super::player_matches;
use pipewire::{
    self as pw,
    context::ContextRc,
    core::PW_ID_CORE,
    link::Link,
    main_loop::MainLoopRc,
    properties::PropertiesBox,
    spa::{
        param::{
            ParamType,
            audio::{AudioFormat, AudioInfoRaw},
        },
        pod::{Object, Pod, Value, serialize::PodSerializer},
        utils::{Direction, SpaTypes},
    },
    stream::{StreamFlags, StreamRc, StreamState},
    types::ObjectType,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

const STREAM_OUTPUT: &str = "Stream/Output/Audio";
/// App captures always deliver this rate; PipeWire resamples if the graph runs
/// at another one.
pub const RATE: u32 = 48_000;
/// How long listing the playing apps may take before giving up.
const LIST_TIMEOUT: Duration = Duration::from_millis(500);

/// An app with at least one playback stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayingApp {
    /// Stable identifier (see [`app_key`]).
    pub key: String,
    /// Name to show, e.g. "Spotify".
    pub label: String,
}

/// What an app capture records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Nothing (yet).
    None,
    /// Every stream of the app with this key.
    App(String),
    /// Every stream of the app that matches this MPRIS player (see
    /// [`player_matches`]).
    Player {
        /// The player's D-Bus name after `org.mpris.MediaPlayer2.`, e.g.
        /// `spotify`, `firefox.instance_1_23` or `org.kde.elisa`.
        bus: String,
        /// The player's display name, e.g. "Mozilla Firefox".
        identity: String,
    },
}

/// Identifies the app behind a stream: its application name, else its
/// executable, else the node name.
fn app_key(props: &pw::spa::utils::dict::DictRef) -> Option<(String, String)> {
    let name = props
        .get(*pw::keys::APP_NAME)
        .or_else(|| props.get(*pw::keys::APP_PROCESS_BINARY))
        .or_else(|| props.get(*pw::keys::NODE_NAME))?
        .trim();
    (!name.is_empty()).then(|| (name.to_lowercase(), name.to_string()))
}

/// The app's names to match a player against: its key and its executable.
fn app_names(props: &pw::spa::utils::dict::DictRef) -> Vec<String> {
    [
        props.get(*pw::keys::APP_NAME),
        props.get(*pw::keys::APP_PROCESS_BINARY),
    ]
    .into_iter()
    .flatten()
    .map(str::to_string)
    .collect()
}

/// Which of our two inputs (0 = left, 1 = right) a source channel feeds.
/// Left-side channels go left, right-side ones right, the rest (mono, center,
/// LFE, unknown) to both.
fn feeds(source_channel: &str) -> &'static [usize] {
    match source_channel {
        "FL" | "RL" | "SL" | "FLC" | "TFL" | "TRL" | "FLW" => &[0],
        "FR" | "RR" | "SR" | "FRC" | "TFR" | "TRR" | "FRW" => &[1],
        _ => &[0, 1],
    }
}

fn props(pairs: &[(&str, &str)]) -> PropertiesBox {
    let mut p = PropertiesBox::new();
    for (k, v) in pairs {
        p.insert(*k, *v);
    }
    p
}

/// Run `body` on a fresh PipeWire connection on its own thread and wait (at most
/// `timeout`) for what it sends back.
fn roundtrip<T: Send + 'static>(
    timeout: Duration,
    body: impl FnOnce(mpsc::Sender<T>) -> Result<(), pw::Error> + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("undavi-pw-list".into())
        .spawn(move || {
            pw::init();
            let _ = body(tx);
        })
        .ok()?;
    rx.recv_timeout(timeout).ok()
}

/// The apps playing (or holding a playback stream open) right now, sorted by
/// name. Empty when PipeWire isn't running.
pub fn playing_apps() -> Vec<PlayingApp> {
    roundtrip(LIST_TIMEOUT, |tx| {
        let mainloop = MainLoopRc::new(None)?;
        let context = ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;
        let registry = core.get_registry_rc()?;

        let found: Rc<RefCell<Vec<PlayingApp>>> = Rc::default();
        let _registry_listener = registry
            .add_listener_local()
            .global({
                let found = found.clone();
                move |global| {
                    let Some(props) = global.props else { return };
                    if global.type_ != ObjectType::Node
                        || props.get(*pw::keys::MEDIA_CLASS) != Some(STREAM_OUTPUT)
                    {
                        return;
                    }
                    if let Some((key, label)) = app_key(props) {
                        let mut found = found.borrow_mut();
                        if !found.iter().any(|a| a.key == key) {
                            found.push(PlayingApp { key, label });
                        }
                    }
                }
            })
            .register();

        // Once the server has answered everything sent so far, the list is complete.
        let pending = core.sync(0)?;
        let _core_listener = core
            .add_listener_local()
            .done({
                let mainloop = mainloop.clone();
                move |id, seq| {
                    if id == PW_ID_CORE && seq == pending {
                        mainloop.quit();
                    }
                }
            })
            .register();
        mainloop.run();

        let mut apps = found.take();
        apps.sort_by_key(|a| a.label.to_lowercase());
        let _ = tx.send(apps);
        Ok(())
    })
    .unwrap_or_default()
}

/// A playback stream we might capture.
struct StreamNode {
    key: String,
    names: Vec<String>,
}

struct PortInfo {
    node: u32,
    output: bool,
    channel: String,
}

/// What the capture thread knows about the graph.
#[derive(Default)]
struct Graph {
    target: Option<Target>,
    streams: HashMap<u32, StreamNode>,
    ports: HashMap<u32, PortInfo>,
    /// Our capture stream's node, once it exists.
    ours: Option<u32>,
    /// Links we made, by (output port, our input port).
    links: HashMap<(u32, u32), Link>,
}

impl Graph {
    fn wanted(&self, node: &StreamNode) -> bool {
        match &self.target {
            None | Some(Target::None) => false,
            Some(Target::App(key)) => node.key == *key,
            Some(Target::Player { bus, identity }) => player_matches(bus, identity, &node.names),
        }
    }

    /// Make the links match the target: one from every output port of every
    /// wanted stream to our left and/or right input. Returns how many streams
    /// are linked.
    fn relink(&mut self, core: &pw::core::CoreRc) -> usize {
        let Some(ours) = self.ours else { return 0 };
        // Our inputs, left then right (by channel name).
        let mut inputs: Vec<(u32, &str)> = self
            .ports
            .iter()
            .filter(|(_, p)| p.node == ours && !p.output)
            .map(|(&id, p)| (id, p.channel.as_str()))
            .collect();
        inputs.sort_by_key(|(_, ch)| if *ch == "FR" { 1 } else { 0 });
        if inputs.is_empty() {
            return 0;
        }

        let mut wanted = HashSet::new();
        let mut nodes = HashSet::new();
        for (&port, info) in &self.ports {
            let Some(node) = self.streams.get(&info.node) else {
                continue;
            };
            if !info.output || !self.wanted(node) {
                continue;
            }
            for &side in feeds(&info.channel) {
                let input = inputs.get(side).or(inputs.first());
                if let Some(&(input, _)) = input {
                    wanted.insert((port, input, info.node));
                    nodes.insert(info.node);
                }
            }
        }

        self.links
            .retain(|pair, _| wanted.iter().any(|w| (w.0, w.1) == *pair));
        for (output, input, node) in wanted {
            if self.links.contains_key(&(output, input)) {
                continue;
            }
            let link = core.create_object::<Link>(
                "link-factory",
                &props(&[
                    (*pw::keys::LINK_OUTPUT_NODE, &node.to_string()),
                    (*pw::keys::LINK_OUTPUT_PORT, &output.to_string()),
                    (*pw::keys::LINK_INPUT_NODE, &ours.to_string()),
                    (*pw::keys::LINK_INPUT_PORT, &input.to_string()),
                    // Gone with us; never left behind in the graph.
                    (*pw::keys::OBJECT_LINGER, "false"),
                ]),
            );
            match link {
                Ok(link) => {
                    self.links.insert((output, input), link);
                }
                Err(e) => eprintln!("undavi: couldn't link an app stream: {e}"),
            }
        }
        nodes.len()
    }
}

enum Command {
    Target(Target),
    Stop,
}

/// A running capture of one app (or the player's app). Dropping it stops it.
pub struct AppCapture {
    commands: pw::channel::Sender<Command>,
    linked: Arc<AtomicUsize>,
    target: Target,
    thread: Option<JoinHandle<()>>,
}

impl AppCapture {
    /// Start capturing `target`. `push` receives interleaved left/right pairs at
    /// [`RATE`] on the capture thread; `dead` is set if PipeWire goes away.
    pub fn start(
        target: Target,
        push: impl FnMut(&[f32]) + Send + 'static,
        dead: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let (commands, receiver) = pw::channel::channel::<Command>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let linked = Arc::new(AtomicUsize::new(0));
        let thread = {
            let target = target.clone();
            let linked = linked.clone();
            std::thread::Builder::new()
                .name("undavi-app-capture".into())
                .spawn(move || {
                    pw::init();
                    if let Err(e) = run(target, push, dead, linked, receiver, &ready_tx) {
                        let _ = ready_tx.send(Err(e.to_string()));
                    }
                })
                .map_err(|e| e.to_string())?
        };
        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self {
                commands,
                linked,
                target,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(format!("PipeWire: {e}")),
            Err(_) => Err("PipeWire didn't answer".into()),
        }
    }

    /// Follow something else (for "Music player", when the player changes).
    pub fn set_target(&mut self, target: Target) {
        if target != self.target {
            self.target = target.clone();
            let _ = self.commands.send(Command::Target(target));
        }
    }

    /// How many of the target's streams are being captured.
    pub fn linked(&self) -> usize {
        self.linked.load(Ordering::Relaxed)
    }
}

impl Drop for AppCapture {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The capture thread: our stream, a registry watcher, and the links between.
fn run(
    target: Target,
    mut push: impl FnMut(&[f32]) + 'static,
    dead: Arc<AtomicBool>,
    linked: Arc<AtomicUsize>,
    commands: pw::channel::Receiver<Command>,
    ready: &mpsc::Sender<Result<(), String>>,
) -> Result<(), pw::Error> {
    let mainloop = MainLoopRc::new(None)?;
    let context = ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let graph = Rc::new(RefCell::new(Graph {
        target: Some(target),
        ..Graph::default()
    }));
    let relink = {
        let graph = graph.clone();
        let core = core.clone();
        let linked = linked.clone();
        Rc::new(move || {
            let n = graph.borrow_mut().relink(&core);
            linked.store(n, Ordering::Relaxed);
        })
    };

    // The server going away ends the capture; the app opens a new one.
    let _core_listener = core
        .add_listener_local()
        .error({
            let dead = dead.clone();
            move |id, _seq, _res, _message| {
                if id == PW_ID_CORE {
                    dead.store(true, Ordering::Relaxed);
                }
            }
        })
        .register();

    let _registry_listener = registry
        .add_listener_local()
        .global({
            let graph = graph.clone();
            let relink = relink.clone();
            move |global| {
                let Some(props) = global.props else { return };
                match global.type_ {
                    ObjectType::Node
                        if props.get(*pw::keys::MEDIA_CLASS) == Some(STREAM_OUTPUT) =>
                    {
                        let Some((key, _)) = app_key(props) else {
                            return;
                        };
                        let names = app_names(props);
                        graph
                            .borrow_mut()
                            .streams
                            .insert(global.id, StreamNode { key, names });
                    }
                    ObjectType::Port => {
                        let node = props.get(*pw::keys::NODE_ID).and_then(|n| n.parse().ok());
                        let Some(node) = node else { return };
                        let port = PortInfo {
                            node,
                            output: props.get(*pw::keys::PORT_DIRECTION) == Some("out"),
                            channel: props
                                .get(*pw::keys::AUDIO_CHANNEL)
                                .unwrap_or("")
                                .to_string(),
                        };
                        graph.borrow_mut().ports.insert(global.id, port);
                    }
                    _ => return,
                }
                relink();
            }
        })
        .global_remove({
            let graph = graph.clone();
            let relink = relink.clone();
            move |id| {
                {
                    let mut g = graph.borrow_mut();
                    g.streams.remove(&id);
                    g.ports.remove(&id);
                    g.ports.retain(|_, p| p.node != id);
                    g.links.retain(|(out, inp), _| *out != id && *inp != id);
                }
                relink();
            }
        })
        .register();

    let stream = StreamRc::new(
        core.clone(),
        "Undavi",
        props(&[
            (*pw::keys::MEDIA_TYPE, "Audio"),
            (*pw::keys::MEDIA_CATEGORY, "Capture"),
            (*pw::keys::MEDIA_ROLE, "Music"),
            (*pw::keys::APP_NAME, "Undavi"),
            (*pw::keys::NODE_NAME, "undavi-app-capture"),
            // We make our own links; the session manager shouldn't add any.
            (*pw::keys::NODE_AUTOCONNECT, "false"),
        ]),
    )?;
    let _stream_listener = stream
        .add_local_listener_with_user_data(())
        .state_changed({
            let graph = graph.clone();
            let relink = relink.clone();
            let dead = dead.clone();
            move |stream, _, _old, new| match new {
                StreamState::Paused | StreamState::Streaming => {
                    let id = stream.node_id();
                    let changed = graph.borrow().ours != Some(id);
                    if changed {
                        graph.borrow_mut().ours = Some(id);
                        relink();
                    }
                }
                StreamState::Error(_) => dead.store(true, Ordering::Relaxed),
                _ => {}
            }
        })
        .process(move |stream, _| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let size = data.chunk().size() as usize;
            let Some(bytes) = data.data() else { return };
            let bytes = &bytes[..size.min(bytes.len())];
            // Interleaved f32 pairs, as asked for in the format below.
            let samples: Vec<f32> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&b| f32::from_ne_bytes(b))
                .collect();
            push(&samples[..samples.len() & !1]);
        })
        .register()?;

    // Stereo f32 at a fixed rate; PipeWire converts whatever the apps play.
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_rate(RATE);
    info.set_channels(2);
    let mut position = [0; pw::spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
    position[0] = pw::spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = pw::spa::sys::SPA_AUDIO_CHANNEL_FR;
    info.set_position(position);
    let format = Value::Object(Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    });
    let bytes = PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &format)
        .map_err(|_| pw::Error::CreationFailed)?
        .0
        .into_inner();
    let pod = Pod::from_bytes(&bytes).ok_or(pw::Error::CreationFailed)?;
    stream.connect(
        Direction::Input,
        None,
        StreamFlags::MAP_BUFFERS | StreamFlags::DONT_RECONNECT,
        &mut [pod],
    )?;

    let _commands = commands.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let stream = stream.clone();
        let graph = graph.clone();
        move |command| match command {
            Command::Target(target) => {
                graph.borrow_mut().target = Some(target);
                relink();
            }
            Command::Stop => {
                let _ = stream.disconnect();
                mainloop.quit();
            }
        }
    });

    let _ = ready.send(Ok(()));
    mainloop.run();
    graph.borrow_mut().links.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn players_match_their_apps() {
        let spotify = names(&["Spotify", "spotify"]);
        let firefox = names(&["Firefox", "firefox"]);
        let chrome = names(&["Google Chrome", "chrome"]);
        assert!(player_matches("spotify", "Spotify", &spotify));
        assert!(player_matches(
            "firefox.instance_1_23",
            "Mozilla Firefox",
            &firefox
        ));
        assert!(player_matches(
            "chromium.instance42",
            "Google Chrome",
            &chrome
        ));
        assert!(!player_matches("spotify", "Spotify", &firefox));
        assert!(!player_matches(
            "firefox.instance_1_23",
            "Mozilla Firefox",
            &spotify
        ));
        assert!(player_matches("org.kde.elisa", "Elisa", &names(&["elisa"])));
        assert!(player_matches(
            "io.github.celluloid_player.Celluloid",
            "Celluloid",
            &names(&["Celluloid"])
        ));
        // Domain parts and very short names don't match everything.
        assert!(!player_matches(
            "org.kde.elisa",
            "Elisa",
            &names(&["KDE Connect"])
        ));
        assert!(!player_matches("mp", "mp", &names(&["Spotify"])));
    }

    #[test]
    fn channels_go_to_their_side() {
        assert_eq!(feeds("FL"), &[0]);
        assert_eq!(feeds("RR"), &[1]);
        assert_eq!(feeds("MONO"), &[0, 1]);
        assert_eq!(feeds("FC"), &[0, 1]);
        assert_eq!(feeds(""), &[0, 1]);
    }
}
