use std::fs;
use std::path::Path;
use std::sync::Mutex;
use symple_core::*;

#[derive(Default)]
struct Rec(Mutex<Vec<Conflict>>);
impl Reporter for Rec {
    fn log(&self, _: &str, _: Level, _: String) {}
    fn status(&self, _: &PairStatus) {}
    fn conflict(&self, c: &Conflict) {
        self.0.lock().unwrap().push(c.clone());
    }
}

fn write(p: &Path, content: &str, secs: i64) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
    filetime::set_file_mtime(p, filetime::FileTime::from_unix_time(secs, 0)).unwrap();
}

fn setup(mode: SyncMode, del: bool, conflict: ConflictPolicy) -> (tempfile::TempDir, Engine) {
    let t = tempfile::tempdir().unwrap();
    for d in ["L", "R", "S"] {
        fs::create_dir(t.path().join(d)).unwrap();
    }
    let cfg = PairConfig {
        id: "p".into(),
        left: t.path().join("L"),
        right: t.path().join("R"),
        mode,
        propagate_deletes: del,
        conflict,
        ..Default::default()
    };
    let e = Engine::new(cfg, &t.path().join("S"));
    (t, e)
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap()
}

#[test]
fn bidirectional_basics() {
    let (t, mut e) = setup(SyncMode::Bidirectional, true, ConflictPolicy::Ask);
    let (l, r) = (t.path().join("L"), t.path().join("R"));
    let rep = Rec::default();
    write(&l.join("a/1.txt"), "un", 1000);
    write(&r.join("2.txt"), "deux", 1000);
    write(&l.join("Thumbs.db"), "x", 1000);
    e.sync(None, &rep).unwrap();
    assert_eq!(read(&r.join("a/1.txt")), "un");
    assert_eq!(read(&l.join("2.txt")), "deux");
    assert!(!r.join("Thumbs.db").exists());

    // Modification à droite → copiée à gauche
    write(&r.join("a/1.txt"), "un bis", 2000);
    e.sync(None, &rep).unwrap();
    assert_eq!(read(&l.join("a/1.txt")), "un bis");

    // Suppression propagée (fichier puis dossier vide)
    fs::remove_dir_all(r.join("a")).unwrap();
    e.sync(None, &rep).unwrap();
    assert!(!l.join("a").exists());

    // Partial sync sur un chemin précis
    write(&l.join("3.txt"), "trois", 3000);
    e.sync(Some(&["3.txt".to_string()]), &rep).unwrap();
    assert_eq!(read(&r.join("3.txt")), "trois");
    assert!(rep.0.lock().unwrap().is_empty());
}

#[test]
fn deletion_not_propagated_restores() {
    let (t, mut e) = setup(SyncMode::Bidirectional, false, ConflictPolicy::Newest);
    let (l, r) = (t.path().join("L"), t.path().join("R"));
    let rep = Rec::default();
    write(&l.join("f.txt"), "x", 1000);
    e.sync(None, &rep).unwrap();
    fs::remove_file(r.join("f.txt")).unwrap();
    e.sync(None, &rep).unwrap();
    assert!(r.join("f.txt").exists());
}

#[test]
fn conflict_policies() {
    // Plus récent + sauvegarde
    let (t, mut e) = setup(SyncMode::Bidirectional, false, ConflictPolicy::NewestWithBackup);
    let (l, r) = (t.path().join("L"), t.path().join("R"));
    let rep = Rec::default();
    write(&l.join("f.txt"), "base", 1000);
    e.sync(None, &rep).unwrap();
    write(&l.join("f.txt"), "gauche", 5000);
    write(&r.join("f.txt"), "droite!", 6000);
    e.sync(None, &rep).unwrap();
    assert_eq!(read(&l.join("f.txt")), "droite!");
    let backups: Vec<_> = fs::read_dir(&l).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("conflit")).collect();
    assert_eq!(backups.len(), 1);

    // Demander
    let (t, mut e) = setup(SyncMode::Bidirectional, false, ConflictPolicy::Ask);
    let (l, r) = (t.path().join("L"), t.path().join("R"));
    write(&l.join("f.txt"), "gauche", 5000);
    write(&r.join("f.txt"), "droite!", 6000);
    e.sync(None, &rep).unwrap();
    e.sync(None, &rep).unwrap();
    assert_eq!(rep.0.lock().unwrap().len(), 1, "un seul pop-up par conflit");
    assert_eq!(read(&l.join("f.txt")), "gauche");
    e.resolve("f.txt", ConflictChoice::KeepLeft, &rep).unwrap();
    assert_eq!(read(&r.join("f.txt")), "gauche");
    e.sync(None, &rep).unwrap();
    assert!(e.pending_conflicts().is_empty());
}

#[test]
fn mirror() {
    let (t, mut e) = setup(SyncMode::Mirror, true, ConflictPolicy::Newest);
    let (l, r) = (t.path().join("L"), t.path().join("R"));
    let rep = Rec::default();
    write(&l.join("a.txt"), "a", 1000);
    write(&r.join("extra.txt"), "z", 1000);
    write(&r.join("a.txt"), "old", 9000);
    e.sync(None, &rep).unwrap();
    assert_eq!(read(&r.join("a.txt")), "a");
    assert!(!r.join("extra.txt").exists());
}

#[test]
fn runner_realtime() {
    let t = tempfile::tempdir().unwrap();
    let (l, r) = (t.path().join("L"), t.path().join("R"));
    fs::create_dir_all(&l).unwrap();
    fs::create_dir_all(&r).unwrap();
    let cfg = PairConfig { id: "rt".into(), left: l.clone(), right: r.clone(), poll_secs: 3600, ..Default::default() };
    let runner = PairRunner::start(cfg, t.path().join("S"), std::sync::Arc::new(Rec::default()));
    std::thread::sleep(std::time::Duration::from_millis(300));
    fs::write(l.join("live.txt"), "temps réel").unwrap();
    let ok = (0..40).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        r.join("live.txt").exists()
    });
    drop(runner);
    assert!(ok, "le watcher doit copier sans attendre le polling");
}
