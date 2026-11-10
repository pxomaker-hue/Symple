//! Exécution en tâche de fond d'une paire : surveillance temps réel + vérification périodique.

use crate::config::PairConfig;
use crate::engine::Engine;
use crate::report::{ConflictChoice, Level, PairStatus, Reporter, State};
use crate::tr;
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEBOUNCE: Duration = Duration::from_millis(350);

enum Cmd {
    Changed(Vec<PathBuf>),
    Full,
    Resolve(String, ConflictChoice),
    Pause(bool),
    Stop,
}

pub struct PairRunner {
    pub id: String,
    tx: Sender<Cmd>,
    handle: Option<JoinHandle<()>>,
}

impl PairRunner {
    pub fn start(cfg: PairConfig, state_dir: PathBuf, rep: Arc<dyn Reporter>) -> Self {
        let (tx, rx) = unbounded();
        let id = cfg.id.clone();
        let wtx = tx.clone();
        let handle = std::thread::Builder::new()
            .name(format!("symple-{id}"))
            .spawn(move || Worker::new(cfg, &state_dir, rep, wtx).run(rx))
            .ok();
        PairRunner { id, tx, handle }
    }

    pub fn sync_now(&self) {
        let _ = self.tx.send(Cmd::Full);
    }

    pub fn set_paused(&self, paused: bool) {
        let _ = self.tx.send(Cmd::Pause(paused));
    }

    pub fn resolve(&self, rel: String, choice: ConflictChoice) {
        let _ = self.tx.send(Cmd::Resolve(rel, choice));
    }
}

impl Drop for PairRunner {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

struct Worker {
    engine: Engine,
    rep: Arc<dyn Reporter>,
    tx: Sender<Cmd>,
    watcher: Option<RecommendedWatcher>,
    status: PairStatus,
    paused: bool,
    poll: Duration,
    backlog: VecDeque<Cmd>,
}

impl Worker {
    fn new(cfg: PairConfig, state_dir: &Path, rep: Arc<dyn Reporter>, tx: Sender<Cmd>) -> Self {
        let paused = !cfg.enabled;
        let poll = Duration::from_secs(cfg.poll_secs.max(2));
        let status = PairStatus {
            pair_id: cfg.id.clone(),
            state: if paused { State::Paused } else { State::Idle },
            last_sync: None,
            files: 0,
            conflicts: 0,
            watching: false,
            error: None,
        };
        Worker { engine: Engine::new(cfg, state_dir), rep, tx, watcher: None, status, paused, poll, backlog: VecDeque::new() }
    }

    fn run(mut self, rx: Receiver<Cmd>) {
        self.rep.status(&self.status);
        let mut need_full = true;
        loop {
            if need_full && !self.paused {
                self.sync(None);
                need_full = false;
            }
            let cmd = match self.backlog.pop_front() {
                Some(c) => c,
                None => match rx.recv_timeout(self.poll) {
                    Ok(c) => c,
                    Err(RecvTimeoutError::Timeout) => Cmd::Full,
                    Err(RecvTimeoutError::Disconnected) => return,
                },
            };
            match cmd {
                Cmd::Stop => return,
                Cmd::Full => need_full = true,
                Cmd::Pause(p) => {
                    self.paused = p;
                    if p {
                        self.watcher = None;
                        self.status.watching = false;
                        self.status.state = State::Paused;
                        self.rep.status(&self.status);
                    } else {
                        need_full = true;
                    }
                }
                Cmd::Resolve(rel, choice) => {
                    if let Err(e) = self.engine.resolve(&rel, choice, &*self.rep) {
                        self.rep.log(&self.engine.cfg.id, Level::Error, e);
                    }
                    self.status.conflicts = self.engine.pending_conflicts().len();
                    self.rep.status(&self.status);
                }
                Cmd::Changed(paths) => {
                    if self.paused {
                        continue;
                    }
                    // Regroupe les événements rapprochés.
                    let mut rels = BTreeSet::new();
                    let mut full = self.add_rels(&paths, &mut rels);
                    let deadline = Instant::now() + DEBOUNCE;
                    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                        match rx.recv_timeout(left) {
                            Ok(Cmd::Changed(p)) => full |= self.add_rels(&p, &mut rels),
                            Ok(other) => self.backlog.push_back(other),
                            Err(_) => break,
                        }
                    }
                    if full {
                        need_full = true;
                    } else if !rels.is_empty() {
                        let rels: Vec<String> = rels.into_iter().collect();
                        self.sync(Some(&rels));
                    }
                }
            }
        }
    }

