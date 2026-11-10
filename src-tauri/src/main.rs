#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use symple_core::*;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_opener::OpenerExt;

const MAX_LOGS: usize = 300;

// ------------------------------------------------------------------ reporter

#[derive(Clone, Serialize)]
struct LogLine {
    pair_id: String,
    level: Level,
    msg: String,
    time: i64,
}

struct UiReporter {
    app: AppHandle,
    logs: Mutex<VecDeque<LogLine>>,
    statuses: Mutex<HashMap<String, PairStatus>>,
    conflicts: Mutex<Vec<Conflict>>,
}

impl Reporter for UiReporter {
    fn log(&self, pair_id: &str, level: Level, msg: String) {
        let line = LogLine { pair_id: pair_id.into(), level, msg, time: now_ms() };
        let mut logs = self.logs.lock().unwrap();
        if logs.len() >= MAX_LOGS {
            logs.pop_front();
        }
        logs.push_back(line.clone());
        let _ = self.app.emit("log", line);
    }

    fn status(&self, status: &PairStatus) {
        if status.conflicts == 0 {
            self.conflicts.lock().unwrap().retain(|c| c.pair_id != status.pair_id);
        }
        self.statuses.lock().unwrap().insert(status.pair_id.clone(), status.clone());
        let _ = self.app.emit("status", status);
    }

    fn conflict(&self, c: &Conflict) {
        self.conflicts.lock().unwrap().push(c.clone());
        let _ = self.app.emit("conflict", c);
        show_window(&self.app);
    }
}

// --------------------------------------------------------------------- state

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    pairs: Vec<PairConfig>,
    start_minimized: bool,
}

struct App {
    dir: PathBuf,
    config: Mutex<Config>,
    runners: Mutex<HashMap<String, PairRunner>>,
    rep: Arc<UiReporter>,
}

impl App {
    fn save(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap();
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_vec_pretty(&*cfg).map_err(|e| e.to_string())?;
        let tmp = self.dir.join("config.tmp");
        fs::write(&tmp, json).map_err(|e| e.to_string())?;
        fs::rename(tmp, self.dir.join("config.json")).map_err(|e| e.to_string())
    }

    fn start(&self, pair: &PairConfig) {
        let mut runners = self.runners.lock().unwrap();
        runners.remove(&pair.id); // arrête l'ancien
        let rep: Arc<dyn Reporter> = self.rep.clone();
        runners.insert(pair.id.clone(), PairRunner::start(pair.clone(), self.dir.join("state"), rep));
    }
}

// ------------------------------------------------------------------ commands

#[derive(Serialize)]
struct Snapshot {
    pairs: Vec<PairConfig>,
    statuses: HashMap<String, PairStatus>,
    logs: Vec<LogLine>,
    conflicts: Vec<Conflict>,
    autostart: bool,
    start_minimized: bool,
    default_excludes: Vec<String>,
}

