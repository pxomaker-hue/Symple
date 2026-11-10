//! Moteur de synchronisation : scan, comparaison, copie, corbeille, conflits.

use crate::config::{ConflictPolicy, PairConfig, SyncMode};
use crate::filter::Filter;
use crate::platform::{is_network, join, long_path};
use crate::report::*;
use crate::tr;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Tolérance de comparaison des dates (FAT/SMB arrondissent à 2 s).
const MTIME_TOLERANCE_MS: i64 = 2000;
/// Au-delà, copie par blocs pour suivre la progression.
const CHUNKED_COPY_MIN: u64 = 16 * 1024 * 1024;
const CHUNK: usize = 4 * 1024 * 1024;
const PROGRESS_EVERY_MS: i64 = 200;
pub const TMP_SUFFIX: &str = ".symple-tmp";
pub const TRASH_DIR: &str = ".symple-trash";
pub const VERSIONS_DIR: &str = ".symple-versions";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Meta {
    pub size: u64,
    /// Date de modification en millisecondes Unix.
    pub mtime: i64,
    pub dir: bool,
    /// Fichier « en ligne uniquement » (contenu non téléchargé par le client cloud).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub online: bool,
}

impl Meta {
    fn from_fs(m: &fs::Metadata) -> Self {
        Meta { size: if m.is_dir() { 0 } else { m.len() }, mtime: mtime_ms(m), dir: m.is_dir(), online: !m.is_dir() && is_online_only(m) }
    }
}

/// Attributs Windows des fichiers cloud non téléchargés :
/// OFFLINE (0x1000), RECALL_ON_OPEN (0x40000), RECALL_ON_DATA_ACCESS (0x400000).
#[cfg(windows)]
fn is_online_only(m: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & (0x1000 | 0x40000 | 0x40_0000) != 0
}

#[cfg(not(windows))]
fn is_online_only(_: &fs::Metadata) -> bool {
    false
}

