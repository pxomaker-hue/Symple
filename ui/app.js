"use strict";

const T = window.__TAURI__;
const invoke = T ? T.core.invoke : mockInvoke;
const listen = T ? T.event.listen : () => {};
const $ = (s, el = document) => el.querySelector(s);
const ACCENTS = ["#6366f1", "#ec4899", "#14b8a6", "#f59e0b", "#3b82f6", "#8b5cf6", "#ef4444", "#22c55e"];
const REPO_API = "https://api.github.com/repos/pxomaker-hue/Symple/releases/latest";

let S = { pairs: [], statuses: {}, logs: [], conflicts: [], default_excludes: [] };
let editing = null;
const PROG = {}; // progression par paire : { p, speed, lastBytes, lastTime }

// ---------------------------------------------------------------- outils
function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}
function ago(ms) {
  if (!ms) return t("never");
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 5) return t("justNow");
  if (s < 60) return t("secAgo", s);
  if (s < 3600) return t("minAgo", Math.floor(s / 60));
  return new Date(ms).toLocaleString(t("locale"), { dateStyle: "short", timeStyle: "short" });
}
function size(b) {
  const u = t("units");
  let i = 0;
  while (b >= 1024 && i < u.length - 1) { b /= 1024; i++; }
  return `${i ? b.toFixed(1) : Math.round(b)} ${u[i]}`;
}
function duration(s) {
  if (!isFinite(s) || s <= 0) return "";
  if (s < 60) return `${Math.ceil(s)} s`;
  if (s < 3600) return `${Math.ceil(s / 60)} min`;
  return `${Math.floor(s / 3600)} h ${Math.ceil((s % 3600) / 60)} min`;
}
function pairName(id) {
  const p = S.pairs.find((p) => p.id === id);
  return p ? p.name || t("unnamed") : "";
}
const baseName = (s) => s.split(/[\\/]/).filter(Boolean).pop() || s;

// ---------------------------------------------------------------- rendu
function renderPairs() {
  const root = $("#pairs");
  if (!S.pairs.length) {
    root.innerHTML = `<div class="empty"><div class="big">⇄</div><h2>${t("emptyTitle")}</h2>
      <p>${t("emptyText")}</p>
      <button class="btn primary" id="btn-first-pair">${t("emptyBtn")}</button></div>`;
  } else {
    root.innerHTML = S.pairs.map(cardHtml).join("");
  }
  const st = Object.values(S.statuses);
  const syncing = st.filter((s) => s.state === "syncing").length;
  const errors = st.filter((s) => s.state === "error").length;
  $("#summary").textContent = !S.pairs.length
    ? t("tagline")
    : t("summary", S.pairs.length, syncing ? t("nSyncing", syncing) : errors ? t("nErrors", errors) : t("allUpToDate"));
}

function progressHtml(id) {
  const pr = PROG[id];
  if (!pr || !pr.p.total_files) return "";
  const p = pr.p;
  const pct = p.total_bytes ? (p.done_bytes / p.total_bytes) * 100 : (p.done_files / p.total_files) * 100;
  const eta = pr.speed > 0 ? duration((p.total_bytes - p.done_bytes) / pr.speed) : "";
  return `<div class="progress"><div class="bar"><span style="width:${Math.min(100, pct).toFixed(1)}%"></span></div>
    <div class="progress-text">${esc(t("progress", p.done_files, p.total_files, size(pr.speed || 0), eta))}</div></div>`;
}