    /// Convertit les chemins absolus en chemins relatifs. Renvoie `true` si un scan complet est nécessaire.
    fn add_rels(&self, paths: &[PathBuf], out: &mut BTreeSet<String>) -> bool {
        for p in paths {
            let rel = [&self.engine.cfg.left, &self.engine.cfg.right]
                .iter()
                .find_map(|root| p.strip_prefix(root).ok())
                .map(|r| r.to_string_lossy().replace('\\', "/"));
            match rel {
                Some(r) if r.is_empty() => return true,
                Some(r) if !self.engine.is_ignored(&r) => {
                    out.insert(r);
                }
                _ => {}
            }
        }
        false
    }

    fn sync(&mut self, scope: Option<&[String]>) {
        if self.watcher.is_none() {
            self.watcher = self.make_watcher();
            self.status.watching = self.watcher.is_some();
        }
        self.status.state = State::Syncing;
        self.rep.status(&self.status);
        match self.engine.sync(scope, &*self.rep) {
            Ok(stats) => {
                self.status.state = State::Idle;
                self.status.error = None;
                self.status.files = stats.files;
                self.status.last_sync = Some(now_ms());
                if stats.copied + stats.deleted + stats.moved > 0 {
                    self.rep.log(
                        &self.engine.cfg.id,
                        Level::Info,
                        tr!(
                            "Synchro : {} copié(s), {} déplacé(s), {} supprimé(s), {}",
                            "Sync: {} copied, {} moved, {} deleted, {}",
                            stats.copied,
                            stats.moved,
                            stats.deleted,
                            human(stats.bytes)
                        ),
                    );
                }
            }
            Err(e) => {
                if self.status.error.as_deref() != Some(&e) {
                    self.rep.log(&self.engine.cfg.id, Level::Error, e.clone());
                }
                self.status.state = State::Error;
                self.status.error = Some(e);
                // Dossier perdu (NAS débranché) : on recrée la surveillance plus tard.
                self.watcher = None;
                self.status.watching = false;
            }
        }
        self.status.conflicts = self.engine.pending_conflicts().len();
        self.rep.status(&self.status);
    }

    fn make_watcher(&self) -> Option<RecommendedWatcher> {
        let (l, r) = (&self.engine.cfg.left, &self.engine.cfg.right);
        if !l.is_dir() || !r.is_dir() {
            return None;
        }
        let tx = self.tx.clone();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let _ = match res {
                Ok(ev) if ev.need_rescan() => tx.send(Cmd::Full),
                Ok(ev) if ev.kind.is_access() => Ok(()),
                Ok(ev) => tx.send(Cmd::Changed(ev.paths)),
                Err(_) => tx.send(Cmd::Full),
            };
        })
        .ok()?;
        for root in [l, r] {
            if let Err(e) = w.watch(root, RecursiveMode::Recursive) {
                self.rep.log(
                    &self.engine.cfg.id,
                    Level::Warn,
                    tr!(
                        "Surveillance temps réel indisponible pour {} ({}) : vérification périodique seulement",
                        "Real-time watching unavailable for {} ({}): periodic checks only",
                        root.display(),
                        e
                    ),
                );
                return None;
            }
        }
        Some(w)
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn human(b: u64) -> String {
    let units: [&str; 5] = if crate::i18n::is_en() { ["B", "KB", "MB", "GB", "TB"] } else { ["o", "Ko", "Mo", "Go", "To"] };
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} {}", units[0]) } else { format!("{v:.1} {}", units[i]) }
}