#[tauri::command]
fn get_state(app: AppHandle, s: State<App>) -> Snapshot {
    let cfg = s.config.lock().unwrap();
    Snapshot {
        pairs: cfg.pairs.clone(),
        statuses: s.rep.statuses.lock().unwrap().clone(),
        logs: s.rep.logs.lock().unwrap().iter().cloned().collect(),
        conflicts: s.rep.conflicts.lock().unwrap().clone(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        start_minimized: cfg.start_minimized,
        default_excludes: default_excludes(),
    }
}

#[tauri::command]
fn save_pair(s: State<App>, mut pair: PairConfig) -> Result<PairConfig, String> {
    validate(&pair)?;
    if pair.id.is_empty() {
        pair.id = format!("{:x}", now_ms());
    }
    pair.excludes.retain(|e| !e.trim().is_empty());
    {
        let mut cfg = s.config.lock().unwrap();
        match cfg.pairs.iter_mut().find(|p| p.id == pair.id) {
            Some(p) => *p = pair.clone(),
            None => cfg.pairs.push(pair.clone()),
        }
    }
    s.save()?;
    s.start(&pair);
    Ok(pair)
}

#[tauri::command]
fn delete_pair(s: State<App>, id: String) -> Result<(), String> {
    s.runners.lock().unwrap().remove(&id);
    s.config.lock().unwrap().pairs.retain(|p| p.id != id);
    s.rep.statuses.lock().unwrap().remove(&id);
    s.rep.conflicts.lock().unwrap().retain(|c| c.pair_id != id);
    let _ = fs::remove_file(s.dir.join("state").join(format!("{id}.json")));
    s.save()
}

#[tauri::command]
fn set_paused(s: State<App>, id: String, paused: bool) -> Result<(), String> {
    if let Some(p) = s.config.lock().unwrap().pairs.iter_mut().find(|p| p.id == id) {
        p.enabled = !paused;
    }
    if let Some(r) = s.runners.lock().unwrap().get(&id) {
        r.set_paused(paused);
    }
    s.save()
}

#[tauri::command]
fn sync_now(s: State<App>, id: Option<String>) {
    for (rid, r) in s.runners.lock().unwrap().iter() {
        if id.as_deref().is_none_or(|i| i == rid) {
            r.sync_now();
        }
    }
}

#[tauri::command]
fn resolve_conflict(s: State<App>, pair_id: String, rel: String, choice: ConflictChoice) {
    s.rep.conflicts.lock().unwrap().retain(|c| !(c.pair_id == pair_id && c.rel == rel));
    if let Some(r) = s.runners.lock().unwrap().get(&pair_id) {
        r.resolve(rel, choice);
    }
}

#[tauri::command]
fn set_settings(app: AppHandle, s: State<App>, autostart: bool, start_minimized: bool) -> Result<(), String> {
    let al = app.autolaunch();
    let res = if autostart { al.enable() } else { al.disable() };
    res.map_err(|e| e.to_string())?;
    s.config.lock().unwrap().start_minimized = start_minimized;
    s.save()
}

#[tauri::command]
fn open_folder(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

fn validate(p: &PairConfig) -> Result<(), String> {
    let (l, r) = (p.left.as_path(), p.right.as_path());
    if l.as_os_str().is_empty() || r.as_os_str().is_empty() {
        return Err("Choisis les deux dossiers.".into());
    }
    let norm = |x: &Path| fs::canonicalize(x).unwrap_or_else(|_| x.to_path_buf());
    let (nl, nr) = (norm(l), norm(r));
    if nl == nr || nl.starts_with(&nr) || nr.starts_with(&nl) {
        return Err("Les deux dossiers doivent être distincts et non imbriqués.".into());
    }
    Ok(())
}

// ----------------------------------------------------------------------- app

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show_window(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let dir = app.path().app_config_dir()?;
            let config: Config = fs::read(dir.join("config.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            let minimized = config.start_minimized || std::env::args().any(|a| a == "--minimized");
            let rep = Arc::new(UiReporter {
                app: handle.clone(),
                logs: Mutex::default(),
                statuses: Mutex::default(),
                conflicts: Mutex::default(),
            });
            let state = App { dir, config: Mutex::new(config), runners: Mutex::default(), rep };
            for p in state.config.lock().unwrap().pairs.iter() {
                state.start(p);
            }
            app.manage(state);

            // Icône de la barre des tâches
            let open = MenuItem::with_id(app, "open", "Ouvrir Symple", true, None::<&str>)?;
            let sync = MenuItem::with_id(app, "sync", "Tout synchroniser", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &sync, &quit])?;
            TrayIconBuilder::with_id("tray")
                .icon(app.default_window_icon().cloned().ok_or("icône manquante")?)
                .tooltip("Symple")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "open" => show_window(app),
                    "sync" => {
                        for r in app.state::<App>().runners.lock().unwrap().values() {
                            r.sync_now();
                        }
                    }
                    "quit" => {
                        app.state::<App>().runners.lock().unwrap().clear();
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        show_window(tray.app_handle());
                    }
                })
                .build(app)?;

            if !minimized {
                show_window(&handle);
            }
            Ok(())
        })
        .on_window_event(|w, e| {
            // Fermer la fenêtre = réduire dans la barre des tâches.
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            save_pair,
            delete_pair,
            set_paused,
            sync_now,
            resolve_conflict,
            set_settings,
            open_folder
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de Symple");
}
