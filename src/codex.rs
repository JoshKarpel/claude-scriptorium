//! The codex: one server for every folio the scriptorium holds, each following
//! its session as it is written.
//!
//! Two scopes, one server. Pointed at a projects root it lists every quire and
//! serves any session in it; pointed at a single session it serves that folio and
//! nothing else, which is the render loop. What differs between them is what `/`
//! answers with and which sessions can be named; everything below that (the
//! assets, the folio page, the stream) is the same code, so the two cannot drift
//! apart.
//!
//! **A page is patched, not reloaded.** A panel's id is its turn number, which
//! counts the raw records of a session file that is only ever appended to, so a
//! panel keeps its id however much the session grows (see [`Scribe::panels`]).
//! The server therefore sets the session again, compares the panels it got with
//! the ones it last sent, and pushes only those that differ. A reader keeps their
//! scroll position, their open folds, their search, and their place in the
//! conversation, because nothing about the page is thrown away to add to it.
//!
//! **Nothing about a session is read on the request path.** One watcher thread
//! holds every mutable thing here: the listing, the titles it has read, and the
//! last setting of each folio being read. It looks for changes on a timer, and
//! only for folios someone actually has open, so an idle codex does no work.

use std::{
    collections::HashMap,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use tiny_http::{Header, Request, Response, Server};

use crate::{
    catalogue::{Catalogue, Peeks, Shelf, Shelved},
    render::{self, Asset, Colophon, Scribe},
    transcript::Folio,
};

/// How often the watcher looks for a change. Short enough that a session reads
/// as live, long enough that a metadata check per open folio is nothing.
const TICK: Duration = Duration::from_millis(400);

/// How long a stream may say nothing before it says nothing out loud. A silent
/// connection is one an intermediary may decide is dead, so an idle codex sends
/// a comment rather than trusting the path to hold.
const HEARTBEAT: Duration = Duration::from_secs(15);

/// How long to keep trying a session that will not parse. A live session is
/// caught mid-line often enough to matter, and a half-written line is finished
/// microseconds later, so the answer is to ask again rather than to hand the
/// reader a failure. A session that is genuinely malformed still fails, having
/// cost half a second.
const PATIENCE: usize = 12;
const BREATH: Duration = Duration::from_millis(40);

/// How many frames may be waiting for one page before that page is let go.
///
/// A frame is written to a socket with no deadline, so a reader whose machine
/// has suspended (or whose receive window has simply stopped moving) leaves its
/// own thread blocked in a write while the watcher goes on producing patches for
/// it. Queueing those without limit is a folio's worth of markup accumulating per
/// tick for a page that will never read any of it.
///
/// Letting the page go is the honest answer rather than a lossy one: the stream
/// ends, the browser reconnects on its own, and it reports what it holds in
/// `Last-Event-ID`, which is exactly the catch-up a dropped connection already
/// takes. Small, because a page that is this far behind is not going to catch up
/// by being queued for.
const BACKLOG: usize = 8;

/// What a codex is serving.
pub enum Scope {
    /// Every session under a projects root, listed and browsable.
    Codex { root: PathBuf },
    /// One session, named on the command line. There is no listing above it, so
    /// nothing links to one.
    Folio { session: PathBuf },
}

/// Serves until interrupted. `address` is what the server binds, `open` asks for
/// the reader's browser to be pointed at it once it is up.
pub fn run(address: &str, scope: Scope, open: bool, scribe: &Scribe<'_>) -> Result<()> {
    let server = bind(address)?;
    let codex = Codex::new(scope, scribe);
    println!("serving http://{address}  (Ctrl-C to stop)");
    if open {
        let _ = open::that(format!("http://{address}"));
    }

    // Scoped, so the watcher and every request can borrow the scribe rather than
    // the server having to own a `'static` copy of everything a render needs.
    thread::scope(|threads| {
        threads.spawn(|| codex.watch());
        for request in server.incoming_requests() {
            // A thread per request rather than a pool: a stream holds its
            // connection for as long as its reader has the page open, and a pool
            // would spend its workers on waiting.
            threads.spawn(|| codex.answer(request));
        }
    });
    Ok(())
}

/// Binds the port, retrying briefly so a restart can reclaim it from the
/// previous run's lingering connections before giving up.
fn bind(address: &str) -> Result<Server> {
    let mut last = None;
    for _ in 0..40 {
        match Server::http(address) {
            Ok(server) => return Ok(server),
            Err(error) => {
                last = Some(error.to_string());
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Err(anyhow!(
        "could not bind {address}: {}",
        last.unwrap_or_default()
    ))
}

struct Codex<'a> {
    scope: Scope,
    scribe: &'a Scribe<'a>,
    /// This run of the server. A page told a different one is a page set by a
    /// binary that is no longer running, which is the render loop's cue to
    /// reload: a change to the stylesheet or to the renderer reaches a reader no
    /// other way.
    boot: String,
    state: Mutex<State>,
    listeners: AtomicU64,
}

/// Everything mutable, in one place, behind one lock. The contention is a listing
/// against a tick every few hundred milliseconds, so nothing here is worth
/// splitting up.
#[derive(Default)]
struct State {
    catalogue: Catalogue,
    peeks: Peeks,
    /// The last setting of each folio someone has open, by session id.
    folios: HashMap<String, Set>,
    /// The last listing *sent* for each kind of listing page open, which is what
    /// every page watching that listing is holding.
    ///
    /// Only [`Codex::relist`] writes here, and only along with telling everyone
    /// watching. A page load gathers a listing of its own and stores none of it:
    /// storing markup only the one page ever saw would leave every other page
    /// holding an earlier listing that the next tick then measures as unchanged
    /// and says nothing about, which for a quiet codex is forever.
    listings: HashMap<Watching, Listing>,
    listeners: Vec<Listener>,
}

/// One setting of a folio: what the reader following it is holding.
struct Set {
    /// The state of the session file this was set from, which is what a
    /// reconnecting page reports back so the catch-up can be measured.
    stamp: Stamp,
    panels: Vec<(usize, String)>,
    facts: String,
    /// Which cut of the faces the page is dressed in. A panel that arrives
    /// setting a character the cut faces dropped has to bring the whole ones
    /// with it, or a followed folio would render that character worse than a
    /// written one.
    faces: Asset,
}

/// One gathering of a listing: the markup to seat, and the faces to be dressed
/// in while it is seated.
///
/// The faces travel with it for the same reason a folio's patch carries them: a
/// session title or a project path that reaches a character the cut faces
/// dropped arrives after the page was dressed, and a listing replaced without
/// them would render that title in the reader's own fallback font.
struct Listing {
    markup: String,
    faces: Asset,
}

impl Listing {
    fn told(&self) -> String {
        json!({ "html": self.markup, "faces": self.faces.url() }).to_string()
    }
}

/// One open page, and the frames it is waiting for.
struct Listener {
    id: u64,
    watching: Watching,
    frames: SyncSender<String>,
}

/// What an open page is being told about.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum Watching {
    Shelf,
    Quire(String),
    Folio(String),
}

/// What a URL names. Parsing it is a pure function of the path, so every shape a
/// request can take is a test rather than a socket.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Resource {
    /// The codex's own listing, which under a single-session scope is the folio.
    Shelf,
    Quire(String),
    Folio(String),
    Asset(Asset),
    /// A stream to follow, and the state the page asking already holds.
    Live(Watching, Option<String>),
    Missing,
}

fn route(url: &str) -> Resource {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let from = || {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix("from="))
            .map(str::to_owned)
    };
    // An id is a file stem or Claude Code's own encoded directory name, neither
    // of which holds a separator, and nothing here joins a path out of one
    // regardless: an id is looked up in the listing (see `Catalogue::session`).
    //
    // The id is percent-decoded, undoing what `render::encoded` spelled: `serve`
    // takes an arbitrary path, so a session named with a space reaches the
    // listing as itself rather than as `%20`. A segment that is not valid
    // encoding names nothing this codex holds.
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let named = |id: &str, resource: fn(String) -> Resource| {
        render::decoded(id).map_or(Resource::Missing, resource)
    };
    match parts.as_slice() {
        [] => Resource::Shelf,
        ["quire", id] => named(id, Resource::Quire),
        ["folio", id] => named(id, Resource::Folio),
        // The token names the assets' contents, so a request carrying a stale one
        // is asking for something this binary does not have. Answering 404 rather
        // than serving today's bytes under yesterday's name is what keeps the
        // immutable caching honest.
        ["asset", token, name] if *token == render::ASSET_TOKEN => {
            Asset::named(name).map_or(Resource::Missing, Resource::Asset)
        }
        ["live"] => Resource::Live(Watching::Shelf, from()),
        ["live", "quire", id] => match render::decoded(id) {
            Some(id) => Resource::Live(Watching::Quire(id), from()),
            None => Resource::Missing,
        },
        ["live", "folio", id] => match render::decoded(id) {
            Some(id) => Resource::Live(Watching::Folio(id), from()),
            None => Resource::Missing,
        },
        _ => Resource::Missing,
    }
}