function cardHtml(p, i) {
  const st = S.statuses[p.id] || { state: p.enabled ? "idle" : "paused" };
  const accent = ACCENTS[i % ACCENTS.length];
  const paused = !p.enabled;
  const mirror = p.mode === "mirror";
  const conflicts = S.conflicts.filter((c) => c.pair_id === p.id).length;
  return `<article class="card" style="--accent:${accent}" data-id="${p.id}">
    <div class="card-head">
      <h3 title="${esc(p.name)}">${esc(p.name || t("unnamed"))}</h3>
      <span class="pill st-${st.state}">${t("st_" + st.state)}</span>
    </div>
    ${st.error ? `<p class="error-text">${esc(st.error)}</p>` : ""}
    <div class="paths">
      <div class="path" data-open="${esc(p.left)}" title="${esc(p.left)}"><b>A</b>&lrm;${esc(p.left)}&lrm;</div>
      <div class="arrow">${mirror ? "→" : "⇄"}</div>
      <div class="path" data-open="${esc(p.right)}" title="${esc(p.right)}"><b>B</b>&lrm;${esc(p.right)}&lrm;</div>
    </div>
    ${st.state === "syncing" ? progressHtml(p.id) : ""}
    <div class="chips">
      ${st.watching ? `<span class="chip hot">${t("realtime")}</span>` : `<span class="chip">${t("every", p.poll_secs)}</span>`}
      <span class="chip">${mirror ? t("mirror") : t("bidir")}</span>
      ${p.propagate_deletes ? `<span class="chip hot">${t("deletesChip")}</span>` : ""}
      ${p.versions ? `<span class="chip hot">${t("versionsChip", p.versions)}</span>` : ""}
      ${mirror ? "" : `<span class="chip">${t("c_" + p.conflict)}</span>`}
      ${conflicts ? `<span class="chip alert" data-act="conflicts">${t("nConflicts", conflicts)}</span>` : ""}
    </div>
    <div class="card-foot">
      <span class="stats">${t("files", st.files ?? 0)} · ${ago(st.last_sync)}</span>
      <span class="tools">
        <button class="btn ghost" data-act="sync" title="${t("syncNow")}" ${paused ? "disabled" : ""}>⟳</button>
        <button class="btn ghost" data-act="pause" title="${paused ? t("resume") : t("pause")}">${paused ? "▶" : "⏸"}</button>
        <button class="btn ghost" data-act="edit" title="${t("edit")}">✎</button>
        <button class="btn ghost" data-act="delete" title="${t("deletePair")}">✕</button>
      </span>
    </div>
  </article>`;
}

function logHtml(l) {
  const time = new Date(l.time).toLocaleTimeString(t("locale"));
  return `<li class="lv-${l.level}"><span class="dot"></span><div><div class="msg">${esc(l.msg)}</div>
    <div class="meta">${time} · ${esc(pairName(l.pair_id))}</div></div></li>`;
}
function renderLogs() {
  $("#log").innerHTML = S.logs.slice().reverse().map(logHtml).join("") || `<li class="muted">${t("nothingYet")}</li>`;
}

function renderAll() {
  applyI18n();
  renderPairs();
  renderLogs();
}

// --------------------------------------------------------------- toasts
let toastTimer;
function toast(msg) {
  const el = $("#toast");
  el.textContent = msg;
  el.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove("show"), 2600);
}

// ---------------------------------------------------------- éditeur paire
const form = $("#form-pair");
function segValue(name) {
  return $(`.segmented[data-name=${name}] .on`, form)?.dataset.value;
}
function setSeg(name, v) {
  form.querySelectorAll(`.segmented[data-name=${name}] button`).forEach((b) => b.classList.toggle("on", b.dataset.value === v));
  if (name === "mode") {
    $("#dir-arrow").textContent = v === "mirror" ? "↓" : "⇅";
    $("#conflict-field").style.display = v === "mirror" ? "none" : "";
  }
}
form.querySelectorAll(".segmented button").forEach((b) =>
  b.addEventListener("click", () => setSeg(b.parentElement.dataset.name, b.dataset.value)),
);

function openEditor(p) {
  editing = p || null;
  const d = p || {
    name: "", left: "", right: "", mode: "bidirectional", conflict: "newest_with_backup",
    propagate_deletes: false, excludes: S.default_excludes, poll_secs: 30, versions: 0, download_online_only: false,
  };
  $("#pair-title").textContent = p ? t("editPairTitle") : t("newPairTitle");
  form.name.value = d.name;
  form.left.value = d.left;
  form.right.value = d.right;
  form.propagate_deletes.checked = d.propagate_deletes;
  form.download_online_only.checked = !!d.download_online_only;
  form.excludes.value = d.excludes.join("\n");
  form.poll_secs.value = d.poll_secs;
  form.versions.value = d.versions || 0;
  setSeg("mode", d.mode);
  setSeg("conflict", d.conflict);
  $("#pair-error").textContent = "";
  $("#dlg-pair").showModal();
}