fn mtime_ms(m: &fs::Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn same(a: &Meta, b: &Meta) -> bool {
    a.dir == b.dir && (a.dir || (a.size == b.size && (a.mtime - b.mtime).abs() <= MTIME_TOLERANCE_MS))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
    fn idx(self) -> usize {
        self as usize
    }
}

type Tree = HashMap<String, Meta>;

#[derive(Debug, Clone)]
enum Op {
    Copy(Side, String, Meta), // source, chemin relatif, méta de la source
    Trash(Side, String),
    Backup(Side, String),             // renomme la version perdante en « (conflit …) »
    Move(Side, String, String, Meta), // renommage côté `Side` : de → vers
    Record(String, Meta),
    Forget(String),
}

/// Résultat de la planification, avant exécution.
struct Plan {
    online_skipped: usize,
    ops: Vec<Op>,
    dir_checks: Vec<(String, Side)>,
    asks: Vec<(String, Meta, Meta)>,
}

pub struct Engine {
    pub cfg: PairConfig,
    filter: Filter,
    roots: [PathBuf; 2],
    network: [bool; 2],
    state: Tree,
    dirty: bool,
    state_path: PathBuf,
    pending: HashMap<String, Conflict>,
}

impl Engine {
    pub fn new(cfg: PairConfig, state_dir: &Path) -> Self {
        let state_path = state_dir.join(format!("{}.json", cfg.id));
        let state = fs::read(&state_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Engine {
            filter: Filter::new(&cfg.excludes),
            roots: [long_path(&cfg.left), long_path(&cfg.right)],
            network: [is_network(&cfg.left), is_network(&cfg.right)],
            cfg,
            state,
            dirty: false,
            state_path,
            pending: HashMap::new(),
        }
    }

    pub fn root(&self, side: Side) -> &Path {
        &self.roots[side.idx()]
    }

    fn path(&self, side: Side, rel: &str) -> PathBuf {
        join(self.root(side), rel)
    }

    pub fn is_ignored(&self, rel: &str) -> bool {
        self.filter.ignored(rel)
    }

    pub fn pending_conflicts(&self) -> Vec<Conflict> {
        self.pending.values().cloned().collect()
    }

    fn check_roots(&self) -> Result<(), String> {
        for side in [Side::Left, Side::Right] {
            if !self.root(side).is_dir() {
                let p = if side == Side::Left { &self.cfg.left } else { &self.cfg.right };
                return Err(tr!("Dossier inaccessible : {}", "Folder unavailable: {}", p.display()));
            }
        }
        Ok(())
    }

    /// Ce que ferait une synchro complète, sans rien modifier.
    pub fn preview(&self) -> Result<Preview, String> {
        self.check_roots()?;
        let plan = self.plan(None)?;
        let mut pv = Preview { online_skipped: plan.online_skipped, ..Default::default() };
        let mut weights: HashMap<&str, (usize, u64)> = HashMap::new();
        let push = |pv: &mut Preview, kind, rel: &str, size| {
            if pv.items.len() < PREVIEW_LIMIT {
                pv.items.push(PreviewItem { kind, rel: rel.to_string(), size });
            } else {
                pv.truncated = true;
            }
        };
        for op in &plan.ops {
            match op {
                Op::Copy(side, rel, m) if !m.dir => {
                    let kind = if *side == Side::Left { PreviewKind::ToRight } else { PreviewKind::ToLeft };
                    if kind == PreviewKind::ToRight { pv.to_right += 1 } else { pv.to_left += 1 }
                    pv.bytes += m.size;
                    let top = rel.split_once('/').map_or("", |(d, _)| d);
                    let w = weights.entry(top).or_default();
                    w.0 += 1;
                    w.1 += m.size;
                    push(&mut pv, kind, rel, m.size);
                }
                Op::Trash(side, rel) => {
                    pv.deletes += 1;
                    let kind = if *side == Side::Left { PreviewKind::DeleteLeft } else { PreviewKind::DeleteRight };
                    push(&mut pv, kind, rel, 0);
                }
                Op::Move(side, from, to, _) => {
                    pv.moves += 1;
                    let kind = if *side == Side::Left { PreviewKind::MoveLeft } else { PreviewKind::MoveRight };
                    push(&mut pv, kind, &format!("{from} → {to}"), 0);
                }
                Op::Backup(_, rel) => {
                    pv.conflicts += 1;
                    push(&mut pv, PreviewKind::Conflict, rel, 0);
                }
                _ => {}
            }
        }
        for (rel, _, _) in &plan.asks {
            pv.conflicts += 1;
            push(&mut pv, PreviewKind::Conflict, rel, 0);
        }
        let mut top: Vec<DirWeight> =
            weights.into_iter().map(|(d, (files, bytes))| DirWeight { dir: d.to_string(), files, bytes }).collect();
        top.sort_unstable_by_key(|d| std::cmp::Reverse(d.bytes));
        top.truncate(8);
        pv.top_dirs = top;
        Ok(pv)
    }

    /// Synchronise toute la paire (`scope = None`) ou seulement certains chemins relatifs.
    pub fn sync(&mut self, scope: Option<&[String]>, rep: &dyn Reporter) -> Result<Stats, String> {
        self.check_roots()?;
        let Plan { ops, mut dir_checks, asks, .. } = self.plan(scope)?;
        let id = self.cfg.id.clone();

        // Conflits disparus (fichiers redevenus identiques ou supprimés)
        let current: BTreeSet<&String> = asks.iter().map(|c| &c.0).collect();
        self.pending.retain(|k, _| current.contains(k) || scope.is_some_and(|sc| !sc.iter().any(|p| in_scope(k, p))));

        let mut stats = Stats::default();
        for (rel, l, r) in asks {
            stats.conflicts += 1;
            let c = Conflict { pair_id: id.clone(), rel: rel.clone(), left: l, right: r };
            if self.pending.insert(rel, c.clone()).is_none() {
                rep.conflict(&c);
            }
        }

        self.execute(ops, &mut stats, rep);

        // Dossiers supprimés d'un côté : on les met à la corbeille de l'autre côté s'ils sont vides.
        dir_checks.sort_by_key(|d| std::cmp::Reverse(d.0.len()));
        for (rel, present) in dir_checks {
            let p = self.path(present, &rel);
            let other = self.path(present.other(), &rel);
            if other.is_dir() {
                if let Ok(m) = fs::metadata(&p) {
                    self.set_state(rel, Meta::from_fs(&m));
                }
            } else if self.dir_is_empty(&p) {
                match self.trash(present, &rel) {
                    Ok(()) => {
                        stats.deleted += 1;
                        self.del_state(&rel);
                    }
                    Err(e) => self.err(rep, &mut stats, tr!("Suppression de {} : {}", "Deleting {}: {}", rel, e)),
                }
            } else if fs::create_dir_all(&other).is_ok() {
                if let Ok(m) = fs::metadata(&p) {
                    self.set_state(rel, Meta::from_fs(&m));
                }
            }
        }

        stats.files = self.state.values().filter(|m| !m.dir).count();
        self.save_state();
        Ok(stats)
    }

    /// Résout un conflit en attente.
    pub fn resolve(&mut self, rel: &str, choice: ConflictChoice, rep: &dyn Reporter) -> Result<(), String> {
        self.pending.remove(rel).ok_or_else(|| tr!("Conflit introuvable", "Conflict not found"))?;
        let stat = |side: Side| fs::metadata(self.path(side, rel)).map(|m| Meta::from_fs(&m)).map_err(|e| e.to_string());
        let (l, r) = (stat(Side::Left)?, stat(Side::Right)?);
        let (win, meta) = match choice {
            ConflictChoice::KeepLeft => (Side::Left, l),
            ConflictChoice::KeepRight => (Side::Right, r),
            ConflictChoice::KeepBoth => {
                if newer_left(&l, &r) { (Side::Left, l) } else { (Side::Right, r) }
            }
        };
        let mut ops = Vec::new();
        if choice == ConflictChoice::KeepBoth {
            ops.push(Op::Backup(win.other(), rel.to_string()));
        }
        ops.push(Op::Copy(win, rel.to_string(), meta));
        let mut stats = Stats::default();
        self.execute(ops, &mut stats, rep);
        self.save_state();
        if stats.errors > 0 { Err(tr!("Échec de la résolution de {}", "Could not resolve {}", rel)) } else { Ok(()) }
    }

    // ---------------------------------------------------------- planification

    fn plan(&self, scope: Option<&[String]>) -> Result<Plan, String> {
        let (left, right) = std::thread::scope(|s| {
            let l = s.spawn(|| self.collect(Side::Left, scope));
            let r = self.collect(Side::Right, scope);
            (l.join().unwrap_or_default(), r)
        });

        // Garde-fou : un côté vide alors que l'historique est plein = partage non monté.
        if scope.is_none() && self.state.len() > 10 && (left.is_empty() || right.is_empty()) {
            return Err(tr!(
                "Un des dossiers est vide alors qu'il contenait des fichiers : synchro suspendue par sécurité.",
                "One folder is suddenly empty although it contained files: sync paused for safety."
            ));
        }

        let mut keys: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
        let state_keys: Vec<&String> = match scope {
            None => self.state.keys().collect(),
            Some(sc) => self.state.keys().filter(|k| sc.iter().any(|p| in_scope(k, p))).collect(),
        };
        keys.extend(state_keys);

        let mut ops = Vec::new();
        let mut dir_checks = Vec::new();
        let mut conflicts = Vec::new();
        let mut online_skipped = 0;
        for rel in keys {
            let (l, r, s) = (left.get(rel), right.get(rel), self.state.get(rel));
            // Fichier cloud non téléchargé : on n'y touche pas (ni copie, ni suppression, ni historique).
            if !self.cfg.download_online_only && (l.is_some_and(|m| m.online) || r.is_some_and(|m| m.online)) {
                online_skipped += 1;
                continue;
            }
            match self.cfg.mode {
                SyncMode::Mirror => plan_mirror(rel, l, r, self.cfg.propagate_deletes, &mut ops),
                SyncMode::Bidirectional => {
                    plan_bidir(rel, l, r, s, self.cfg.propagate_deletes, &mut ops, &mut dir_checks, &mut conflicts)
                }
            }
        }

        // Politique de conflit. Sans historique (1re synchro), « Demander » devient
        // « plus récent + copie » pour éviter une avalanche de questions.
        let mut asks = Vec::new();
        for (rel, l, r, first) in conflicts {
            let policy = if first && self.cfg.conflict == ConflictPolicy::Ask {
                ConflictPolicy::NewestWithBackup
            } else {
                self.cfg.conflict
            };
            if policy == ConflictPolicy::Ask {
                asks.push((rel, l, r));
                continue;
            }
            let win = if newer_left(&l, &r) { Side::Left } else { Side::Right };
            if policy == ConflictPolicy::NewestWithBackup {
                ops.push(Op::Backup(win.other(), rel.clone()));
            }
            ops.push(Op::Copy(win, rel, if win == Side::Left { l } else { r }));
        }

        detect_moves(&mut ops, [&left, &right]);
        Ok(Plan { online_skipped, ops, dir_checks, asks })
    }

    // ---------------------------------------------------------------- scan

    fn collect(&self, side: Side, scope: Option<&[String]>) -> Tree {
        let mut out = Tree::new();
        match scope {
            None => self.walk(side, "", &mut out),
            Some(rels) => {
                for rel in rels {
                    if rel.is_empty() {
                        self.walk(side, "", &mut out);
                        continue;
                    }
                    if self.filter.ignored(rel) {
                        continue;
                    }
                    if let Ok(m) = fs::symlink_metadata(self.path(side, rel)) {
                        if m.file_type().is_symlink() {
                            continue;
                        }
                        out.insert(rel.clone(), Meta::from_fs(&m));
                        if m.is_dir() {
                            self.walk(side, rel, &mut out);
                        }
                    }
                }
            }
        }
        out
    }

    fn walk(&self, side: Side, start: &str, out: &mut Tree) {
        let mut stack = vec![start.to_string()];
        while let Some(rel) = stack.pop() {
            let Ok(rd) = fs::read_dir(self.path(side, &rel)) else { continue };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let child = if rel.is_empty() { name } else { format!("{rel}/{name}") };
                if self.filter.ignored(&child) {
                    continue;
                }
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_symlink() {
                    continue;
                }
                let Ok(m) = e.metadata() else { continue };
                if ft.is_dir() {
                    stack.push(child.clone());
                }
                out.insert(child, Meta::from_fs(&m));
            }
        }
    }

    fn dir_is_empty(&self, p: &Path) -> bool {
        fs::read_dir(p)
            .map(|rd| rd.flatten().all(|e| self.filter.ignored(&e.file_name().to_string_lossy())))
            .unwrap_or(false)
    }

    // ------------------------------------------------------------ exécution

    fn execute(&mut self, ops: Vec<Op>, stats: &mut Stats, rep: &dyn Reporter) {
        let mut copies = Vec::new();
        let mut trashes = Vec::new();
        let mut moves = Vec::new();
        for op in ops {
            match op {
                Op::Record(rel, m) => self.set_state(rel, m),
                Op::Forget(rel) => self.del_state(&rel),
                Op::Backup(side, rel) => match backup(&self.path(side, &rel)) {
                    Ok(name) => self.log(
                        rep,
                        Level::Warn,
                        tr!("Conflit sur {} : ancienne version conservée ({})", "Conflict on {}: previous version kept ({})", rel, name),
                    ),
                    Err(e) => self.err(rep, stats, tr!("Sauvegarde de {} : {}", "Backing up {}: {}", rel, e)),
                },
                Op::Copy(side, rel, m) => copies.push((side, rel, m)),
                Op::Trash(side, rel) => trashes.push((side, rel)),
                Op::Move(side, from, to, m) => moves.push((side, from, to, m)),
            }
        }

        // Renommages / déplacements : simple renommage de l'autre côté, sans recopie.
        for (side, from, to, m) in moves {
            let (src, dst) = (self.path(side, &from), self.path(side, &to));
            let res = dst.parent().map_or(Ok(()), fs::create_dir_all).and_then(|_| {
                if dst.exists() { Err(io::Error::from(io::ErrorKind::AlreadyExists)) } else { fs::rename(&src, &dst) }
            });
            match res {
                Ok(()) => {
                    stats.moved += 1;
                    self.log(rep, Level::Info, format!("↪ {from} → {to}"));
                    self.del_state(&from);
                    self.set_state(to, m);
                }
                Err(_) => {
                    // Repli : copie + suppression classiques.
                    copies.push((side.other(), to, m));
                    trashes.push((side, from));
                }
            }
        }

        // Dossiers d'abord (dans l'ordre), puis fichiers en parallèle.
        let (dirs, files): (Vec<_>, Vec<_>) = copies.into_iter().partition(|c| c.2.dir);
        for (side, rel, m) in dirs {
            match fs::create_dir_all(self.path(side.other(), &rel)) {
                Ok(()) => self.set_state(rel, m),
                Err(e) => self.err(rep, stats, tr!("Création de {} : {}", "Creating {}: {}", rel, e)),
            }
        }

        let progress = ProgressTracker::new(&self.cfg.id, files.len(), files.iter().map(|f| f.2.size).sum());
        let versions = self.cfg.versions;
        let this = &*self;
        let results: Vec<_> = files
            .into_par_iter()
            .map(|(side, rel, m)| {
                let (src, dst) = (this.path(side, &rel), this.path(side.other(), &rel));
                let keep = (versions > 0).then(|| (this.root(side.other()), rel.as_str(), versions));
                let res = copy_file(&src, &dst, &m, keep, &|n| progress.add_bytes(n, rep));
                progress.file_done(rep);
                (side, rel, m, res)
            })
            .collect();
        progress.finish(rep);
        for (side, rel, m, res) in results {
            match res {
                Ok(()) => {
                    stats.copied += 1;
                    stats.bytes += m.size;
                    let arrow = if side == Side::Left { "→" } else { "←" };
                    self.log(rep, Level::Info, format!("{arrow} {rel}"));
                    self.set_state(rel, m);
                }
                Err(e) => self.err(rep, stats, tr!("Copie de {} : {}", "Copying {}: {}", rel, e)),
            }
        }

        for (side, rel) in trashes {
            match self.trash(side, &rel) {
                Ok(()) => {
                    stats.deleted += 1;
                    self.log(rep, Level::Info, format!("🗑 {rel}"));
                    self.del_state(&rel);
                }
                Err(e) => self.err(rep, stats, tr!("Suppression de {} : {}", "Deleting {}: {}", rel, e)),
            }
        }
    }

    fn trash(&self, side: Side, rel: &str) -> io::Result<()> {
        let p = self.path(side, rel);
        if !p.exists() {
            return Ok(());
        }
        // La corbeille Windows n'existe pas sur les partages et lecteurs réseau : dossier .symple-trash.
        if !self.network[side.idx()] {
            let plain = if side == Side::Left { &self.cfg.left } else { &self.cfg.right };
            if trash::delete(join(plain, rel)).is_ok() {
                return Ok(());
            }
        }
        let dst = unique(join(&self.root(side).join(TRASH_DIR).join(stamp(false)), rel));
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&p, &dst)
    }

    fn set_state(&mut self, rel: String, m: Meta) {
        if self.state.get(&rel) != Some(&m) {
            self.state.insert(rel, m);
            self.dirty = true;
        }
    }

    fn del_state(&mut self, rel: &str) {
        if self.state.remove(rel).is_some() {
            self.dirty = true;
        }
    }

    /// N'écrit l'historique que s'il a changé.
    fn save_state(&mut self) {
        if !self.dirty {
            return;
        }
        if let Some(p) = self.state_path.parent() {
            let _ = fs::create_dir_all(p);
        }
        if let Ok(b) = serde_json::to_vec(&self.state) {
            let tmp = self.state_path.with_extension("tmp");
            if fs::write(&tmp, b).is_ok() && fs::rename(tmp, &self.state_path).is_ok() {
                self.dirty = false;
            }
        }
    }

    fn log(&self, rep: &dyn Reporter, level: Level, msg: String) {
        rep.log(&self.cfg.id, level, msg);
    }

    fn err(&self, rep: &dyn Reporter, stats: &mut Stats, msg: String) {
        stats.errors += 1;
        rep.log(&self.cfg.id, Level::Error, msg);
    }
}