/// What changed between two settings of one folio, as turn numbers: which panels
/// to set again, and which the folio no longer has.
///
/// Keyed by turn number rather than by position, which is what makes a patch
/// small: a tool result joins the panel holding its call, so a session usually
/// changes its last panel or two and adds one, however long it is.
///
/// Answering with numbers rather than markup keeps this a function of its
/// arguments with nothing to copy; the caller looks up what it sends.
#[derive(Debug, Default, Eq, PartialEq)]
struct Patch {
    changed: Vec<usize>,
    gone: Vec<usize>,
}

impl Patch {
    fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.gone.is_empty()
    }
}

fn patch(before: &[(usize, String)], after: &[(usize, String)]) -> Patch {
    let held: HashMap<usize, &str> = before
        .iter()
        .map(|(turn, markup)| (*turn, markup.as_str()))
        .collect();
    let changed = after
        .iter()
        .filter(|(turn, markup)| held.get(turn) != Some(&markup.as_str()))
        .map(|(turn, _)| *turn)
        .collect();
    let standing: HashMap<usize, ()> = after.iter().map(|(turn, _)| (*turn, ())).collect();
    let gone = before
        .iter()
        .map(|(turn, _)| *turn)
        .filter(|turn| !standing.contains_key(turn))
        .collect();
    Patch { changed, gone }
}