form.querySelectorAll("[data-pick]").forEach((b) =>
  b.addEventListener("click", async () => {
    if (!T) return;
    const dir = await T.dialog.open({ directory: true, multiple: false, title: t("pickFolder") });
    if (dir) {
      form[b.dataset.pick].value = dir;
      if (!form.name.value && form.left.value && form.right.value) {
        form.name.value = `${baseName(form.left.value)} ⇄ ${baseName(form.right.value)}`;
      }
    }
  }),
);

function pairFromForm() {
  const pair = {
    ...(editing || {}),
    id: editing?.id || "",
    name: form.name.value.trim(),
    left: form.left.value.trim(),
    right: form.right.value.trim(),
    mode: segValue("mode"),
    conflict: segValue("conflict"),
    propagate_deletes: form.propagate_deletes.checked,
    download_online_only: form.download_online_only.checked,
    excludes: form.excludes.value.split("\n").map((s) => s.trim()).filter(Boolean),
    poll_secs: Math.max(2, parseInt(form.poll_secs.value, 10) || 30),
    versions: Math.max(0, Math.min(100, parseInt(form.versions.value, 10) || 0)),
    enabled: editing ? editing.enabled : true,
  };
  if (!pair.name) pair.name = `${baseName(pair.left)} ${pair.mode === "mirror" ? "→" : "⇄"} ${baseName(pair.right)}`;
  return pair;
}

let pendingPair = null;
form.addEventListener("submit", async (e) => {
  e.preventDefault();
  const pair = pairFromForm();
  const pathsChanged = !editing || editing.left !== pair.left || editing.right !== pair.right;
  if (!pathsChanged) return savePair(pair);
  // Nouvelle paire (ou dossiers changés) : aperçu avant la première synchro.
  const btn = $("#btn-save-pair");
  btn.disabled = true;
  btn.textContent = t("analysing");
  try {
    const pv = await invoke("preview_pair", { pair });
    pendingPair = pair;
    showPreview(pv);
  } catch (err) {
    $("#pair-error").textContent = err;
  } finally {
    btn.disabled = false;
    btn.textContent = t("save");
  }
});

async function savePair(pair) {
  try {
    const saved = await invoke("save_pair", { pair });
    const i = S.pairs.findIndex((p) => p.id === saved.id);
    if (i >= 0) S.pairs[i] = saved;
    else S.pairs.push(saved);
    $("#dlg-pair").open && $("#dlg-pair").close();
    renderPairs();
    toast(t("saved"));
    return true;
  } catch (err) {
    $("#pair-error").textContent = err;
    return false;
  }
}

// ---------------------------------------------------------------- aperçu
const PV_ICON = { to_right: "→", to_left: "←", delete_left: "🗑 A", delete_right: "🗑 B", move_left: "↪ A", move_right: "↪ B", conflict: "⚠" };
function showPreview(pv) {
  const stat = (label, v, cls = "") => `<div class="pv-stat ${cls}"><b>${v}</b><span>${label}</span></div>`;
  const nothing = !pv.to_right && !pv.to_left && !pv.deletes && !pv.moves && !pv.conflicts;
  $("#preview-stats").innerHTML = nothing
    ? `<p>${t("pvNothing")}</p>`
    : stat(t("pvToRight"), pv.to_right) + stat(t("pvToLeft"), pv.to_left) + stat(t("pvBytes"), size(pv.bytes)) +
      (pv.moves ? stat(t("pvMoves"), pv.moves) : "") +
      (pv.deletes ? stat(t("pvDeletes"), pv.deletes, "warn") : "") +
      (pv.conflicts ? stat(t("pvConflicts"), pv.conflicts, "warn") : "");
  const max = Math.max(1, ...(pv.top_dirs || []).map((d) => d.bytes));
  $("#preview-dirs").innerHTML = (pv.top_dirs || []).length > 1
    ? `<h3>${t("pvTopDirs")}</h3>` + pv.top_dirs.map((d) => `<div class="pv-dir${isTrashName(d.dir) ? " trash" : ""}"><span class="pv-dir-name">${esc(d.dir || t("pvRoot"))}</span>
        <span class="pv-dir-bar"><span style="width:${((d.bytes / max) * 100).toFixed(1)}%"></span></span>
        <span class="pv-size">${t("files", d.files)} · ${size(d.bytes)}</span>
        ${d.dir ? `<button class="btn tiny ghost" data-exclude="${esc(d.dir)}">${t("exclude")}</button>` : "<span></span>"}</div>`).join("")
    : "";
  const trash = (pv.top_dirs || []).filter((d) => isTrashName(d.dir)).map((d) => d.dir);
  $("#preview-online").textContent = [
    trash.length ? t("pvTrash", trash.join(", ")) : "",
    pv.online_skipped ? t("pvOnline", pv.online_skipped) : "",
  ].filter(Boolean).join("\n");
  $("#preview-list").innerHTML =
    pv.items.map((it) => `<li class="pv-${it.kind}"><span class="pv-kind">${PV_ICON[it.kind]}</span><span class="pv-rel">${esc(it.rel)}</span>${it.size ? `<span class="pv-size">${size(it.size)}</span>` : ""}</li>`).join("") +
    (pv.truncated ? `<li class="muted">${t("pvTruncated", pv.items.length)}</li>` : "");
  if (!$("#dlg-preview").open) $("#dlg-preview").showModal();
}
// Dossiers qui ressemblent à une corbeille (MEGA, Windows, Linux, macOS…).
const TRASH_RE = /^(rubbish( bin)?|trash|\.trash(es|-\d+)?|\$recycle\.bin|recycle(d| bin)|corbeille)$/i;
const isTrashName = (dir) => !!dir && TRASH_RE.test(dir);

