"use strict";

const T = window.__TAURI__;
const invoke = T ? T.core.invoke : mockInvoke;
const listen = T ? T.event.listen : () => {};
const $ = (s, el = document) => el.querySelector(s);
const ACCENTS = ["#6366f1", "#ec4899", "#14b8a6", "#f59e0b", "#3b82f6", "#8b5cf6", "#ef4444", "#22c55e"];
const STATE_LABEL = { idle: "À jour", syncing: "Synchro…", paused: "En pause", error: "Erreur" };
const CONFLICT_LABEL = { ask: "Conflit : demander", newest_with_backup: "Plus récent + copie", newest: "Plus récent" };

let S = { pairs: [], statuses: {}, logs: [], conflicts: [], default_excludes: [] };
let editing = null;

// ---------------------------------------------------------------- rendu
function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}
function ago(ms) {
  if (!ms) return "jamais";
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 5) return "à l'instant";
  if (s < 60) return `il y a ${s} s`;
  if (s < 3600) return `il y a ${Math.floor(s / 60)} min`;
  return new Date(ms).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
}
function size(b) {
  const u = ["o", "Ko", "Mo", "Go", "To"];
  let i = 0;
  while (b >= 1024 && i < u.length - 1) { b /= 1024; i++; }
  return `${i ? b.toFixed(1) : b} ${u[i]}`;
}
function pairName(id) {
  const p = S.pairs.find((p) => p.id === id);
  return p ? p.name || "Sans nom" : "";
}

function renderPairs() {
  const root = $("#pairs");
  if (!S.pairs.length) {
    root.innerHTML = `<div class="empty"><div class="big">⇄</div><h2>Aucune paire de dossiers</h2>
      <p>Ajoute deux dossiers (sur ce PC ou sur ton NAS) pour les garder synchronisés en temps réel.</p>
      <button class="btn primary" id="btn-first-pair">＋ Créer ma première paire</button></div>`;
  } else {
    root.innerHTML = S.pairs.map(cardHtml).join("");
  }
  const syncing = Object.values(S.statuses).filter((s) => s.state === "syncing").length;
  const errors = Object.values(S.statuses).filter((s) => s.state === "error").length;
  $("#summary").textContent = !S.pairs.length
    ? "Synchronisation en temps réel"
    : `${S.pairs.length} paire(s) · ${syncing ? syncing + " en cours" : errors ? errors + " en erreur" : "tout est à jour"}`;
}

function cardHtml(p, i) {
  const st = S.statuses[p.id] || { state: p.enabled ? "idle" : "paused" };
  const accent = ACCENTS[i % ACCENTS.length];
  const paused = !p.enabled;
  const mirror = p.mode === "mirror";
  const conflicts = S.conflicts.filter((c) => c.pair_id === p.id).length;
  return `<article class="card" style="--accent:${accent}" data-id="${p.id}">
    <div class="card-head">
      <h3 title="${esc(p.name)}">${esc(p.name || "Sans nom")}</h3>
      <span class="pill st-${st.state}">${STATE_LABEL[st.state]}</span>
    </div>
    ${st.error ? `<p class="error-text">${esc(st.error)}</p>` : ""}
    <div class="paths">
      <div class="path" data-open="${esc(p.left)}" title="${esc(p.left)}"><b>A</b>&lrm;${esc(p.left)}&lrm;</div>
      <div class="arrow">${mirror ? "→" : "⇄"}</div>
      <div class="path" data-open="${esc(p.right)}" title="${esc(p.right)}"><b>B</b>&lrm;${esc(p.right)}&lrm;</div>
    </div>
    <div class="chips">
      ${st.watching ? '<span class="chip hot">⚡ Temps réel</span>' : `<span class="chip">⏱ Toutes les ${p.poll_secs} s</span>`}
      <span class="chip">${mirror ? "Miroir" : "Bidirectionnelle"}</span>
      ${p.propagate_deletes ? '<span class="chip hot">🗑 Suppressions</span>' : ""}
      ${mirror ? "" : `<span class="chip">${CONFLICT_LABEL[p.conflict]}</span>`}
      ${conflicts ? `<span class="chip alert" data-act="conflicts">⚠ ${conflicts} conflit(s)</span>` : ""}
    </div>
    <div class="card-foot">
      <span class="stats">${st.files ?? 0} fichiers · ${ago(st.last_sync)}</span>
      <span class="tools">
        <button class="btn ghost" data-act="sync" title="Synchroniser maintenant" ${paused ? "disabled" : ""}>⟳</button>
        <button class="btn ghost" data-act="pause" title="${paused ? "Reprendre" : "Mettre en pause"}">${paused ? "▶" : "⏸"}</button>
        <button class="btn ghost" data-act="edit" title="Modifier">✎</button>
        <button class="btn ghost" data-act="delete" title="Supprimer la paire">✕</button>
      </span>
    </div>
  </article>`;
}