impl<'a> Codex<'a> {
    fn new(scope: Scope, scribe: &'a Scribe<'a>) -> Self {
        Self {
            scope,
            scribe,
            boot: format!("{}", nanos(SystemTime::now())),
            state: Mutex::new(State::default()),
            listeners: AtomicU64::new(0),
        }
    }

    /// Everything mutable, locked.
    ///
    /// A poisoned lock is recovered rather than propagated. The lock is held
    /// across real work (a listing reads and parses every previewed session), so
    /// a panic under it is possible; and [`run`] never returns, so propagating
    /// the poison would leave every later request and every tick panicking on
    /// the lock while the process stayed up serving nothing, which is the one
    /// failure a `Restart=always` unit cannot see. One bad answer is smaller
    /// than a server that is up and dead.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // --- The control plane -------------------------------------------------

    /// Looks for changes on a timer, for the pages someone actually has open.
    ///
    /// Everything that reads a session happens here rather than while a request
    /// waits: a folio nobody is reading is never set, and a listing nobody is
    /// looking at is never gathered.
    fn watch(&self) {
        loop {
            thread::sleep(TICK);
            for watching in self.watched() {
                match &watching {
                    Watching::Folio(id) => {
                        // Out of the listing alone: a tick must never scan the
                        // root (see [`Codex::locate`]), and a folio someone has
                        // open was found by the request that served it.
                        if let Some(path) = self.filed(id) {
                            // A session caught mid-write simply has not changed
                            // yet: the last good setting stands, and the next
                            // tick asks again. Nothing reaches the reader until a
                            // whole setting succeeds.
                            let _ = self.reset(id, &path);
                        }
                    }
                    _ => {
                        self.relist(&watching);
                    }
                }
            }
        }
    }

    /// What at least one open page is being told about.
    fn watched(&self) -> Vec<Watching> {
        let state = self.state();
        let mut watched: Vec<Watching> = Vec::new();
        for listener in &state.listeners {
            if !watched.contains(&listener.watching) {
                watched.push(listener.watching.clone());
            }
        }
        watched
    }

    /// Sets a folio again, tells everyone following it what changed, and holds
    /// the result as what those readers are now holding.
    ///
    /// A subscription and a tick both come through here, so a reader can never be
    /// left holding a state no patch was ever measured against.
    ///
    /// A session that has not been written since the last setting is not set
    /// again: the stamp is the file's own, and a setting is a pure function of the
    /// file, so an unchanged stamp means the answer would be the same markup. Only
    /// the stamp is looked at on a tick, which is one `metadata` call per open
    /// folio, where setting one again is most of a render.
    ///
    /// A session that is no longer there is not read at all. [`read`] spends half
    /// a second retrying, which is the right answer for a line caught half-written
    /// and the wrong one for a file that has been deleted: nothing will finish it,
    /// so a page left open on a deleted session would put this thread into a
    /// permanent retry, ahead of every other folio and listing being watched. The
    /// last good setting stands instead, which is what a reader is holding anyway.
    fn reset(&self, id: &str, path: &Path) -> Result<()> {
        let Some(stamp) = Stamp::of(path) else {
            return Ok(());
        };
        if self.holds(id, stamp) {
            return Ok(());
        }
        let folio = read(path)?;
        let panels = folio.panels();
        let (set, panel_reach) = self.scribe.panels(&panels);
        let facts = self.scribe.facts(&folio, &panels).into_string();
        // Weighed exactly as a whole folio is, source path included: a page
        // already dressed in the whole faces because of its own plaque must not
        // be re-dressed down into the cut ones by a patch that only looked at
        // the panels.
        let faces = self
            .scribe
            .faces(!render::reached(&folio, panel_reach).is_empty());

        let mut state = self.state();
        // A tick and a subscription both come through here, and each reads and
        // renders outside the lock so a render is never held under it, so two
        // settings of one folio can be in flight at once. Whichever commits
        // second may be the older read, and committing it would measure the
        // newer panels as *gone* and take them back a tick later, under the
        // reader. The stamp is ordered exactly so that can be recognised.
        if state.folios.get(id).is_some_and(|held| held.stamp >= stamp) {
            return Ok(());
        }
        if let Some(held) = state.folios.get(id) {
            let patch = patch(&held.panels, &set);
            // The faces are named only when they change, so a folio that was
            // already dressed in the whole ones says nothing about them.
            let dress = (faces != held.faces).then(|| faces.url());
            if !patch.is_empty() || facts != held.facts || dress.is_some() {
                let word = stamp.word();
                let data = told(&word, &patch, &set, &facts, dress.as_deref());
                state.tell(
                    &Watching::Folio(id.to_owned()),
                    "panels",
                    &data,
                    Some(&word),
                );
            }
        }
        state.folios.insert(
            id.to_owned(),
            Set {
                stamp,
                panels: set,
                facts,
                faces,
            },
        );
        Ok(())
    }

    /// Whether the setting already held for this folio was made from the session
    /// as it now stands.
    fn holds(&self, id: &str, stamp: Stamp) -> bool {
        let state = self.state();
        state.folios.get(id).is_some_and(|held| held.stamp == stamp)
    }

