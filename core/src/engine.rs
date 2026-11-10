//! Moteur de synchronisation : scan, comparaison, copie, corbeille, conflits.

use crate::config::{ConflictPolicy, PairConfig, SyncMode};
use crate::filter::Filter;
use crate::report::{Conflict, ConflictChoice, Level, Reporter, Stats};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Tolérance de comparaison des dates (FAT/SMB arrondissent à 2 s).
const MTIME_TOLERANCE_MS: i64 = 2000;
pub const TMP_SUFFIX: &str = ".symple-tmp";
pub const TRASH_DIR: &str = ".symple-trash";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Meta {
    pub size: u64,
    /// Date de modification en millisecondes Unix.
    pub mtime: i64,
    pub dir: bool,
}

impl Meta {
    fn from_fs(m: &fs::Metadata) -> Self {
        Meta { size: if m.is_dir() { 0 } else { m.len() }, mtime: mtime_ms(m), dir: m.is_dir() }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

type Tree = HashMap<String, Meta>;

pub struct Engine {
    pub cfg: PairConfig,
    filter: Filter,
    state: Tree,
    state_path: PathBuf,
    pending: HashMap<String, Conflict>,
}

enum Op {
    Copy(Side, String, Meta), // source, chemin relatif, méta de la source
    Trash(Side, String),
    Backup(Side, String),     // renomme la version perdante en « (conflit …) »
    Record(String, Meta),
    Forget(String),
}

impl Engine {
    pub fn new(cfg: PairConfig, state_dir: &Path) -> Self {
        let state_path = state_dir.join(format!("{}.json", cfg.id));
        let state = fs::read(&state_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let filter = Filter::new(&cfg.excludes);
        Engine { cfg, filter, state, state_path, pending: HashMap::new() }
    }

    pub fn root(&self, side: Side) -> &Path {
        match side {
            Side::Left => &self.cfg.left,
            Side::Right => &self.cfg.right,
        }
    }

    pub fn is_ignored(&self, rel: &str) -> bool {
        self.filter.ignored(rel)
    }

    pub fn pending_conflicts(&self) -> Vec<Conflict> {
        self.pending.values().cloned().collect()
    }

    /// Synchronise toute la paire (`scope = None`) ou seulement certains chemins relatifs.
    pub fn sync(&mut self, scope: Option<&[String]>, rep: &dyn Reporter) -> Result<Stats, String> {
        let id = self.cfg.id.clone();
        for side in [Side::Left, Side::Right] {
            if !self.root(side).is_dir() {
                return Err(format!("Dossier inaccessible : {}", self.root(side).display()));
            }
        }

        let (left, right) = std::thread::scope(|s| {
            let l = s.spawn(|| self.collect(Side::Left, scope));
            let r = self.collect(Side::Right, scope);
            (l.join().unwrap_or_default(), r)
        });

        // Garde-fou : un côté vide alors que l'historique est plein = partage non monté.
        if scope.is_none() && self.state.len() > 10 && (left.is_empty() || right.is_empty()) {
            return Err("Un des dossiers est vide alors qu'il contenait des fichiers : synchro suspendue par sécurité.".into());
        }

        let mut keys: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
        let state_keys: Vec<String> = match scope {
            None => self.state.keys().cloned().collect(),
            Some(sc) => self.state.keys().filter(|k| sc.iter().any(|p| in_scope(k, p))).cloned().collect(),
        };
        keys.extend(state_keys.iter());

        let mut ops = Vec::new();
        let mut dir_checks: Vec<(String, Side)> = Vec::new(); // dossiers supprimés d'un côté
        let mut conflicts = Vec::new();

        for rel in keys {
            let (l, r, s) = (left.get(rel), right.get(rel), self.state.get(rel));
            match self.cfg.mode {
                SyncMode::Mirror => plan_mirror(rel, l, r, self.cfg.propagate_deletes, &mut ops),
                SyncMode::Bidirectional => plan_bidir(
                    rel, l, r, s, self.cfg.propagate_deletes, &mut ops, &mut dir_checks, &mut conflicts,
                ),
            }
        }

        // Conflits disparus (fichiers redevenus identiques ou supprimés)
        let current: BTreeSet<String> = conflicts.iter().map(|c| c.0.clone()).collect();
        self.pending.retain(|k, _| current.contains(k) || scope.is_some_and(|sc| !sc.iter().any(|p| in_scope(k, p))));

        let mut stats = Stats::default();
        for (rel, l, r) in conflicts {
            match self.cfg.conflict {
                ConflictPolicy::Ask => {
                    stats.conflicts += 1;
                    let c = Conflict { pair_id: id.clone(), rel: rel.clone(), left: l, right: r };
                    if self.pending.insert(rel, c.clone()).is_none() {
                        rep.conflict(&c);
                    }
                }
                policy => {
                    let win = if newer_left(&l, &r) { Side::Left } else { Side::Right };
                    if policy == ConflictPolicy::NewestWithBackup {
                        ops.push(Op::Backup(win.other(), rel.clone()));
                    }
                    ops.push(Op::Copy(win, rel, if win == Side::Left { l } else { r }));
                }
            }
        }

        self.execute(ops, &mut stats, rep);

        // Dossiers supprimés d'un côté : on les met à la corbeille de l'autre côté s'ils sont vides.
        dir_checks.sort_by_key(|d| std::cmp::Reverse(d.0.len()));
        for (rel, present) in dir_checks {
            let p = self.root(present).join(&rel);
            let other = self.root(present.other()).join(&rel);
            if other.is_dir() {
                if let Ok(m) = fs::metadata(&p) {
                    self.state.insert(rel, Meta::from_fs(&m));
                }
            } else if self.dir_is_empty(&p) {
                match self.trash(present, &rel) {
                    Ok(()) => {
                        stats.deleted += 1;
                        self.state.remove(&rel);
                    }
                    Err(e) => self.err(rep, &mut stats, format!("Suppression de {rel} : {e}")),
                }
            } else if fs::create_dir_all(&other).is_ok() {
                if let Ok(m) = fs::metadata(&p) {
                    self.state.insert(rel, Meta::from_fs(&m));
                }
            }
        }

        stats.files = self.state.values().filter(|m| !m.dir).count();
        self.save_state();
        Ok(stats)
    }

    /// Résout un conflit en attente.
    pub fn resolve(&mut self, rel: &str, choice: ConflictChoice, rep: &dyn Reporter) -> Result<(), String> {
        self.pending.remove(rel).ok_or("Conflit introuvable")?;
        let stat = |side: Side| fs::metadata(self.root(side).join(rel)).map(|m| Meta::from_fs(&m)).map_err(|e| e.to_string());
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
        if stats.errors > 0 { Err(format!("Échec de la résolution de {rel}")) } else { Ok(()) }
    }

    // ---------------------------------------------------------------- scan

    fn collect(&self, side: Side, scope: Option<&[String]>) -> Tree {
        let root = self.root(side);
        let mut out = Tree::new();
        match scope {
            None => self.walk(root, "", &mut out),
            Some(rels) => {
                for rel in rels {
                    if rel.is_empty() {
                        self.walk(root, "", &mut out);
                        continue;
                    }
                    if self.filter.ignored(rel) {
                        continue;
                    }
                    if let Ok(m) = fs::symlink_metadata(root.join(rel)) {
                        if m.file_type().is_symlink() {
                            continue;
                        }
                        out.insert(rel.clone(), Meta::from_fs(&m));
                        if m.is_dir() {
                            self.walk(root, rel, &mut out);
                        }
                    }
                }
            }
        }
        out
    }

    fn walk(&self, root: &Path, start: &str, out: &mut Tree) {
        let mut stack = vec![start.to_string()];
        while let Some(rel) = stack.pop() {
            let Ok(rd) = fs::read_dir(if rel.is_empty() { root.to_path_buf() } else { root.join(&rel) }) else { continue };
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
        for op in ops {
            match op {
                Op::Record(rel, m) => {
                    self.state.insert(rel, m);
                }
                Op::Forget(rel) => {
                    self.state.remove(&rel);
                }
                Op::Backup(side, rel) => match backup(self.root(side), &rel) {
                    Ok(name) => self.log(rep, Level::Warn, format!("Conflit sur {rel} : ancienne version conservée ({name})")),
                    Err(e) => self.err(rep, stats, format!("Sauvegarde de {rel} : {e}")),
                },
                Op::Copy(side, rel, m) => copies.push((side, rel, m)),
                Op::Trash(side, rel) => trashes.push((side, rel)),
            }
        }

        // Dossiers d'abord (dans l'ordre), puis fichiers en parallèle.
        let (dirs, files): (Vec<_>, Vec<_>) = copies.into_iter().partition(|c| c.2.dir);
        for (side, rel, m) in dirs {
            let dst = self.root(side.other()).join(&rel);
            match fs::create_dir_all(&dst) {
                Ok(()) => {
                    self.state.insert(rel, m);
                }
                Err(e) => self.err(rep, stats, format!("Création de {rel} : {e}")),
            }
        }

        let (l, r) = (self.cfg.left.clone(), self.cfg.right.clone());
        let results: Vec<_> = files
            .into_par_iter()
            .map(|(side, rel, m)| {
                let (src, dst) = match side {
                    Side::Left => (l.join(&rel), r.join(&rel)),
                    Side::Right => (r.join(&rel), l.join(&rel)),
                };
                let res = copy_file(&src, &dst, &m);
                (side, rel, m, res)
            })
            .collect();
        for (side, rel, m, res) in results {
            match res {
                Ok(()) => {
                    stats.copied += 1;
                    stats.bytes += m.size;
                    let arrow = if side == Side::Left { "→" } else { "←" };
                    self.log(rep, Level::Info, format!("{arrow} {rel}"));
                    self.state.insert(rel, m);
                }
                Err(e) => self.err(rep, stats, format!("Copie de {rel} : {e}")),
            }
        }

        for (side, rel) in trashes {
            match self.trash(side, &rel) {
                Ok(()) => {
                    stats.deleted += 1;
                    self.log(rep, Level::Info, format!("🗑 {rel}"));
                    self.state.remove(&rel);
                }
                Err(e) => self.err(rep, stats, format!("Suppression de {rel} : {e}")),
            }
        }
    }

    fn trash(&self, side: Side, rel: &str) -> io::Result<()> {
        let root = self.root(side);
        let p = root.join(rel);
        if !p.exists() {
            return Ok(());
        }
        // La corbeille Windows n'existe pas sur les partages réseau : dossier .symple-trash à la place.
        if !is_network(root) && trash::delete(&p).is_ok() {
            return Ok(());
        }
        let dst = root.join(TRASH_DIR).join(day_stamp()).join(rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        let dst = unique(dst);
        fs::rename(&p, &dst)
    }

    fn save_state(&self) {
        if let Some(p) = self.state_path.parent() {
            let _ = fs::create_dir_all(p);
        }
        if let Ok(b) = serde_json::to_vec(&self.state) {
            let tmp = self.state_path.with_extension("tmp");
            if fs::write(&tmp, b).is_ok() {
                let _ = fs::rename(tmp, &self.state_path);
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
    conflicts: &mut Vec<(String, Meta, Meta)>,
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
                    _ => conflicts.push((rel.into(), *l, *r)),
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

/// Copie atomique : fichier temporaire, date conservée, puis renommage.
fn copy_file(src: &Path, dst: &Path, m: &Meta) -> io::Result<()> {
    if let Some(p) = dst.parent() {
        fs::create_dir_all(p)?;
    }
    let name = dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dst.with_file_name(format!(".{name}{TMP_SUFFIX}"));
    let res = (|| {
        fs::copy(src, &tmp)?;
        let ft = filetime::FileTime::from_unix_time(m.mtime.div_euclid(1000), (m.mtime.rem_euclid(1000) * 1_000_000) as u32);
        filetime::set_file_mtime(&tmp, ft)?;
        // Le fichier source a-t-il bougé pendant la copie ? On réessaiera plus tard.
        let now = Meta::from_fs(&fs::metadata(src)?);
        if !same(&now, m) {
            return Err(io::Error::other("fichier en cours d'écriture, nouvel essai au prochain passage"));
        }
        if let Ok(md) = fs::metadata(dst) {
            let mut p = md.permissions();
            if p.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                p.set_readonly(false);
                let _ = fs::set_permissions(dst, p);
            }
        }
        fs::rename(&tmp, dst)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// Renomme `rel` en « nom (conflit AAAA-MM-JJ HHhMM).ext ».
fn backup(root: &Path, rel: &str) -> io::Result<String> {
    let p = root.join(rel);
    if !p.exists() {
        return Ok(String::new());
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dst = unique(p.with_file_name(format!("{stem} (conflit {}){ext}", minute_stamp())));
    fs::rename(&p, &dst)?;
    Ok(dst.file_name().unwrap_or_default().to_string_lossy().into_owned())
}

fn unique(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..).map(|i| p.with_file_name(format!("{stem} ({i}){ext}"))).find(|c| !c.exists()).unwrap()
}

fn is_network(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.starts_with(r"\\") || s.starts_with("//")
}

fn civil(secs: i64) -> (i64, i64, i64, i64, i64) {
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
    (y, m, d, rem / 3600, (rem % 3600) / 60)
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn day_stamp() -> String {
    let (y, m, d, _, _) = civil(now_secs());
    format!("{y:04}-{m:02}-{d:02}")
}

fn minute_stamp() -> String {
    let (y, m, d, h, mi) = civil(now_secs());
    format!("{y:04}-{m:02}-{d:02} {h:02}h{mi:02}")
}