// ------------------------------------------------------------ progression

struct ProgressTracker {
    id: String,
    total_files: usize,
    total_bytes: u64,
    files: AtomicUsize,
    bytes: AtomicU64,
    last: AtomicI64,
}

impl ProgressTracker {
    fn new(id: &str, total_files: usize, total_bytes: u64) -> Self {
        ProgressTracker {
            id: id.to_string(),
            total_files,
            total_bytes,
            files: AtomicUsize::new(0),
            bytes: AtomicU64::new(0),
            last: AtomicI64::new(0),
        }
    }

    fn add_bytes(&self, n: u64, rep: &dyn Reporter) {
        self.bytes.fetch_add(n, Ordering::Relaxed);
        self.emit(rep, false);
    }

    fn file_done(&self, rep: &dyn Reporter) {
        self.files.fetch_add(1, Ordering::Relaxed);
        self.emit(rep, false);
    }

    fn finish(&self, rep: &dyn Reporter) {
        if self.total_files > 0 {
            self.emit(rep, true);
        }
    }

    fn emit(&self, rep: &dyn Reporter, force: bool) {
        let now = now_ms();
        let last = self.last.load(Ordering::Relaxed);
        if !force && now - last < PROGRESS_EVERY_MS {
            return;
        }
        if !force && self.last.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_err() {
            return;
        }
        rep.progress(&Progress {
            pair_id: self.id.clone(),
            done_files: self.files.load(Ordering::Relaxed),
            total_files: self.total_files,
            done_bytes: self.bytes.load(Ordering::Relaxed),
            total_bytes: self.total_bytes,
        });
    }
}