    /// Gathers a listing again and sends it to everyone watching if it says
    /// anything new, answering whether it did. A listing is a few kilobytes, so
    /// it is replaced whole rather than picked apart: what changes as a session
    /// is written is every row's "how long ago" as much as the rows themselves.
    ///
    /// Telling and storing are one step, which is what keeps the store meaning
    /// what [`State::listings`] says it means.
    fn relist(&self, watching: &Watching) -> bool {
        let Some(listing) = self.listing(watching) else {
            return false;
        };
        let mut state = self.state();
        if state
            .listings
            .get(watching)
            .is_some_and(|held| held.markup == listing.markup)
        {
            return false;
        }
        let told = listing.told();
        state.listings.insert(watching.clone(), listing);
        state.tell(watching, "listing", &told, None);
        true
    }

    /// One listing's markup, gathered fresh. `None` when the quire a page names
    /// is no longer there, in which case the page keeps what it has until its
    /// reader goes back up.
    fn listing(&self, watching: &Watching) -> Option<Listing> {
        let root = self.root()?;
        let catalogue = Catalogue::scan(root).ok()?;
        let now = SystemTime::now();
        let mut state = self.state();
        state.peeks.keep_to(&catalogue);
        let listing = match watching {
            Watching::Shelf => {
                let shelf = Shelf::of(&catalogue, &mut state.peeks, now);
                Listing {
                    markup: self.scribe.shelf(&shelf).into_string(),
                    faces: self.scribe.listing_faces(&shelf.labels()),
                }
            }
            Watching::Quire(id) => {
                let quire = catalogue.quire(id)?;
                let shelved = Shelved::whole(quire, &mut state.peeks, now);
                Listing {
                    markup: self.scribe.leaves(&shelved).into_string(),
                    faces: self.scribe.listing_faces(&shelved.labels()),
                }
            }
            Watching::Folio(_) => return None,
        };
        state.catalogue = catalogue;
        Some(listing)
    }

    // --- Where a session is ------------------------------------------------

    fn root(&self) -> Option<&Path> {
        match &self.scope {
            Scope::Codex { root } => Some(root),
            Scope::Folio { .. } => None,
        }
    }

    /// The session a URL's id names, out of what is already known.
    ///
    /// A single-session scope answers for its own session and nothing else; a
    /// codex answers out of the listing it last scanned. Either way no path is
    /// composed from what a request said, and nothing here touches the disk.
    fn filed(&self, id: &str) -> Option<PathBuf> {
        match &self.scope {
            Scope::Folio { session } => (stem(session) == id).then(|| session.clone()),
            Scope::Codex { .. } => self
                .state()
                .catalogue
                .session(id)
                .map(|found| found.path.clone()),
        }
    }

    /// The session a URL's id names, rescanning once if the listing does not
    /// hold it: a session recorded since the last scan is a miss to fill, not a
    /// stranger.
    ///
    /// This is the *request* path's lookup. A scan reads every project directory
    /// and stats every session in it, which is far too much to do on a timer, so
    /// the watcher asks [`Codex::filed`] instead: an id that is genuinely absent
    /// misses forever, and a page left open on a session that has been deleted
    /// would otherwise rescan the whole root several times a second for as long
    /// as it stayed open.
    fn locate(&self, id: &str) -> Option<PathBuf> {
        if let Some(found) = self.filed(id) {
            return Some(found);
        }
        let Scope::Codex { root } = &self.scope else {
            return None;
        };
        let catalogue = Catalogue::scan(root).ok()?;
        let mut state = self.state();
        state.catalogue = catalogue;
        state.catalogue.session(id).map(|found| found.path.clone())
    }

    // --- The request path --------------------------------------------------

    fn answer(&self, request: Request) {
        let resource = route(request.url());
        match resource {
            Resource::Live(watching, from) => self.stream(request, watching, from),
            Resource::Asset(asset) => respond(request, asset_response(asset)),
            other => match self.page(&other) {
                Ok(Some(page)) => respond(request, page_response(page)),
                Ok(None) => respond(request, missing()),
                Err(error) => respond(request, failed(&error)),
            },
        }
    }

    /// One page's markup, or `None` for a resource this codex does not hold.
    fn page(&self, resource: &Resource) -> Result<Option<String>> {
        match resource {
            // Under a single-session scope the root *is* the folio: there is no
            // listing to stand in front of one session.
            Resource::Shelf => match &self.scope {
                Scope::Folio { session } => self.folio(&stem(session)).map(Some),
                Scope::Codex { .. } => Ok(Some(self.shelf_page())),
            },
            Resource::Quire(id) => Ok(self.quire_page(id)),
            Resource::Folio(id) => match self.locate(id) {
                Some(_) => self.folio(id).map(Some),
                None => Ok(None),
            },
            Resource::Asset(_) | Resource::Live(..) | Resource::Missing => Ok(None),
        }
    }

    /// The codex's own listing page. What is stored as sent is left to the
    /// subscription this page is about to open (see [`State::listings`]).
    fn shelf_page(&self) -> String {
        let now = SystemTime::now();
        let catalogue = self
            .root()
            .and_then(|root| Catalogue::scan(root).ok())
            .unwrap_or_default();
        let mut state = self.state();
        state.peeks.keep_to(&catalogue);
        let shelf = Shelf::of(&catalogue, &mut state.peeks, now);
        state.catalogue = catalogue;
        self.scribe.codex(&shelf).into_string()
    }