// Exclure un dossier depuis l'aperçu, puis recalculer.
$("#preview-dirs").addEventListener("click", async (e) => {
  const b = e.target.closest("[data-exclude]");
  if (!b || !pendingPair) return;
  const dir = b.dataset.exclude;
  if (!pendingPair.excludes.includes(dir)) pendingPair.excludes = [...pendingPair.excludes, dir];
  form.excludes.value = pendingPair.excludes.join("\n");
  b.disabled = true;
  b.textContent = t("analysing");
  try {
    showPreview(await invoke("preview_pair", { pair: pendingPair }));
  } catch (err) {
    toast(String(err));
  }
});
$("#btn-preview-back").onclick = () => $("#dlg-preview").close();
$("#btn-preview-go").onclick = async () => {
  $("#dlg-preview").close();
  if (pendingPair && (await savePair(pendingPair))) pendingPair = null;
};

// -------------------------------------------------------- actions cartes
$("#pairs").addEventListener("click", async (e) => {
  // Pas d'onclick inline : la CSP de Tauri les bloque.
  if (e.target.closest("#btn-first-pair")) return openEditor();
  const open = e.target.closest("[data-open]");
  if (open) return invoke("open_folder", { path: open.dataset.open }).catch(toast);
  const btn = e.target.closest("[data-act]");
  if (!btn) return;
  const id = btn.closest(".card").dataset.id;
  const p = S.pairs.find((x) => x.id === id);
  switch (btn.dataset.act) {
    case "sync":
      await invoke("sync_now", { id });
      toast(t("syncStarted"));
      break;
    case "pause":
      p.enabled = !p.enabled;
      await invoke("set_paused", { id, paused: !p.enabled });
      renderPairs();
      break;
    case "edit":
      openEditor(p);
      break;
    case "delete":
      if (confirm(t("confirmDelete", p.name))) {
        await invoke("delete_pair", { id });
        S.pairs = S.pairs.filter((x) => x.id !== id);
        delete S.statuses[id];
        renderPairs();
      }
      break;
    case "conflicts":
      showConflict();
      break;
  }
});

// -------------------------------------------------------------- conflits
let current = null;
function metaHtml(m, newer) {
  const d = new Date(m.mtime).toLocaleString(t("locale"), { dateStyle: "medium", timeStyle: "medium" });
  return `${d}<br>${size(m.size)}${newer ? ` · <span class="newer">${t("newer")}</span>` : ""}`;
}
function showConflict() {
  const dlg = $("#dlg-conflict");
  current = S.conflicts[0];
  if (!current) return dlg.open && dlg.close();
  const p = S.pairs.find((x) => x.id === current.pair_id) || {};
  const leftNewer = current.left.mtime >= current.right.mtime;
  $("#conflict-file").textContent = current.rel;
  $("#conflict-left-path").textContent = p.left || "A";
  $("#conflict-right-path").textContent = p.right || "B";
  $("#conflict-left-meta").innerHTML = metaHtml(current.left, leftNewer);
  $("#conflict-right-meta").innerHTML = metaHtml(current.right, !leftNewer);
  $("#conflict-count").textContent = S.conflicts.length > 1 ? t("pendingConflicts", S.conflicts.length) : "";
  if (!dlg.open) dlg.showModal();
}
$("#dlg-conflict").addEventListener("click", async (e) => {
  const b = e.target.closest("[data-choice]");
  if (!b || !current) return;
  if (b.dataset.choice === "later") {
    $("#dlg-conflict").close();
    return;
  }
  await invoke("resolve_conflict", { pairId: current.pair_id, rel: current.rel, choice: b.dataset.choice });
  S.conflicts = S.conflicts.filter((c) => c !== current);
  renderPairs();
  showConflict();
});

