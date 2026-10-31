//! Bridge to the browser extension (milestone M6).
//!
//! A named pipe server accepts memwarden-host connections, one per browser
//! profile. Each connection is mapped to its browser by walking up from the
//! pipe client's PID; nothing the client says about itself is trusted.
//! The extension reports tab counts only, never URLs or titles. The bridge
//! changes no process itself: on user request it asks the extension to
//! discard idle tabs, and the browser does the discarding.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use memwarden_host::browser::browser_ancestor;
use memwarden_host::frame::{read_frame, write_frame};
use memwarden_host::pipe::Pipe;
use memwarden_host::{PROTOCOL_VERSION, pipe_name};
use serde::Deserialize;

use crate::process::Instance;

/// A discard request still unanswered after this long is treated as finished,
/// so a stuck extension never blocks the button.
const DISCARD_TIMEOUT: Duration = Duration::from_secs(30);
const RETRY: Duration = Duration::from_secs(1);

/// Tab counts reported by one extension instance.
#[derive(Deserialize, Clone, Copy, Default, PartialEq, Debug)]
pub struct Tabs {
    pub total: u32,
    pub discarded: u32,
    /// Tabs the extension would discard now (idle past its threshold, not
    /// active, audible or pinned).
    pub candidates: u32,
}

/// Tabs of one browser, summed over its connected profiles.
pub struct BrowserTabs {
    pub browser: Instance,
    pub tabs: Tabs,
    pub discard: Option<Discard>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Discard {
    Running,
    Done { discarded: u32, at: Instant },
}

pub struct Bridge {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    state: State,
    pipes: HashMap<u64, Arc<Pipe>>,
    next_id: u64,
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(|e| e.into_inner())
}

impl Bridge {
    /// Starts the pipe server. Returns `None` when the pipe cannot be created,
    /// for example because another memwarden is already running.
    pub fn start(ctx: egui::Context) -> Option<Self> {
        let name = pipe_name().ok()?;
        let first = Pipe::create(&name, true).ok()?;
        let inner = Arc::new(Mutex::new(Inner::default()));
        let shared = inner.clone();
        thread::Builder::new()
            .name("bridge".into())
            .spawn(move || {
                let mut next = Some(first);
                loop {
                    let pipe = match next.take().map_or_else(|| Pipe::create(&name, false), Ok) {
                        Ok(pipe) => pipe,
                        Err(_) => {
                            thread::sleep(RETRY);
                            continue;
                        }
                    };
                    if pipe.accept().is_ok() {
                        let (shared, ctx) = (shared.clone(), ctx.clone());
                        let _ = thread::Builder::new()
                            .name("bridge-conn".into())
                            .spawn(move || serve(Arc::new(pipe), &shared, &ctx));
                    }
                }
            })
            .ok()?;
        Some(Self { inner })
    }

    /// Connected browsers that have reported their tabs.
    pub fn browsers(&self) -> Vec<BrowserTabs> {
        lock(&self.inner).state.browsers(Instant::now())
    }

    /// Whether an extension or host speaks another protocol version.
    pub fn incompatible(&self) -> bool {
        lock(&self.inner).state.incompatible
    }