    fn quire_page(&self, id: &str) -> Option<String> {
        let now = SystemTime::now();
        let catalogue = Catalogue::scan(self.root()?).ok()?;
        let mut state = self.state();
        let quire = catalogue.quire(id)?;
        let shelved = Shelved::whole(quire, &mut state.peeks, now);
        state.catalogue = catalogue;
        Some(self.scribe.quire(&shelved).into_string())
    }

    /// A folio, set from the session as it stands, and told which state it was
    /// set from so it can be followed from there.
    fn folio(&self, id: &str) -> Result<String> {
        let path = self
            .locate(id)
            .ok_or_else(|| anyhow!("no session named {id}"))?;
        // Asked before the read rather than after it, so a session that has been
        // deleted since the listing was scanned says so at once instead of
        // spending [`read`]'s whole patience on a file nothing will finish.
        let stamp =
            Stamp::of(&path).ok_or_else(|| anyhow!("{} is no longer there", path.display()))?;
        let folio = read(&path)?;
        let set = render::set(self.scribe, &folio, &Colophon::now(), Some(&stamp.word()));
        eprintln!(
            "{} {} in {}",
            id,
            render::size(set.document.len()),
            render::elapsed(set.labour.took)
        );
        Ok(set.document)
    }

    // --- The stream --------------------------------------------------------

    /// Follows a page for as long as its reader has it open.
    ///
    /// The reader says what they already hold, either in `Last-Event-ID` (which
    /// the browser resends on its own after a dropped connection) or in the `from`
    /// the page was set with. A page that is already current is told nothing; one
    /// that fell behind is sent the folio as it stands, which is the honest answer
    /// when the server no longer holds the state that page was set from.
    fn stream(&self, request: Request, watching: Watching, from: Option<String>) {
        let held = resumed(&request).or(from);
        let (frames, waiting) = mpsc::sync_channel(BACKLOG);
        let id = self.listeners.fetch_add(1, Ordering::Relaxed);
        // Registered before it is greeted, so nothing about what this page is
        // being caught up to can change between the two, and let go by the guard
        // however this thread ends.
        let _attending = Attending::new(self, id, watching.clone(), frames);

        // Bring this page up to date before anything else can change under it.
        self.greet(&watching, held.as_deref(), id);

        let mut writer = request.into_writer();
        let outcome = self.pour(&mut writer, &waiting);
        drop(writer);

        if let Err(error) = outcome {
            // A reader closing a tab is an ordinary end to a stream, not a fault.
            if error.kind() != io::ErrorKind::BrokenPipe {
                eprintln!("stream ended: {error}");
            }
        }
    }

    /// What a page is told the moment it starts listening: which run of the
    /// server it is talking to, and whatever it is missing.
    fn greet(&self, watching: &Watching, held: Option<&str>, id: u64) {
        let hello = json!({ "boot": self.boot }).to_string();
        {
            let mut state = self.state();
            state.only(id, "hello", &hello, None);
        }

        match watching {
            Watching::Folio(session) => {
                if let Some(path) = self.locate(session) {
                    // Set it again first, so what this page is measured against
                    // is the session as it stands rather than as it was when
                    // somebody else's page was served.
                    let _ = self.reset(session, &path);
                }
                let mut state = self.state();
                let Some(set) = state.folios.get(session) else {
                    return;
                };
                let word = set.stamp.word();
                if held == Some(word.as_str()) {
                    return;
                }
                // The page holds a state this server does not, so the only honest
                // answer is the folio as it stands. Every panel is keyed by its
                // turn number, so setting them all again lands exactly where the
                // page already agrees and replaces what it does not.
                let whole = Patch {
                    changed: set.panels.iter().map(|(turn, _)| *turn).collect(),
                    gone: Vec::new(),
                };
                let data = told(
                    &word,
                    &whole,
                    &set.panels,
                    &set.facts,
                    Some(&set.faces.url()),
                );
                state.only(id, "panels", &data, Some(&word));
            }
            listing => {
                // Bring every page already watching this listing up to date
                // first, this one among them. What that leaves stored is what
                // all of them hold, so a page arriving is never a page whose
                // markup the others are then measured against without ever
                // having been sent it.
                if self.relist(listing) {
                    return;
                }
                let mut state = self.state();
                let Some(held) = state.listings.get(listing) else {
                    return;
                };
                let told = held.told();
                state.only(id, "listing", &told, None);
            }
        }
    }