function logHtml(l) {
  const t = new Date(l.time).toLocaleTimeString("fr-FR");
  return `<li class="lv-${l.level}"><span class="dot"></span><div><div class="msg">${esc(l.msg)}</div>
    <div class="meta">${t} · ${esc(pairName(l.pair_id))}</div></div></li>`;
}
function renderLogs() {
  $("#log").innerHTML = S.logs.slice().reverse().map(logHtml).join("") || '<li class="muted">Rien pour le moment.</li>';
}

// --------------------------------------------------------------- toasts
let toastTimer;
function toast(msg) {
  const t = $("#toast");
  t.textContent = msg;
  t.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove("show"), 2600);
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

window.openEditor = function (p) {
  editing = p || null;
  const d = p || { name: "", left: "", right: "", mode: "bidirectional", conflict: "newest_with_backup", propagate_deletes: false, excludes: S.default_excludes, poll_secs: 30 };
  $("#pair-title").textContent = p ? "Modifier la paire" : "Nouvelle paire";
  form.name.value = d.name;
  form.left.value = d.left;
  form.right.value = d.right;
  form.propagate_deletes.checked = d.propagate_deletes;
  form.excludes.value = d.excludes.join("\n");
  form.poll_secs.value = d.poll_secs;
  setSeg("mode", d.mode);
  setSeg("conflict", d.conflict);
  $("#pair-error").textContent = "";
  $("#dlg-pair").showModal();
};

form.querySelectorAll("[data-pick]").forEach((b) =>
  b.addEventListener("click", async () => {
    if (!T) return;
    const dir = await T.dialog.open({ directory: true, multiple: false, title: "Choisir un dossier" });
    if (dir) {
      form[b.dataset.pick].value = dir;
      if (!form.name.value) {
        const n = (s) => s.split(/[\\/]/).filter(Boolean).pop() || s;
        if (form.left.value && form.right.value) form.name.value = `${n(form.left.value)} ⇄ ${n(form.right.value)}`;
      }
    }
  }),
);

