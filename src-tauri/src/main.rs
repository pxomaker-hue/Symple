#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use symple_core::*;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent, Wry};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

const MAX_LOGS: usize = 300;
const RELEASES_URL: &str = "https://github.com/pxomaker-hue/Symple/releases";

// ------------------------------------------------------------------ reporter

#[derive(Clone, Serialize)]
struct LogLine {
    pair_id: String,
    level: Level,
    msg: String,
    time: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrayState {
    Idle,
    Sync,
    Conflict,
    Error,
}

struct UiReporter {
    app: AppHandle,
    logs: Mutex<VecDeque<LogLine>>,
    statuses: Mutex<HashMap<String, PairStatus>>,
    conflicts: Mutex<Vec<Conflict>>,
    names: Mutex<HashMap<String, String>>,
    tray: Mutex<Option<TrayState>>,
}

impl UiReporter {
    fn name(&self, id: &str) -> String {
        self.names.lock().unwrap().get(id).cloned().unwrap_or_default()
    }

    /// Notification Windows, seulement si la fenêtre n'est pas au premier plan.
    fn notify(&self, title: String, body: String) {
        let visible = self
            .app
            .get_webview_window("main")
            .map(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
            .unwrap_or(false);
        if !visible {
            let _ = self.app.notification().builder().title(title).body(body).show();
        }
    }

    fn update_tray(&self) {
        let state = {
            let st = self.statuses.lock().unwrap();
            if st.values().any(|s| s.state == State_::Error) {
                TrayState::Error
            } else if !self.conflicts.lock().unwrap().is_empty() {
                TrayState::Conflict
            } else if st.values().any(|s| s.state == State_::Syncing) {
                TrayState::Sync
            } else {
                TrayState::Idle
            }
        };
        let mut cur = self.tray.lock().unwrap();
        if *cur == Some(state) {
            return;
        }
        *cur = Some(state);
        let Some(tray) = self.app.tray_by_id("tray") else { return };
        let (bytes, tip): (&[u8], String) = match state {
            TrayState::Idle => (include_bytes!("../icons/tray-idle.png"), tr!("Symple : tout est à jour", "Symple: up to date")),
            TrayState::Sync => (include_bytes!("../icons/tray-sync.png"), tr!("Symple : synchronisation…", "Symple: syncing…")),
            TrayState::Conflict => (include_bytes!("../icons/tray-conflict.png"), tr!("Symple : conflit à résoudre", "Symple: conflict to resolve")),
            TrayState::Error => (include_bytes!("../icons/tray-error.png"), tr!("Symple : erreur", "Symple: error")),
        };
        if let Ok(img) = Image::from_bytes(bytes) {
            let _ = tray.set_icon(Some(img));
        }
        let _ = tray.set_tooltip(Some(tip));
    }
}

use symple_core::State as State_;

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
        let prev = self.statuses.lock().unwrap().insert(status.pair_id.clone(), status.clone());
        let _ = self.app.emit("status", status);
        // Notification à l'apparition d'une erreur (pas à chaque passage).
        if status.state == State_::Error && prev.map(|p| p.error) != Some(status.error.clone()) {
            self.notify(
                tr!("Symple : erreur sur « {} »", "Symple: error on “{}”", self.name(&status.pair_id)),
                status.error.clone().unwrap_or_default(),
            );
        }
        self.update_tray();
    }

    fn conflict(&self, c: &Conflict) {
        self.conflicts.lock().unwrap().push(c.clone());
        let _ = self.app.emit("conflict", c);
        self.notify(
            tr!("Symple : conflit sur « {} »", "Symple: conflict on “{}”", self.name(&c.pair_id)),
            tr!("{} a été modifié des deux côtés.", "{} was changed on both sides.", c.rel),
        );
        show_window(&self.app);
        self.update_tray();
    }

    fn progress(&self, p: &Progress) {
        let _ = self.app.emit("progress", p);
    }
}

// --------------------------------------------------------------------- state

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Config {
    pairs: Vec<PairConfig>,
    start_minimized: bool,
    /// "auto", "fr" ou "en"
    language: String,
    check_updates: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { pairs: Vec::new(), start_minimized: false, language: "auto".into(), check_updates: true }
    }
}

struct TrayMenu {
    open: MenuItem<Wry>,
    sync: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

struct App {
    dir: PathBuf,
    config: Mutex<Config>,
    runners: Mutex<HashMap<String, PairRunner>>,
    rep: Arc<UiReporter>,
    menu: Mutex<Option<TrayMenu>>,
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
        self.rep.names.lock().unwrap().insert(pair.id.clone(), pair.name.clone());
        let mut runners = self.runners.lock().unwrap();
        runners.remove(&pair.id); // arrête l'ancien
        let rep: Arc<dyn Reporter> = self.rep.clone();
        runners.insert(pair.id.clone(), PairRunner::start(pair.clone(), self.dir.join("state"), rep));
    }