    /// Asks every profile of `browser` to discard its idle tabs. Called only
    /// from a user action. Returns whether the request was sent.
    pub fn discard_idle(&self, browser: Instance) -> bool {
        let pipes: Vec<Arc<Pipe>> = {
            let mut inner = lock(&self.inner);
            let ids = inner.state.begin_discard(browser, Instant::now());
            ids.iter().filter_map(|id| inner.pipes.get(id).cloned()).collect()
        };
        let msg = format!(r#"{{"v":{PROTOCOL_VERSION},"type":"discard_idle"}}"#);
        // A failed write closes the connection, which also ends its wait.
        for pipe in &pipes {
            let _ = write_frame(&mut &**pipe, msg.as_bytes());
        }
        !pipes.is_empty()
    }
}

/// Handles one host connection until it closes or breaks the protocol.
fn serve(pipe: Arc<Pipe>, shared: &Mutex<Inner>, ctx: &egui::Context) {
    let browser = pipe.client_pid().ok().and_then(browser_ancestor);
    let mut reader = &*pipe;
    // The host greets first; anything else is not our host.
    match read_frame(&mut reader).map(|m| m.map(|m| parse(&m))) {
        Ok(Some(Ok(Message::Hello))) => {}
        Ok(Some(Err(Reject::Version))) => {
            lock(shared).state.incompatible = true;
            ctx.request_repaint();
            return;
        }
        _ => return,
    }
    let id = {
        let mut inner = lock(shared);
        let id = inner.next_id;
        inner.next_id += 1;
        inner.pipes.insert(id, pipe.clone());
        inner.state.connect(id, browser);
        id
    };
    while let Ok(Some(msg)) = read_frame(&mut reader) {
        match parse(&msg) {
            Ok(msg) => lock(shared).state.receive(id, msg, Instant::now()),
            Err(reject) => {
                if reject == Reject::Version {
                    lock(shared).state.incompatible = true;
                }
                break;
            }
        }
        ctx.request_repaint();
    }
    let mut inner = lock(shared);
    inner.pipes.remove(&id);
    inner.state.disconnect(id, Instant::now());
    ctx.request_repaint();
}

// ---------- protocol ----------

#[derive(PartialEq, Debug)]
enum Message {
    Hello,
    Tabs(Tabs),
    DiscardResult { discarded: u32 },
    /// A message type this version does not know; ignored.
    Other,
}

#[derive(PartialEq, Debug)]
enum Reject {
    Version,
    Invalid,
}

#[derive(Deserialize)]
struct Envelope {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct DiscardResult {
    discarded: u32,
}

fn parse(body: &[u8]) -> Result<Message, Reject> {
    let envelope: Envelope = serde_json::from_slice(body).map_err(|_| Reject::Invalid)?;
    if envelope.v != PROTOCOL_VERSION {
        return Err(Reject::Version);
    }
    Ok(match envelope.kind.as_str() {
        "hello" => Message::Hello,
        "tabs" => Message::Tabs(serde_json::from_slice(body).map_err(|_| Reject::Invalid)?),
        "discard_result" => {
            let r: DiscardResult = serde_json::from_slice(body).map_err(|_| Reject::Invalid)?;
            Message::DiscardResult { discarded: r.discarded }
        }
        _ => Message::Other,
    })
}

// ---------- connection state (no I/O, unit tested) ----------

#[derive(Default)]
struct State {
    conns: HashMap<u64, Conn>,
    runs: HashMap<Instance, Run>,
    incompatible: bool,
}

struct Conn {
    /// `None` when the client was not launched by a supported browser.
    browser: Option<Instance>,
    tabs: Option<Tabs>,
    /// A discard request was sent and not answered yet.
    awaiting: bool,
}

struct Run {
    started: Instant,
    waiting: usize,
    discarded: u32,
    finished: Option<Instant>,
}

impl Run {
    fn answer(&mut self, discarded: u32, now: Instant) {
        self.discarded += discarded;
        self.waiting = self.waiting.saturating_sub(1);
        if self.waiting == 0 {
            self.finished = Some(now);
        }
    }

    fn status(&self, now: Instant) -> Discard {
        let timed_out = now.duration_since(self.started) >= DISCARD_TIMEOUT;
        match self.finished {
            Some(at) => Discard::Done { discarded: self.discarded, at },
            None if timed_out => Discard::Done { discarded: self.discarded, at: self.started + DISCARD_TIMEOUT },
            None => Discard::Running,
        }
    }
}

impl State {
    fn connect(&mut self, id: u64, browser: Option<Instance>) {
        self.incompatible = false;
        self.conns.insert(id, Conn { browser, tabs: None, awaiting: false });
    }

    fn receive(&mut self, id: u64, msg: Message, now: Instant) {
        let Some(conn) = self.conns.get_mut(&id) else { return };
        match msg {
            Message::Tabs(tabs) => conn.tabs = Some(tabs),
            Message::DiscardResult { discarded } if conn.awaiting => {
                conn.awaiting = false;
                if let Some(run) = conn.browser.and_then(|b| self.runs.get_mut(&b)) {
                    run.answer(discarded, now);
                }
            }
            _ => {}
        }
    }

    fn disconnect(&mut self, id: u64, now: Instant) {
        let Some(conn) = self.conns.remove(&id) else { return };
        if conn.awaiting
            && let Some(run) = conn.browser.and_then(|b| self.runs.get_mut(&b))
        {
            run.answer(0, now);
        }
    }

    /// Marks the connections of `browser` as awaiting a result and returns
    /// their ids. Nothing is sent while a previous request is still running.
    fn begin_discard(&mut self, browser: Instance, now: Instant) -> Vec<u64> {
        if self.runs.get(&browser).is_some_and(|r| r.status(now) == Discard::Running) {
            return Vec::new();
        }
        let ids: Vec<u64> = self
            .conns
            .iter_mut()
            .filter(|(_, c)| c.browser == Some(browser) && c.tabs.is_some())
            .map(|(&id, c)| {
                c.awaiting = true;
                id
            })
            .collect();
        if !ids.is_empty() {
            self.runs.insert(browser, Run { started: now, waiting: ids.len(), discarded: 0, finished: None });
        }
        ids
    }