form.addEventListener("submit", async (e) => {
  e.preventDefault();
  const pair = {
    ...(editing || {}),
    id: editing?.id || "",
    name: form.name.value.trim(),
    left: form.left.value.trim(),
    right: form.right.value.trim(),
    mode: segValue("mode"),
    conflict: segValue("conflict"),
    propagate_deletes: form.propagate_deletes.checked,
    excludes: form.excludes.value.split("\n").map((s) => s.trim()).filter(Boolean),
    poll_secs: Math.max(2, parseInt(form.poll_secs.value, 10) || 30),
    enabled: editing ? editing.enabled : true,
  };
  if (!pair.name) {
    const n = (s) => s.split(/[\\/]/).filter(Boolean).pop() || s;
    pair.name = `${n(pair.left)} ${pair.mode === "mirror" ? "→" : "⇄"} ${n(pair.right)}`;
  }
  try {
    const saved = await invoke("save_pair", { pair });
    const i = S.pairs.findIndex((p) => p.id === saved.id);
    if (i >= 0) S.pairs[i] = saved;
    else S.pairs.push(saved);
    $("#dlg-pair").close();
    renderPairs();
    toast("Paire enregistrée ✓");
  } catch (err) {
    $("#pair-error").textContent = err;
  }
});

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
      toast("Synchronisation lancée");
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
      if (confirm(`Supprimer la paire « ${p.name} » ?\nLes fichiers ne sont pas supprimés.`)) {
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
  const d = new Date(m.mtime).toLocaleString("fr-FR", { dateStyle: "medium", timeStyle: "medium" });
  return `${d}<br>${size(m.size)}${newer ? ' · <span class="newer">plus récent</span>' : ""}`;
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
  $("#conflict-count").textContent = S.conflicts.length > 1 ? `${S.conflicts.length} conflits en attente` : "";
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
  $("#dlg-settings").showModal();
};
for (const id of ["#set-autostart", "#set-minimized"]) {
  $(id).addEventListener("change", async () => {
    S.autostart = $("#set-autostart").checked;
    S.start_minimized = $("#set-minimized").checked;
    try {
      await invoke("set_settings", { autostart: S.autostart, startMinimized: S.start_minimized });
    } catch (err) {
      toast(String(err));
    }
  });
}

document.querySelectorAll("[data-close]").forEach((b) => b.addEventListener("click", () => b.closest("dialog").close()));
$("#btn-add").onclick = () => openEditor();
$("#btn-sync-all").onclick = async () => {
  await invoke("sync_now", { id: null });
  toast("Synchronisation de toutes les paires");
};
$("#btn-clear-log").onclick = () => {
  S.logs = [];
  renderLogs();
};

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
  S.statuses[e.payload.pair_id] = e.payload;
  if (!e.payload.conflicts) S.conflicts = S.conflicts.filter((c) => c.pair_id !== e.payload.pair_id);
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
  renderPairs();
  renderLogs();
  if (S.conflicts.length) showConflict();
})();

// ---------------------------------------- aperçu hors Tauri (navigateur)
async function mockInvoke(cmd) {
  if (cmd !== "get_state") return null;
  const now = Date.now();
  return {
    pairs: [
      { id: "1", name: "Photos ⇄ NAS", left: "C:\\Users\\Pxo\\Pictures", right: "\\\\NAS\\photos", mode: "bidirectional", conflict: "ask", propagate_deletes: true, excludes: [], poll_secs: 30, enabled: true },
      { id: "2", name: "Projets → Sauvegarde", left: "D:\\Projets", right: "E:\\Backup\\Projets", mode: "mirror", conflict: "newest", propagate_deletes: false, excludes: [], poll_secs: 30, enabled: true },
      { id: "3", name: "Documents ⇄ Portable", left: "C:\\Users\\Pxo\\Documents", right: "\\\\PORTABLE\\docs", mode: "bidirectional", conflict: "newest_with_backup", propagate_deletes: false, excludes: [], poll_secs: 10, enabled: false },
    ],
    statuses: {
      1: { pair_id: "1", state: "syncing", files: 12840, last_sync: now - 4000, watching: true, conflicts: 1 },
      2: { pair_id: "2", state: "idle", files: 3211, last_sync: now - 120000, watching: true, conflicts: 0 },
      3: { pair_id: "3", state: "paused", files: 812, last_sync: now - 86400000, watching: false, conflicts: 0 },
    },
    logs: [
      { pair_id: "2", level: "info", msg: "→ src/main.rs", time: now - 60000 },
      { pair_id: "1", level: "warn", msg: "Conflit sur vacances/IMG_2041.jpg", time: now - 20000 },
      { pair_id: "1", level: "info", msg: "Synchro : 14 copié(s), 2 supprimé(s), 48.3 Mo", time: now - 5000 },
    ],
    conflicts: [],
    autostart: true,
    start_minimized: false,
    default_excludes: ["*.tmp", "Thumbs.db"],
  };
}