// --------------------------------------------------------------- planif.

fn in_scope(key: &str, scope: &str) -> bool {
    scope.is_empty() || key == scope || (key.starts_with(scope) && key.as_bytes().get(scope.len()) == Some(&b'/'))
}

fn newer_left(l: &Meta, r: &Meta) -> bool {
    l.mtime > r.mtime || (l.mtime == r.mtime && l.size >= r.size)
}

fn plan_mirror(rel: &str, l: Option<&Meta>, r: Option<&Meta>, del: bool, ops: &mut Vec<Op>) {
    match (l, r) {
        (Some(l), Some(r)) if same(l, r) => ops.push(Op::Record(rel.into(), *l)),
        (Some(l), _) => ops.push(Op::Copy(Side::Left, rel.into(), *l)),
        (None, Some(_)) if del => ops.push(Op::Trash(Side::Right, rel.into())),
        _ => ops.push(Op::Forget(rel.into())),
    }
}

#[allow(clippy::too_many_arguments)]
fn plan_bidir(
    rel: &str,
    l: Option<&Meta>,
    r: Option<&Meta>,
    s: Option<&Meta>,
    del: bool,
    ops: &mut Vec<Op>,
    dir_checks: &mut Vec<(String, Side)>,
    conflicts: &mut Vec<(String, Meta, Meta, bool)>,
) {
    let changed = |x: &Meta| s.is_none_or(|s| !same(x, s));
    match (l, r) {
        (None, None) => ops.push(Op::Forget(rel.into())),
        (Some(l), Some(r)) => {
            if same(l, r) {
                ops.push(Op::Record(rel.into(), *l));
            } else if l.dir || r.dir {
                if l.dir && r.dir {
                    ops.push(Op::Record(rel.into(), *l));
                }
                // fichier d'un côté, dossier de l'autre : on ne touche à rien.
            } else {
                match (changed(l), changed(r)) {
                    (true, false) => ops.push(Op::Copy(Side::Left, rel.into(), *l)),
                    (false, true) => ops.push(Op::Copy(Side::Right, rel.into(), *r)),
                    _ => conflicts.push((rel.into(), *l, *r, s.is_none())),
                }
            }
        }
        (Some(x), None) | (None, Some(x)) => {
            let side = if l.is_some() { Side::Left } else { Side::Right };
            let deleted_on_other = del && s.is_some();
            if x.dir {
                if deleted_on_other {
                    dir_checks.push((rel.into(), side));
                } else {
                    ops.push(Op::Copy(side, rel.into(), *x));
                }
            } else if deleted_on_other && !changed(x) {
                ops.push(Op::Trash(side, rel.into()));
            } else {
                ops.push(Op::Copy(side, rel.into(), *x));
            }
        }
    }
}