    /// Writes frames as they arrive, and a comment when none does, until the
    /// reader goes away.
    fn pour(&self, writer: &mut dyn Write, waiting: &Receiver<String>) -> io::Result<()> {
        write!(writer, "{}", head())?;
        // A hint at how long to wait before reconnecting, so a restart is picked
        // up in about a second rather than in whatever the browser defaults to.
        chunk(writer, "retry: 1000\n\n")?;
        loop {
            match waiting.recv_timeout(HEARTBEAT) {
                Ok(frame) => chunk(writer, &frame)?,
                Err(RecvTimeoutError::Timeout) => chunk(writer, ": still here\n\n")?,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    }
}

/// One page's place among the listeners, held for as long as its thread is
/// attending to it.
///
/// A guard rather than a pair of statements, because a panic has to clear it too.
/// [`Codex::greet`] sets a whole folio (`Scribe::panels` runs under rayon, and
/// weighing a panel against the cut faces carries an `expect`), and a listener
/// left behind is one the watcher goes on setting a folio for while nothing ever
/// drains what it is sent. [`run`] never returns, so nothing else would clear it.
struct Attending<'a, 'scribe> {
    codex: &'a Codex<'scribe>,
    id: u64,
    watching: Watching,
}

impl<'a, 'scribe> Attending<'a, 'scribe> {
    fn new(
        codex: &'a Codex<'scribe>,
        id: u64,
        watching: Watching,
        frames: SyncSender<String>,
    ) -> Self {
        codex.state().listeners.push(Listener {
            id,
            watching: watching.clone(),
            frames,
        });
        Self {
            codex,
            id,
            watching,
        }
    }
}

impl Drop for Attending<'_, '_> {
    fn drop(&mut self) {
        let mut state = self.codex.state();
        state.listeners.retain(|listener| listener.id != self.id);
        // A folio nobody is reading is not worth holding a setting of, and the
        // next reader's page will be measured against their own load.
        if state
            .listeners
            .iter()
            .any(|listener| listener.watching == self.watching)
        {
            return;
        }
        match &self.watching {
            Watching::Folio(session) => {
                state.folios.remove(session);
            }
            other => {
                state.listings.remove(other);
            }
        }
    }
}

impl State {
    /// Tells every page watching the same thing, letting go of any that has
    /// fallen a whole [`BACKLOG`] behind (see there for why that is the answer).
    /// A listener whose reader has gone is dropped by its own thread, so a
    /// disconnected channel here is simply a race with that and is nothing to
    /// answer for.
    fn tell(&mut self, watching: &Watching, event: &str, data: &str, id: Option<&str>) {
        let frame = frame(event, data, id);
        let mut sunk = Vec::new();
        for listener in &self.listeners {
            if &listener.watching != watching {
                continue;
            }
            if let Err(TrySendError::Full(_)) = listener.frames.try_send(frame.clone()) {
                sunk.push(listener.id);
            }
        }
        self.listeners
            .retain(|listener| !sunk.contains(&listener.id));
    }

    /// Tells one page, which is how a page that has just started listening is
    /// caught up without telling every other page what it already knows.
    fn only(&mut self, id: u64, event: &str, data: &str, stamp: Option<&str>) {
        let frame = frame(event, data, stamp);
        let sunk = self
            .listeners
            .iter()
            .find(|listener| listener.id == id)
            .is_some_and(|listener| {
                matches!(listener.frames.try_send(frame), Err(TrySendError::Full(_)))
            });
        if sunk {
            self.listeners.retain(|listener| listener.id != id);
        }
    }
}

/// What a folio's reader is told: the panels to set, the ones to drop, the plaque
/// facts as they now stand, and, when it has changed, the faces to be dressed in.
fn told(
    stamp: &str,
    patch: &Patch,
    panels: &[(usize, String)],
    facts: &str,
    faces: Option<&str>,
) -> String {
    let held: HashMap<usize, &str> = panels
        .iter()
        .map(|(turn, markup)| (*turn, markup.as_str()))
        .collect();
    let set: Vec<Value> = patch
        .changed
        .iter()
        .filter_map(|turn| {
            held.get(turn)
                .map(|markup| json!({ "turn": turn, "html": markup }))
        })
        .collect();
    json!({
        "stamp": stamp,
        "panels": set,
        "gone": patch.gone,
        "facts": facts,
        "faces": faces,
    })
    .to_string()
}

/// One event, as the wire carries it.
///
/// The data is split on newlines because that is what the format means by a line;
/// compact JSON never holds one, so this costs nothing and cannot be caught out
/// by a payload that does.
fn frame(event: &str, data: &str, id: Option<&str>) -> String {
    let mut frame = String::new();
    if let Some(id) = id {
        frame.push_str(&format!("id: {id}\n"));
    }
    frame.push_str(&format!("event: {event}\n"));
    for line in data.split('\n') {
        frame.push_str(&format!("data: {line}\n"));
    }
    frame.push('\n');
    frame
}

/// The response head for a stream, written by hand because a stream has no length
/// to declare and tiny_http has no streaming response of its own.
///
/// Chunked rather than closed-delimited, so an intermediary is told where each
/// event ends rather than having to wait for the connection to end to find out.
/// `X-Accel-Buffering` says the same thing to the proxies that read it.
fn head() -> String {
    [
        "HTTP/1.1 200 OK",
        "Content-Type: text/event-stream; charset=utf-8",
        "Cache-Control: no-store",
        "Connection: close",
        "Transfer-Encoding: chunked",
        "X-Accel-Buffering: no",
        "",
        "",
    ]
    .join("\r\n")
}

/// One chunk, flushed at once: a frame held in a buffer is a change the reader
/// has not been told about.
fn chunk(writer: &mut dyn Write, text: &str) -> io::Result<()> {
    write!(writer, "{:x}\r\n{text}\r\n", text.len())?;
    writer.flush()
}

/// What the browser resends after a dropped connection, which is the last state
/// it was told about. It is preferred over the page's own `from`, since the page
/// may have been following for hours by then.
fn resumed(request: &Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Last-Event-ID"))
        .map(|header| header.value.as_str().to_owned())
}

/// The state of a session file: when it was last written, and how long it was.
/// A page reports it back in a word, so a catch-up can be measured without the
/// server keeping any history.
///
/// It is *ordered*, and that is what lets two settings in flight at once be told
/// apart. A session file is only ever appended to, so a later read is a larger
/// stamp, and a setting made from an older read can be recognised as such rather
/// than committed over a newer one.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Stamp {
    modified: u128,
    length: u64,
}

impl Stamp {
    /// `None` for a file that is not there to read, which is not a state any
    /// render matches and so is never a setting anyone is holding.
    fn of(path: &Path) -> Option<Self> {
        let metadata = path.metadata().ok()?;
        Some(Self {
            modified: metadata.modified().map(nanos).unwrap_or(0),
            length: metadata.len(),
        })
    }