    fn relabel_menu(&self) {
        if let Some(m) = self.menu.lock().unwrap().as_ref() {
            let _ = m.open.set_text(tr!("Ouvrir Symple", "Open Symple"));
            let _ = m.sync.set_text(tr!("Tout synchroniser", "Sync all"));
            let _ = m.quit.set_text(tr!("Quitter", "Quit"));
        }
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
    language: String,
    check_updates: bool,
    version: String,
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
        language: cfg.language.clone(),
        check_updates: cfg.check_updates,
        version: app.package_info().version.to_string(),
        default_excludes: default_excludes(),
    }
}

/// Aperçu de la première synchro d'une paire (rien n'est modifié).
#[tauri::command]
async fn preview_pair(pair: PairConfig) -> Result<Preview, String> {
    validate(&pair)?;
    tauri::async_runtime::spawn_blocking(move || {
        let tmp = std::env::temp_dir().join("symple-preview-none");
        let mut p = pair;
        p.id = "__preview__".into();
        Engine::new(p, &tmp).preview()
    })
    .await
    .map_err(|e| e.to_string())?
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
    s.rep.update_tray();
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
    s.rep.update_tray();
}

#[tauri::command]
fn set_settings(
    app: AppHandle,
    s: State<App>,
    autostart: bool,
    start_minimized: bool,
    language: String,
    check_updates: bool,
) -> Result<(), String> {
    let al = app.autolaunch();
    let res = if autostart { al.enable() } else { al.disable() };
    res.map_err(|e| e.to_string())?;
    {
        let mut cfg = s.config.lock().unwrap();
        cfg.start_minimized = start_minimized;
        cfg.language = language;
        cfg.check_updates = check_updates;
    }
    s.save()
}

/// Langue effective choisie par l'interface ("fr" ou "en").
#[tauri::command]
fn set_language(s: State<App>, lang: String) {
    i18n::set_english(lang == "en");
    s.relabel_menu();
    *s.rep.tray.lock().unwrap() = None; // force la mise à jour de l'info-bulle
    s.rep.update_tray();
}

#[tauri::command]
fn open_folder(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_releases(app: AppHandle) -> Result<(), String> {
    app.opener().open_url(RELEASES_URL, None::<&str>).map_err(|e| e.to_string())
}

fn validate(p: &PairConfig) -> Result<(), String> {
    let (l, r) = (p.left.as_path(), p.right.as_path());
    if l.as_os_str().is_empty() || r.as_os_str().is_empty() {
        return Err(tr!("Choisis les deux dossiers.", "Pick both folders."));
    }
    let norm = |x: &Path| fs::canonicalize(x).unwrap_or_else(|_| x.to_path_buf());
    let (nl, nr) = (norm(l), norm(r));
    if nl == nr || nl.starts_with(&nr) || nr.starts_with(&nl) {
        return Err(tr!(
            "Les deux dossiers doivent être distincts et non imbriqués.",
            "The two folders must be different and not inside each other."
        ));
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
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let dir = app.path().app_config_dir()?;
            let config: Config = fs::read(dir.join("config.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            if config.language == "en" {
                i18n::set_english(true);
            }
            let minimized = config.start_minimized || std::env::args().any(|a| a == "--minimized");
            let rep = Arc::new(UiReporter {
                app: handle.clone(),
                logs: Mutex::default(),
                statuses: Mutex::default(),
                conflicts: Mutex::default(),
                names: Mutex::default(),
                tray: Mutex::default(),
            });
            let state = App { dir, config: Mutex::new(config), runners: Mutex::default(), rep, menu: Mutex::default() };

            // Icône de la barre des tâches
            let open = MenuItem::with_id(app, "open", tr!("Ouvrir Symple", "Open Symple"), true, None::<&str>)?;
            let sync = MenuItem::with_id(app, "sync", tr!("Tout synchroniser", "Sync all"), true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", tr!("Quitter", "Quit"), true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &sync, &quit])?;
            TrayIconBuilder::with_id("tray")
                .icon(Image::from_bytes(include_bytes!("../icons/tray-idle.png"))?)
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
            *state.menu.lock().unwrap() = Some(TrayMenu { open, sync, quit });

            for p in state.config.lock().unwrap().pairs.iter() {
                state.start(p);
            }
            app.manage(state);

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
            preview_pair,
            save_pair,
            delete_pair,
            set_paused,
            sync_now,
            resolve_conflict,
            set_settings,
            set_language,
            open_folder,
            open_releases
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de Symple");
}