/// Un fichier supprimé d'un côté et un fichier identique (taille + date) apparu ailleurs
/// = renommage ou déplacement : on renomme de l'autre côté au lieu de recopier.
fn detect_moves(ops: &mut Vec<Op>, trees: [&Tree; 2]) {
    let mut trash_by_size: HashMap<(Side, u64), Vec<usize>> = HashMap::new();
    for (i, op) in ops.iter().enumerate() {
        if let Op::Trash(side, rel) = op {
            if let Some(m) = trees[side.idx()].get(rel).filter(|m| !m.dir && m.size > 0) {
                trash_by_size.entry((*side, m.size)).or_default().push(i);
            }
        }
    }
    if trash_by_size.is_empty() {
        return;
    }
    let mut replaced: HashMap<usize, Op> = HashMap::new();
    let mut removed = vec![false; ops.len()];
    for (i, op) in ops.iter().enumerate() {
        let Op::Copy(src, to, m) = op else { continue };
        let target = src.other();
        if m.dir || m.size == 0 || trees[target.idx()].contains_key(to) {
            continue;
        }
        let Some(cands) = trash_by_size.get_mut(&(target, m.size)) else { continue };
        let found = cands.iter().position(|&j| match &ops[j] {
            Op::Trash(_, from) => trees[target.idx()].get(from).is_some_and(|fm| same(fm, m)),
            _ => false,
        });
        if let Some(pos) = found {
            let j = cands.remove(pos);
            if let Op::Trash(_, from) = &ops[j] {
                replaced.insert(i, Op::Move(target, from.clone(), to.clone(), *m));
                removed[j] = true;
            }
        }
    }
    let old = std::mem::take(ops);
    *ops = old
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !removed[*i])
        .map(|(i, op)| replaced.remove(&i).unwrap_or(op))
        .collect();
}