    /// The one word a page carries and reports back.
    fn word(&self) -> String {
        format!("{}-{}", self.modified, self.length)
    }
}

fn nanos(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or(0)
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "session".to_owned())
}

/// Reads a session, asking again if it will not parse.
///
/// A live session is caught between a line being written and its newline often
/// enough to matter, and that line is complete a moment later, so this retries
/// the same read rather than reaching for a different answer.
fn read(path: &Path) -> Result<Folio> {
    let mut last = None;
    for attempt in 0..PATIENCE {
        match Folio::read(path) {
            Ok(folio) => return Ok(folio),
            Err(error) => last = Some(error),
        }
        if attempt + 1 < PATIENCE {
            thread::sleep(BREATH);
        }
    }
    Err(last.expect("a read that never succeeded left an error behind"))
}

fn respond<R: io::Read>(request: Request, response: Response<R>) {
    let _ = request.respond(response);
}

/// A page is never cached: it is the session as it stood a moment ago, and the
/// stream it names takes it from there.
fn page_response(page: String) -> Response<io::Cursor<Vec<u8>>> {
    Response::from_string(page)
        .with_header(header("Content-Type", "text/html; charset=utf-8"))
        .with_header(header("Cache-Control", "no-store"))
}

/// An asset is cached forever, because its URL names its contents: a stylesheet
/// edited between two runs of the server is a different URL, so no reader can be
/// left holding the old one (see [`Resource`] on a stale token).
fn asset_response(asset: Asset) -> Response<io::Cursor<Vec<u8>>> {
    // The faces are a copy of the fonts, so they carry the copyright notice a
    // written folio carries as a comment above its markup.
    let body = match asset.is_faces() {
        true => format!("{}{}", render::faces_notice(), asset.body()),
        false => asset.body().to_owned(),
    };
    Response::from_string(body)
        .with_header(header("Content-Type", asset.mime()))
        .with_header(header(
            "Cache-Control",
            "public, max-age=31536000, immutable",
        ))
}

fn missing() -> Response<io::Cursor<Vec<u8>>> {
    Response::from_string("not in this codex")
        .with_status_code(404)
        .with_header(header("Content-Type", "text/plain; charset=utf-8"))
}

/// A session that would not parse after every retry. The message is the parse
/// error, file and line included, because that is what says which line of which
/// session to look at.
fn failed(error: &anyhow::Error) -> Response<io::Cursor<Vec<u8>>> {
    Response::from_string(format!("{error:#}"))
        .with_status_code(500)
        .with_header(header("Content-Type", "text/plain; charset=utf-8"))
}