// ----------------------------------------------------------- paramètres
$("#btn-settings").onclick = () => {
  $("#set-autostart").checked = S.autostart;
  $("#set-minimized").checked = S.start_minimized;
  $("#set-updates").checked = S.check_updates;
  $("#set-language").value = S.language || "auto";
  $("#version-line").textContent = t("versionLine", S.version || "?");
  $("#dlg-settings").showModal();
};
for (const id of ["#set-autostart", "#set-minimized", "#set-updates", "#set-language"]) {
  $(id).addEventListener("change", async () => {
    S.autostart = $("#set-autostart").checked;
    S.start_minimized = $("#set-minimized").checked;
    S.check_updates = $("#set-updates").checked;
    const langChanged = S.language !== $("#set-language").value;
    S.language = $("#set-language").value;
    try {
      await invoke("set_settings", {
        autostart: S.autostart, startMinimized: S.start_minimized, language: S.language, checkUpdates: S.check_updates,
      });
    } catch (err) {
      toast(String(err));
    }
    if (langChanged) await setLanguage(S.language);
  });
}

async function setLanguage(pref) {
  LANG = resolveLang(pref);
  await invoke("set_language", { lang: LANG }).catch(() => {});
  renderAll();
  $("#version-line").textContent = t("versionLine", S.version || "?");
}

document.querySelectorAll("[data-close]").forEach((b) => b.addEventListener("click", () => b.closest("dialog").close()));
$("#btn-add").onclick = () => openEditor();
$("#btn-sync-all").onclick = async () => {
  await invoke("sync_now", { id: null });
  toast(t("syncAllStarted"));
};
$("#btn-clear-log").onclick = () => {
  S.logs = [];
  renderLogs();
};

// ------------------------------------------------------- mises à jour
function newer(a, b) {
  const pa = a.replace(/^v/, "").split(".").map(Number);
  const pb = b.replace(/^v/, "").split(".").map(Number);
  for (let i = 0; i < 3; i++) if ((pa[i] || 0) !== (pb[i] || 0)) return (pa[i] || 0) > (pb[i] || 0);
  return false;
}
async function checkUpdates() {
  if (!T || !S.check_updates || !S.version) return;
  try {
    const r = await fetch(REPO_API, { headers: { Accept: "application/vnd.github+json" } });
    if (!r.ok) return;
    const tag = (await r.json()).tag_name || "";
    if (tag && newer(tag, S.version)) {
      $("#update-text").textContent = t("updateAvailable", tag);
      $("#update-banner").hidden = false;
    }
  } catch (_) {
    /* hors ligne : on réessaiera au prochain démarrage */
  }
}
$("#btn-update").onclick = () => invoke("open_releases").catch(toast);
$("#btn-update-close").onclick = () => ($("#update-banner").hidden = true);