// ----------------------------------------------------------------- fichiers

/// Copie atomique : fichier temporaire, date conservée, puis renommage.
/// `keep` : (racine de destination, chemin relatif, nb de versions) pour archiver l'ancienne version.
fn copy_file(src: &Path, dst: &Path, m: &Meta, keep: Option<(&Path, &str, u32)>, progress: &dyn Fn(u64)) -> io::Result<()> {
    if let Some(p) = dst.parent() {
        fs::create_dir_all(p)?;
    }
    let name = dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dst.with_file_name(format!(".{name}{TMP_SUFFIX}"));
    let res = (|| {
        if m.size >= CHUNKED_COPY_MIN {
            let mut r = fs::File::open(src)?;
            let mut w = fs::File::create(&tmp)?;
            let mut buf = vec![0u8; CHUNK];
            loop {
                let n = r.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                w.write_all(&buf[..n])?;
                progress(n as u64);
            }
            w.flush()?;
        } else {
            fs::copy(src, &tmp)?;
            progress(m.size);
        }
        let ft = filetime::FileTime::from_unix_time(m.mtime.div_euclid(1000), (m.mtime.rem_euclid(1000) * 1_000_000) as u32);
        filetime::set_file_mtime(&tmp, ft)?;
        // Le fichier source a-t-il bougé pendant la copie ? On réessaiera plus tard.
        let now = Meta::from_fs(&fs::metadata(src)?);
        if !same(&now, m) {
            return Err(io::Error::other(tr!(
                "fichier en cours d'écriture, nouvel essai au prochain passage",
                "file is being written, will retry on next pass"
            )));
        }
        if let Ok(md) = fs::metadata(dst) {
            let mut p = md.permissions();
            if p.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                p.set_readonly(false);
                let _ = fs::set_permissions(dst, p);
            }
            if let Some((root, rel, n)) = keep {
                archive_version(root, rel, dst, n)?;
            }
        }
        fs::rename(&tmp, dst)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// Déplace l'ancienne version dans `.symple-versions/` et ne garde que les `n` plus récentes.
fn archive_version(root: &Path, rel: &str, current: &Path, n: u32) -> io::Result<()> {
    let (dir_rel, file) = rel.rsplit_once('/').unwrap_or(("", rel));
    let vdir = join(&root.join(VERSIONS_DIR), dir_rel);
    fs::create_dir_all(&vdir)?;
    let (stem, ext) = split_ext(file);
    let dst = unique(vdir.join(format!("{stem} ({}){ext}", stamp(true))));
    fs::rename(current, &dst)?;

    let prefix = format!("{stem} (");
    let mut olds: Vec<String> = fs::read_dir(&vdir)?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|f| f.starts_with(&prefix) && f.ends_with(&ext) && f.len() == prefix.len() + 20 + ext.len())
        .collect();
    olds.sort_unstable_by(|a, b| b.cmp(a));
    for f in olds.into_iter().skip(n as usize) {
        let _ = fs::remove_file(vdir.join(f));
    }
    Ok(())
}

fn split_ext(file: &str) -> (&str, String) {
    match file.rfind('.') {
        Some(i) if i > 0 => (&file[..i], file[i..].to_string()),
        _ => (file, String::new()),
    }
}

/// Renomme le fichier en « nom (conflit AAAA-MM-JJ HHhMMmSS).ext ».
fn backup(p: &Path) -> io::Result<String> {
    if !p.exists() {
        return Ok(String::new());
    }
    let file = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = split_ext(&file);
    let word = tr!("conflit", "conflict");
    let dst = unique(p.with_file_name(format!("{stem} ({word} {}){ext}", stamp(true))));
    fs::rename(p, &dst)?;
    Ok(dst.file_name().unwrap_or_default().to_string_lossy().into_owned())
}

fn unique(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let file = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = split_ext(&file);
    (2..).map(|i| p.with_file_name(format!("{stem} ({i}){ext}"))).find(|c| !c.exists()).unwrap()
}

fn civil(secs: i64) -> (i64, i64, i64, i64, i64, i64) {
    // Conversion jours → date (algorithme de H. Hinnant), en UTC.
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// `AAAA-MM-JJ` ou `AAAA-MM-JJ HHhMMmSS` (19 caractères).
fn stamp(with_time: bool) -> String {
    let (y, m, d, h, mi, s) = civil(now_ms() / 1000);
    if with_time {
        format!("{y:04}-{m:02}-{d:02} {h:02}h{mi:02}m{s:02}")
    } else {
        format!("{y:04}-{m:02}-{d:02}")
    }
}