fn header(field: &str, value: &str) -> Header {
    Header::from_bytes(field.as_bytes(), value.as_bytes())
        .expect("a header this crate spells out is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Stream;

    fn set(panels: &[(usize, &str)]) -> Vec<(usize, String)> {
        panels
            .iter()
            .map(|(turn, markup)| (*turn, (*markup).to_owned()))
            .collect()
    }

    #[test]
    fn the_root_is_the_listing() {
        assert_eq!(route("/"), Resource::Shelf);
        assert_eq!(route(""), Resource::Shelf);
    }

    #[test]
    fn a_quire_and_a_folio_are_named_by_id() {
        assert_eq!(
            route("/quire/-srv-alpha"),
            Resource::Quire("-srv-alpha".to_owned())
        );
        assert_eq!(
            route("/folio/abc-123"),
            Resource::Folio("abc-123".to_owned())
        );
    }

    #[test]
    fn an_asset_is_served_only_under_the_token_naming_its_contents() {
        let url = Asset::Style.url();

        assert_eq!(route(&url), Resource::Asset(Asset::Style));
        assert_eq!(
            route("/asset/0000000000000000/illumination.css"),
            Resource::Missing
        );
        assert_eq!(
            route(&format!("/asset/{}/nothing.css", render::ASSET_TOKEN)),
            Resource::Missing
        );
    }

    /// The renderer spells these URLs and this parses them, so the two are held
    /// together here rather than trusted to agree.
    #[test]
    fn every_stream_a_page_can_name_is_routed_back() {
        assert_eq!(
            route(&Stream::Shelf.url()),
            Resource::Live(Watching::Shelf, None)
        );
        assert_eq!(
            route(&Stream::Quire("-srv-alpha").url()),
            Resource::Live(Watching::Quire("-srv-alpha".to_owned()), None)
        );
        assert_eq!(
            route(
                &Stream::Folio {
                    session: "abc",
                    from: "17-42"
                }
                .url()
            ),
            Resource::Live(Watching::Folio("abc".to_owned()), Some("17-42".to_owned()))
        );
    }

    /// `serve` takes an arbitrary path, so a session's stem is not always a
    /// Claude Code uuid. The renderer spells the id into the URL and this reads
    /// it back out, and a mismatch is a folio that silently never updates rather
    /// than an error anyone would see, so the round trip is held here.
    #[test]
    fn an_id_that_needs_encoding_reaches_the_listing_as_itself() {
        let session = "my session #2";

        assert_eq!(
            route(&format!("/folio/{}", render::encoded(session))),
            Resource::Folio(session.to_owned())
        );
        assert_eq!(
            route(
                &Stream::Folio {
                    session,
                    from: "17-42"
                }
                .url()
            ),
            Resource::Live(
                Watching::Folio(session.to_owned()),
                Some("17-42".to_owned())
            )
        );
    }

    #[test]
    fn nothing_else_is_routed_anywhere() {
        for url in [
            "/folio",
            "/folio/abc/extra",
            "/../etc/passwd",
            "/quire",
            "/live/folio",
            "/livereload",
            "/asset",
            // Not valid percent-encoding, so it names no session.
            "/folio/%zz",
        ] {
            assert_eq!(route(url), Resource::Missing, "{url}");
        }
    }

    #[test]
    fn a_folio_that_gained_a_panel_sends_that_panel_alone() {
        let before = set(&[(1, "<a>"), (2, "<b>")]);
        let after = set(&[(1, "<a>"), (2, "<b>"), (5, "<c>")]);

        assert_eq!(
            patch(&before, &after),
            Patch {
                changed: vec![5],
                gone: Vec::new()
            }
        );
    }

    /// A tool result joins the panel holding its call, so the panel a reader
    /// already has is set again. Its turn number is unchanged, which is what lets
    /// the page replace it in place rather than append a second copy.
    #[test]
    fn a_panel_set_again_is_sent_again_under_the_same_number() {
        let before = set(&[(1, "<a>"), (2, "<b>")]);
        let after = set(&[(1, "<a>"), (2, "<b and its result>")]);

        assert_eq!(
            patch(&before, &after),
            Patch {
                changed: vec![2],
                gone: Vec::new()
            }
        );
    }

    #[test]
    fn a_folio_that_did_not_change_says_nothing() {
        let held = set(&[(1, "<a>"), (2, "<b>")]);

        assert!(patch(&held, &held).is_empty());
    }

    /// A panel can leave: a turn whose every block the folio drops stops being a
    /// panel, and a page holding one has to be told rather than left showing it.
    #[test]
    fn a_panel_the_folio_no_longer_holds_is_named_as_gone() {
        let before = set(&[(1, "<a>"), (2, "<b>")]);
        let after = set(&[(1, "<a>")]);

        assert_eq!(
            patch(&before, &after),
            Patch {
                changed: Vec::new(),
                gone: vec![2]
            }
        );
    }

    /// Two settings of one folio can be in flight at once, each read outside the
    /// lock, so the one that commits second is not always the newer. A session
    /// file is only ever appended to, so the stamp is what says which is which.
    #[test]
    fn a_session_that_grew_carries_the_larger_stamp() {
        let read = Stamp {
            modified: 17,
            length: 400,
        };
        let appended = Stamp {
            modified: 17,
            length: 900,
        };
        let rewritten = Stamp {
            modified: 18,
            length: 400,
        };

        assert!(appended > read);
        assert!(rewritten > appended);
    }

    /// A file that is not there has no state to name, which is what keeps a
    /// deleted session from being read again on every tick for as long as
    /// somebody has its page open.
    #[test]
    fn a_session_that_is_not_there_has_no_stamp() {
        assert_eq!(Stamp::of(Path::new("nowhere/at/all.jsonl")), None);
    }

    #[test]
    fn a_stamp_is_carried_as_one_word() {
        assert_eq!(
            Stamp {
                modified: 17,
                length: 42
            }
            .word(),
            "17-42"
        );
    }

    #[test]
    fn a_frame_carries_its_event_its_data_and_the_state_it_names() {
        assert_eq!(
            frame("panels", r#"{"turn":1}"#, Some("17-42")),
            "id: 17-42\nevent: panels\ndata: {\"turn\":1}\n\n"
        );
    }

    /// The format is line-oriented, so a payload holding a newline has to become
    /// two data lines rather than one broken frame.
    #[test]
    fn a_frame_breaks_its_data_across_lines_the_way_the_format_does() {
        assert_eq!(
            frame("listing", "one\ntwo", None),
            "event: listing\ndata: one\ndata: two\n\n"
        );
    }

    #[test]
    fn a_stream_declares_itself_unbuffered_and_chunked() {
        let head = head();

        assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(head.contains("Content-Type: text/event-stream"));
        assert!(head.contains("Transfer-Encoding: chunked"));
        assert!(head.ends_with("\r\n\r\n"));
    }

    #[test]
    fn a_chunk_states_its_length_in_hex() {
        let mut written = Vec::new();

        chunk(&mut written, ": hi\n\n").unwrap();

        assert_eq!(String::from_utf8(written).unwrap(), "6\r\n: hi\n\n\r\n");
    }
}