// -------------------------------------------------------------- événements
let renderQueued = false;
function scheduleRender() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => {
    renderQueued = false;
    renderPairs();
  });
}
listen("status", (e) => {
  const s = e.payload;
  S.statuses[s.pair_id] = s;
  if (s.state !== "syncing") delete PROG[s.pair_id];
  if (!s.conflicts) S.conflicts = S.conflicts.filter((c) => c.pair_id !== s.pair_id);
  scheduleRender();
});
listen("progress", (e) => {
  const p = e.payload;
  const now = performance.now();
  const pr = PROG[p.pair_id] || { speed: 0, lastBytes: 0, lastTime: now };
  const dt = (now - pr.lastTime) / 1000;
  if (dt > 0.1) {
    const inst = Math.max(0, p.done_bytes - pr.lastBytes) / dt;
    pr.speed = pr.speed ? pr.speed * 0.7 + inst * 0.3 : inst;
    pr.lastBytes = p.done_bytes;
    pr.lastTime = now;
  }
  pr.p = p;
  PROG[p.pair_id] = pr;
  scheduleRender();
});
listen("log", (e) => {
  S.logs.push(e.payload);
  if (S.logs.length > 300) S.logs.shift();
  const ul = $("#log");
  if (S.logs.length === 1) ul.innerHTML = "";
  ul.insertAdjacentHTML("afterbegin", logHtml(e.payload));
  if (ul.children.length > 300) ul.lastElementChild.remove();
});
listen("conflict", (e) => {
  S.conflicts.push(e.payload);
  scheduleRender();
  showConflict();
});
setInterval(scheduleRender, 15000); // rafraîchit « il y a … »

(async () => {
  S = await invoke("get_state");
  await setLanguage(S.language || "auto");
  if (S.conflicts.length) showConflict();
  checkUpdates();
})();

// ---------------------------------------- aperçu hors Tauri (navigateur)
async function mockInvoke(cmd) {
  if (cmd === "preview_pair") {
    return {
      to_right: 1240, to_left: 12, deletes: 0, moves: 3, conflicts: 2, bytes: 5.2e9, truncated: true, online_skipped: 37,
      top_dirs: [{ dir: "Rubbish", files: 1548, bytes: 6.2e9 }, { dir: "Photos", files: 900, bytes: 4.1e9 }, { dir: "Docs", files: 300, bytes: 0.9e9 }, { dir: "", files: 52, bytes: 0.2e9 }],
      items: [
        { kind: "to_right", rel: "vacances/IMG_2041.jpg", size: 4200000 },
        { kind: "to_left", rel: "docs/facture.pdf", size: 120000 },
        { kind: "move_right", rel: "a/film.mkv → b/film.mkv", size: 0 },
        { kind: "conflict", rel: "notes.txt", size: 0 },
      ],
    };
  }
  if (cmd !== "get_state") return null;
  const now = Date.now();
  setTimeout(() => {
    PROG["1"] = { p: { pair_id: "1", done_files: 412, total_files: 1240, done_bytes: 1.9e9, total_bytes: 5.2e9 }, speed: 48e6 };
    scheduleRender();
  }, 50);
  return {
    pairs: [
      { id: "1", name: "Photos ⇄ NAS", left: "C:\\Users\\Pxo\\Pictures", right: "\\\\NAS\\photos", mode: "bidirectional", conflict: "ask", propagate_deletes: true, excludes: [], poll_secs: 30, versions: 3, enabled: true },
      { id: "2", name: "Projets → Sauvegarde", left: "D:\\Projets", right: "E:\\Backup\\Projets", mode: "mirror", conflict: "newest", propagate_deletes: false, excludes: [], poll_secs: 30, versions: 0, enabled: true },
      { id: "3", name: "Documents ⇄ Portable", left: "C:\\Users\\Pxo\\Documents", right: "\\\\PORTABLE\\docs", mode: "bidirectional", conflict: "newest_with_backup", propagate_deletes: false, excludes: [], poll_secs: 10, versions: 0, enabled: false },
    ],
    statuses: {
      1: { pair_id: "1", state: "syncing", files: 12840, last_sync: now - 4000, watching: true, conflicts: 1 },
      2: { pair_id: "2", state: "idle", files: 3211, last_sync: now - 120000, watching: true, conflicts: 0 },
      3: { pair_id: "3", state: "paused", files: 812, last_sync: now - 86400000, watching: false, conflicts: 0 },
    },
    logs: [
      { pair_id: "2", level: "info", msg: "→ src/main.rs", time: now - 60000 },
      { pair_id: "1", level: "warn", msg: "Conflit sur vacances/IMG_2041.jpg", time: now - 20000 },
      { pair_id: "1", level: "info", msg: "Synchro : 14 copié(s), 1 déplacé(s), 2 supprimé(s), 48.3 Mo", time: now - 5000 },
    ],
    conflicts: [],
    autostart: true,
    start_minimized: false,
    language: new URLSearchParams(location.search).get("lang") || "auto",
    check_updates: true,
    version: "0.2.0",
    default_excludes: ["*.tmp", "Thumbs.db"],
  };
}