    fn browsers(&self, now: Instant) -> Vec<BrowserTabs> {
        let mut by_browser: HashMap<Instance, Tabs> = HashMap::new();
        for conn in self.conns.values() {
            if let (Some(browser), Some(tabs)) = (conn.browser, conn.tabs) {
                let sum = by_browser.entry(browser).or_default();
                sum.total += tabs.total;
                sum.discarded += tabs.discarded;
                sum.candidates += tabs.candidates;
            }
        }
        let mut out: Vec<BrowserTabs> = by_browser
            .into_iter()
            .map(|(browser, tabs)| BrowserTabs {
                browser,
                tabs,
                discard: self.runs.get(&browser).map(|r| r.status(now)),
            })
            .collect();
        out.sort_by_key(|b| b.browser);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME: Instance = (100, 1);
    const EDGE: Instance = (200, 2);

    fn tabs(total: u32, discarded: u32, candidates: u32) -> Message {
        Message::Tabs(Tabs { total, discarded, candidates })
    }

    #[test]
    fn parses_known_messages() {
        assert_eq!(parse(br#"{"v":1,"type":"hello"}"#), Ok(Message::Hello));
        assert_eq!(
            parse(br#"{"v":1,"type":"tabs","total":5,"discarded":1,"candidates":2,"threshold_min":15}"#),
            Ok(tabs(5, 1, 2))
        );
        assert_eq!(
            parse(br#"{"v":1,"type":"discard_result","requested":3,"discarded":2,"skipped":1}"#),
            Ok(Message::DiscardResult { discarded: 2 })
        );
        assert_eq!(parse(br#"{"v":1,"type":"something_new"}"#), Ok(Message::Other));
    }

    #[test]
    fn rejects_bad_messages() {
        assert_eq!(parse(br#"{"v":2,"type":"hello"}"#), Err(Reject::Version));
        assert_eq!(parse(b"not json"), Err(Reject::Invalid));
        assert_eq!(parse(br#"{"type":"hello"}"#), Err(Reject::Invalid));
        assert_eq!(parse(br#"{"v":1,"type":"tabs","total":-1}"#), Err(Reject::Invalid));
    }

    #[test]
    fn sums_profiles_per_browser() {
        let now = Instant::now();
        let mut s = State::default();
        s.connect(1, Some(CHROME));
        s.connect(2, Some(CHROME));
        s.connect(3, Some(EDGE));
        s.connect(4, None);
        s.receive(1, tabs(10, 2, 3), now);
        s.receive(2, tabs(5, 0, 1), now);
        s.receive(4, tabs(99, 0, 99), now);
        // Edge has not reported yet, the unknown client is never shown.
        let b = s.browsers(now);
        assert_eq!(b.len(), 1);
        assert_eq!((b[0].browser, b[0].tabs), (CHROME, Tabs { total: 15, discarded: 2, candidates: 4 }));

        s.disconnect(2, now);
        assert_eq!(s.browsers(now)[0].tabs.total, 10);
    }

    #[test]
    fn discard_waits_for_every_profile() {
        let now = Instant::now();
        let mut s = State::default();
        s.connect(1, Some(CHROME));
        s.connect(2, Some(CHROME));
        s.receive(1, tabs(10, 0, 3), now);
        s.receive(2, tabs(5, 0, 1), now);

        let mut ids = s.begin_discard(CHROME, now);
        ids.sort();
        assert_eq!(ids, [1, 2]);
        // A second click while running sends nothing.
        assert!(s.begin_discard(CHROME, now).is_empty());

        s.receive(1, Message::DiscardResult { discarded: 3 }, now);
        assert_eq!(s.browsers(now)[0].discard, Some(Discard::Running));
        // An unrequested result is ignored.
        s.receive(1, Message::DiscardResult { discarded: 50 }, now);
        let later = now + Duration::from_secs(1);
        s.receive(2, Message::DiscardResult { discarded: 1 }, later);
        assert_eq!(s.browsers(later)[0].discard, Some(Discard::Done { discarded: 4, at: later }));
    }

    #[test]
    fn disconnect_or_timeout_ends_a_discard() {
        let now = Instant::now();
        let mut s = State::default();
        s.connect(1, Some(CHROME));
        s.connect(2, Some(CHROME));
        s.receive(1, tabs(10, 0, 3), now);
        s.receive(2, tabs(5, 0, 1), now);
        s.begin_discard(CHROME, now);
        s.receive(1, Message::DiscardResult { discarded: 3 }, now);
        s.disconnect(2, now);
        assert_eq!(s.browsers(now)[0].discard, Some(Discard::Done { discarded: 3, at: now }));

        s.receive(1, tabs(10, 3, 0), now);
        s.begin_discard(CHROME, now);
        let late = now + DISCARD_TIMEOUT;
        assert_eq!(s.browsers(late)[0].discard, Some(Discard::Done { discarded: 0, at: late }));
    }
}
