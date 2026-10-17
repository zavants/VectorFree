// Vector Free UI: a page-based wizard (Load Image → Wizard Mode → questions →
// Review → Edit/Remove Background → Export), with the image viewer on the left.
"use strict";

const TAURI = window.__TAURI__;
const invoke = TAURI.core.invoke;
const $ = (id) => document.getElementById(id);
const SVGNS = "http://www.w3.org/2000/svg";
const VERSION = "0.3.0";

/** Tiny element builder: h("div", {class: "x", onclick}, child, "text"). */
function h(tag, attrs = {}, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === undefined || v === null || v === false) continue;
    if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (k === "class") el.className = v;
    else if (k === "html") el.innerHTML = v;
    else if (v === true) el.setAttribute(k, "");
    else el.setAttribute(k, v);
  }
  for (const kid of kids.flat()) {
    if (kid === null || kid === undefined || kid === false) continue;
    el.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
  }
  return el;
}

// ---------------------------------------------------------------- storage
const store = {
  get(key, fallback) {
    try {
      const v = localStorage.getItem("vf-" + key);
      return v === null ? fallback : JSON.parse(v);
    } catch {
      return fallback;
    }
  },
  set(key, value) {
    try { localStorage.setItem("vf-" + key, JSON.stringify(value)); } catch { /* storage unavailable */ }
  },
};

// ---------------------------------------------------------------- icons (drawn for Vector Free)
const svg = (body, vb = "0 0 104 88") => `<svg viewBox="${vb}" xmlns="${SVGNS}">${body}</svg>`;
const STAIR = (blend) => {
  // A diagonal edge drawn as pixels; `blend` adds anti-aliased edge pixels.
  let s = "";
  for (let y = 0; y < 8; y++) {
    const edge = 8 - y;
    for (let x = 0; x < 9; x++) {
      if (x >= edge) s += `<rect x="${14 + x * 8}" y="${14 + y * 8}" width="8" height="8" fill="#1a4fa0"/>`;
      else if (blend && x === edge - 1) s += `<rect x="${14 + x * 8}" y="${14 + y * 8}" width="8" height="8" fill="#8fa9d6"/>`;
    }
  }
  return s;
};
const ICON = {
  folder: svg(`<path d="M14 24h26l7 7h43v41H14z" fill="#f1c96a" stroke="#b58a2c" stroke-width="2"/><path d="M14 36h76v36H14z" fill="#ffdc85" stroke="#b58a2c" stroke-width="2"/>`),
  clipboard: svg(`<rect x="28" y="14" width="48" height="62" rx="4" fill="#d9d2c3" stroke="#8c836f" stroke-width="2"/><rect x="40" y="9" width="24" height="10" rx="3" fill="#9a9a9a"/><rect x="34" y="26" width="36" height="44" fill="#fff"/><text x="52" y="52" font-size="11" text-anchor="middle" fill="#666" font-family="Segoe UI">Ctrl+V</text>`),
  batch: svg(`<rect x="34" y="12" width="46" height="36" fill="#fff" stroke="#556" stroke-width="2"/><rect x="26" y="22" width="46" height="36" fill="#fff" stroke="#556" stroke-width="2"/><rect x="18" y="32" width="46" height="36" fill="#cfe2ff" stroke="#556" stroke-width="2"/><path d="M24 62l12-14 9 9 6-6 10 11z" fill="#3b7d3b"/>`),
  addImages: svg(`<path d="M14 24h26l7 7h43v41H14z" fill="#f1c96a" stroke="#b58a2c" stroke-width="2"/><circle cx="26" cy="66" r="13" fill="#2e9b3a" stroke="#fff" stroke-width="3"/><path d="M26 59v14M19 66h14" stroke="#fff" stroke-width="4"/>`),
  auto: svg(`<circle cx="52" cy="44" r="30" fill="#e8f3e8" stroke="#2e9b3a" stroke-width="3"/><path d="M58 18L38 48h14l-6 24 22-32H54z" fill="#f6b21a" stroke="#a87410" stroke-width="2" stroke-linejoin="round"/>`),
  basic: svg(`<rect x="22" y="14" width="60" height="60" rx="6" fill="#fff" stroke="#556" stroke-width="2"/>${[0, 1, 2].map((i) => `<rect x="30" y="${24 + i * 16}" width="10" height="10" fill="${i === 1 ? "#2a5db0" : "#fff"}" stroke="#2a5db0" stroke-width="2"/><rect x="46" y="${27 + i * 16}" width="28" height="4" fill="#99a"/>`).join("")}`),
  advanced: svg(`${[0, 1, 2, 3].map((i) => `<rect x="${22 + i * 18}" y="16" width="4" height="58" fill="#556"/><rect x="${16 + i * 18}" y="${[40, 24, 52, 32][i]}" width="16" height="10" rx="2" fill="#e9eaee" stroke="#333" stroke-width="2"/>`).join("")}`),
  photo: svg(`<rect x="10" y="10" width="84" height="68" fill="#9fd0f4"/><circle cx="72" cy="28" r="9" fill="#ffd34d"/><path d="M10 78l26-34 16 18 12-12 30 28z" fill="#4f8a3c"/><path d="M10 78l26-34 8 9" fill="#6aa253"/>`),
  blended: svg(`<rect x="10" y="10" width="84" height="68" fill="#fff"/><rect x="11" y="11" width="16" height="12" fill="#fff" stroke="#1a4fa0" stroke-width="1.5"/>${STAIR(true)}`),
  flat: svg(`<rect x="10" y="10" width="84" height="68" fill="#fff"/><rect x="11" y="11" width="16" height="12" fill="#fff" stroke="#1a4fa0" stroke-width="1.5"/>${STAIR(false)}`),
  qHigh: svg(`${STAIR(true)}`),
  qMedium: svg(`<filter id="b1"><feGaussianBlur stdDeviation="1.4"/></filter><g filter="url(#b1)">${STAIR(true)}</g><circle cx="30" cy="30" r="2" fill="#c9d3ef"/><circle cx="44" cy="22" r="1.5" fill="#c9d3ef"/>`),
  qLow: svg(`<filter id="b2"><feGaussianBlur stdDeviation="3.2"/></filter><g filter="url(#b2)">${STAIR(true)}</g>${[[24, 30], [38, 18], [30, 46], [50, 36], [22, 60]].map(([x, y]) => `<circle cx="${x}" cy="${y}" r="2.5" fill="#d8b9e8"/>`).join("")}`),
  two: svg(`<rect x="10" y="10" width="84" height="68" fill="#fff"/><path d="M52 14c18 10 26 26 18 44-6 12-18 18-18 18s-12-6-18-18c-8-18 0-34 18-44z" fill="#111"/><path d="M52 22v50" stroke="#fff" stroke-width="2"/>`),
  custom: svg(`<ellipse cx="52" cy="46" rx="38" ry="30" fill="#e8c98f" stroke="#a5824a" stroke-width="2"/><circle cx="36" cy="36" r="6" fill="#d33"/><circle cx="52" cy="28" r="6" fill="#2a5db0"/><circle cx="68" cy="36" r="6" fill="#2e9b3a"/><circle cx="40" cy="56" r="6" fill="#f6b21a"/><circle cx="64" cy="56" r="7" fill="#fff" stroke="#a5824a" stroke-width="2"/>`),
  unlimited: svg(`<defs><linearGradient id="rb" x1="0" x2="1"><stop offset="0" stop-color="#e53935"/><stop offset=".2" stop-color="#fb8c00"/><stop offset=".4" stop-color="#fdd835"/><stop offset=".6" stop-color="#43a047"/><stop offset=".8" stop-color="#1e88e5"/><stop offset="1" stop-color="#8e24aa"/></linearGradient><linearGradient id="wh" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity="0"/><stop offset="1" stop-color="#fff" stop-opacity=".9"/></linearGradient></defs><rect x="10" y="10" width="84" height="68" fill="url(#rb)"/><rect x="10" y="10" width="84" height="68" fill="url(#wh)"/>`),
  done: svg(`<circle cx="52" cy="44" r="32" fill="#e3f4e1" stroke="#2e9b3a" stroke-width="4"/><path d="M36 45l11 11 22-24" fill="none" stroke="#2e9b3a" stroke-width="8" stroke-linecap="round" stroke-linejoin="round"/>`),
  save: svg(`<rect x="22" y="14" width="60" height="62" rx="4" fill="#5d6b84" stroke="#3b465a" stroke-width="2"/><rect x="32" y="14" width="38" height="22" fill="#e9edf3"/><rect x="58" y="18" width="7" height="14" fill="#5d6b84"/><rect x="30" y="48" width="44" height="22" rx="2" fill="#fff"/>`),
};

// ---------------------------------------------------------------- texts
const KIND_SUMMARY = { photo: "Photograph", art_blended: "Anti-aliased artwork", art_flat: "Aliased artwork" };
const Q_LABEL = { high: "High", medium: "Medium", low: "Low" };
// Complexity 1..12 → number of colors in advanced mode.
const COMPLEXITY_COLORS = [2, 3, 4, 5, 6, 8, 10, 13, 17, 22, 30, 40];

// ---------------------------------------------------------------- state
const ADV_DEFAULTS = {
  complexity: 5, minPixels: 0, aaRejection: "aggressive", autoSeg: true,
  smoothness: 6, corners: true, aaEngine: true, nodes: 6, autoVec: true, curves: "curves", denoise: "",
};
const EXPORT_DEFAULTS = { format: "svg", shape: "cutout_group", stroke: "none", dxf: "more", bitmapFormat: "png", bitmapScale: 2 };

const S = {
  info: null,                // {name, path, width, height, png, suggested_kind, exact_colors}
  view: null,                // last result from the engine
  page: "load",
  history: [],
  mode: store.get("mode", "basic"),
  kind: "art_blended",
  quality: "high",
  colorMode: "custom",       // two | custom | unlimited
  palette: [],               // custom palette (hex)
  palSel: 0,
  sugg: null,                // {palettes, auto_index}
  maxK: 12,
  activeSugg: -1,
  adv: { ...ADV_DEFAULTS },
  exp: { ...EXPORT_DEFAULTS, ...store.get("export", {}) },
  prefs: { skipReview: false, ...store.get("prefs", {}) },
  ran: false,                // a result exists for the current settings
  viewMode: "original",      // original | segments | result
  split: false,
  paths: false,
  tool: "pan",
  label: 0,                  // color used by pencil and fill
  autoEdit: true,
  wholeColor: false,
  z: 1, px: 0, py: 0, space: false, shift: false,
  problems: [], probIdx: -1,
  crop: null,
  batch: { files: [], sel: -1, out: "", outMode: "same", formats: store.get("batchFormats", ["svg"]), auto: true, running: false, log: [] },
  helpTopic: null,
  helpHistory: [],
};

const panes = {};
for (const el of document.querySelectorAll(".pane")) {
  panes[el.dataset.pane] = { el, viewport: el.querySelector(".viewport"), stage: el.querySelector(".stage") };
}

// ---------------------------------------------------------------- status & busy
function setStatus(text, isError = false) {
  const s = $("statusMsg");
  s.textContent = text || "";
  s.classList.toggle("error", isError);
}

function summary() {
  if (!S.info) return "";
  if (S.mode === "advanced") return "Advanced";
  if (S.mode === "auto") return "Fully Automatic";
  const colors = S.colorMode === "unlimited" ? "Unlimited colors" : S.colorMode === "two" ? "2 colors"
    : S.palette.length ? `${S.palette.length} colors` : "Fixed colors";
  return `Basic: ${KIND_SUMMARY[S.kind]}, ${Q_LABEL[S.quality]}, ${colors}`;
}

function updateStatusInfo() {
  if (!S.info) {
    $("statusInfo").textContent = "";
    return;
  }
  const mp = (S.info.width * S.info.height / 1e6).toFixed(1);
  $("statusInfo").textContent = `${S.info.width}x${S.info.height} (${mp} megapixel)   ${summary()}`;
  document.title = `${S.info.name || "image"} - Vector Free`;
}

async function busy(text, fn) {
  $("busyText").textContent = text;
  $("busy").hidden = false;
  try {
    return await fn();
  } catch (e) {
    setStatus(String(e), true);
    return undefined;
  } finally {
    $("busy").hidden = true;
  }
}

// ---------------------------------------------------------------- settings
function buildSettings() {
  const s = {
    kind: S.kind,
    quality: S.quality,
    num_colors: null,
    palette: [],
    remove_background: false,
    advanced: {},
  };
  if (S.mode === "advanced") {
    const a = S.adv;
    s.quality = "medium";
    s.num_colors = COMPLEXITY_COLORS[a.complexity - 1];
    s.advanced = {
      min_area: a.minPixels > 0 ? a.minPixels : null,
      aa_rejection: a.aaRejection,
      smoothing: Math.pow(2, (a.smoothness - 6) / 3),
      tolerance_scale: Math.pow(2, (6 - a.nodes) / 3),
      detect_corners: a.corners,
      aa_engine: a.aaEngine,
      curves: a.curves,
      denoise: a.denoise === "" ? null : a.denoise === "on",
    };
  } else if (S.mode === "basic" && S.colorMode === "two" && !S.palette.length) {
    s.num_colors = 2;
  }
  return s;
}

function exportOptions() {
  const e = S.exp;
  return {
    layout: e.shape === "stack" ? "stacked" : "cutout",
    group_by_color: e.shape !== "cutout",
    stroke: e.stroke === "stroke",
    stroke_width: 0.5,
    raster_scale: Number(e.bitmapScale) || 1,
    dxf_splines: e.dxf === "splines",
    dxf_flatten: e.dxf === "fewer" ? 1.0 : 0.25,
  };
}

// ---------------------------------------------------------------- vectorizing
function usesCustomPalette() {
  return S.mode === "basic" && (S.colorMode === "custom" || S.colorMode === "two") && S.palette.length > 0;
}

async function runVectorize(text = "Vectorizing…") {
  if (!S.info) return false;
  clearProblems();
  const settings = buildSettings();
  const t0 = performance.now();
  const v = await busy(text, () => (usesCustomPalette()
    ? invoke("resegment", { palette: S.palette, settings })
    : invoke("process", { settings })));
  if (!v) return false;
  setView(v);
  S.ran = true;
  setStatus(`${visibleColors()} colors · ${v.paths} paths · ${v.nodes} nodes · ${((performance.now() - t0) / 1000).toFixed(2)} s`);
  if (S.mode !== "advanced") rememberBasic();
  return true;
}

async function refit(text = "Smoothing paths…") {
  const v = await busy(text, () => invoke("revectorize", { settings: buildSettings() }));
  if (v) setView(v, true);
}

function visibleColors() {
  return S.view ? S.view.palette.filter((p) => !p.transparent).length : 0;
}

function setView(v, keepZoom = true) {
  S.view = v;
  if (S.label >= v.palette.length) S.label = 0;
  renderResult();
  setImage("segments", v.seg_png, "pixelated");
  updateHistoryButtons(v.can_undo, v.can_redo);
  updateToolSwatch();
  updateViewButtons();
  updateStatusInfo();
  if (!keepZoom) requestAnimationFrame(fit);
}

function handleEdit(r) {
  if (r.view) setView(r.view, true);
  else {
    setImage("segments", r.seg_png, "pixelated");
    setStatus("Segmentation edited. The vector result updates when you click Update.");
  }
  updateHistoryButtons(r.can_undo, r.can_redo);
  if (S.problems.length) findProblems(true);
}

async function runEdit(op) {
  try {
    handleEdit(await invoke("edit", { op, revectorize: S.autoEdit }));
  } catch (e) {
    setStatus(String(e), true);
  }
}

// ---------------------------------------------------------------- viewer
function applyTransform() {
  for (const p of Object.values(panes)) {
    p.stage.style.transform = `translate(${S.px}px, ${S.py}px) scale(${S.z})`;
    p.stage.style.setProperty("--z", S.z);
  }
  $("zoomLabel").textContent = `${Math.round(S.z * 100)}%`;
  $("zoomSlider").value = Math.log2(S.z);
}

function visibleViewport() {
  return Object.values(panes).find((p) => !p.el.hidden)?.viewport;
}

function fit() {
  if (!S.info) return;
  const vp = visibleViewport();
  if (!vp) return;
  const r = vp.getBoundingClientRect();
  S.z = Math.max(0.02, Math.min(r.width / S.info.width, r.height / S.info.height) * 0.92);
  S.px = (r.width - S.info.width * S.z) / 2;
  S.py = (r.height - S.info.height * S.z) / 2;
  applyTransform();
}

function zoomAt(newZ, mx, my) {
  const nz = Math.min(64, Math.max(0.02, newZ));
  S.px = mx - (mx - S.px) * (nz / S.z);
  S.py = my - (my - S.py) * (nz / S.z);
  S.z = nz;
  applyTransform();
}

function zoomCenter(factor) {
  const vp = visibleViewport();
  if (!vp) return;
  const r = vp.getBoundingClientRect();
  zoomAt(S.z * factor, r.width / 2, r.height / 2);
}

function centerOn(x, y, minZoom) {
  const vp = visibleViewport();
  if (!vp) return;
  const r = vp.getBoundingClientRect();
  S.z = Math.max(S.z, minZoom);
  S.px = r.width / 2 - x * S.z;
  S.py = r.height / 2 - y * S.z;
  applyTransform();
}

function setViewMode(mode) {
  if (mode === "segments" && !S.view) mode = "original";
  if (mode === "result" && !S.view) mode = "original";
  S.viewMode = mode;
  const show = S.split && S.view ? ["original", mode === "original" ? "result" : mode] : [mode];
  for (const [name, p] of Object.entries(panes)) p.el.hidden = !show.includes(name);
  $("panes").classList.toggle("split", show.length > 1);
  updateViewButtons();
  requestAnimationFrame(() => (S.info ? (S.z === 1 && S.px === 0 ? fit() : applyTransform()) : null));
}

function updateViewButtons() {
  for (const b of document.querySelectorAll("#viewFloat button[data-view]")) {
    b.classList.toggle("on", b.dataset.view === S.viewMode);
    b.disabled = b.dataset.view !== "original" && !S.view;
  }
}

function overlay(stage) {
  let o = stage.querySelector("svg.overlay");
  if (!o) {
    o = document.createElementNS(SVGNS, "svg");
    o.classList.add("overlay");
    stage.appendChild(o);
  }
  o.setAttribute("viewBox", `0 0 ${S.info.width} ${S.info.height}`);
  o.setAttribute("width", S.info.width);
  o.setAttribute("height", S.info.height);
  return o;
}

function makeImg(src, cls) {
  const img = new Image();
  img.src = src;
  img.width = S.info.width;
  img.height = S.info.height;
  img.draggable = false;
  if (cls) img.className = cls;
  return img;
}

function setImage(pane, src, cls) {
  const { stage } = panes[pane];
  stage.innerHTML = "";
  if (!src || !S.info) return;
  stage.appendChild(makeImg(src, cls));
  overlay(stage);
  drawMarkers();
}

function svgLayer(markup, cls) {
  const wrap = document.createElement("div");
  wrap.innerHTML = markup.replace(/^<\?xml[^>]*>\s*/, "");
  const el = wrap.querySelector("svg");
  el.classList.add(cls);
  el.setAttribute("width", S.info.width);
  el.setAttribute("height", S.info.height);
  return el;
}

function nodeLayer(src) {
  const layer = document.createElementNS(SVGNS, "svg");
  layer.classList.add("nodes");
  layer.setAttribute("viewBox", `0 0 ${S.info.width} ${S.info.height}`);
  layer.setAttribute("width", S.info.width);
  layer.setAttribute("height", S.info.height);
  let count = 0;
  const frag = document.createDocumentFragment();
  for (const path of src.querySelectorAll("path")) {
    for (const m of (path.getAttribute("d") || "").matchAll(/([MLC])([^MLCZ]*)/g)) {
      const nums = m[2].trim().split(/[\s,]+/).map(Number);
      if (nums.length < 2) continue;
      const c = document.createElementNS(SVGNS, "circle");
      c.setAttribute("cx", nums[nums.length - 2]);
      c.setAttribute("cy", nums[nums.length - 1]);
      c.style.r = "calc(2.5px / var(--z))";
      frag.appendChild(c);
      if (++count > 20000) return null;
    }
  }
  layer.appendChild(frag);
  return layer;
}

function renderResult() {
  const { stage } = panes.result;
  stage.innerHTML = "";
  if (!S.view || !S.info) return;
  const vec = svgLayer(S.view.svg, "vector");
  stage.appendChild(vec);
  if (S.paths) {
    stage.appendChild(svgLayer(S.view.svg, "paths")).style.position = "absolute";
    stage.lastChild.style.inset = "0";
    const nodes = nodeLayer(vec);
    if (nodes) stage.appendChild(nodes);
  }
  if (S.view.erased_svg) stage.appendChild(svgLayer(S.view.erased_svg, "erased"));
  overlay(stage);
  drawMarkers();
}

function drawMarkers() {
  if (!S.info) return;
  for (const name of ["result", "segments", "original"]) {
    const { stage } = panes[name];
    if (!stage.firstChild) continue;
    const o = overlay(stage);
    o.querySelectorAll(".marker").forEach((m) => m.remove());
    S.problems.forEach((p, i) => {
      const c = document.createElementNS(SVGNS, "circle");
      c.setAttribute("cx", p.x);
      c.setAttribute("cy", p.y);
      c.style.r = "calc(9px / var(--z))";
      c.classList.add("marker");
      if (i === S.probIdx) c.classList.add("current");
      o.appendChild(c);
    });
  }
}

// ---------------------------------------------------------------- edit tools (floating palette)
const EDIT_TOOLS = [
  ["pan", "✋ Pan (A)"], ["zap", "⚡ Zap (D)"], ["fill", "🪣 Fill (F)"], ["pencil", "✏ Pencil (X)"], ["color", "💧 Color (C)"],
];
const ERASE_TOOLS = [["pan", "✋ Pan (A)"], ["erase", "✂ Erase (D)"]];

function showTools(kind) {
  $("toolsFloat").hidden = !kind || !S.view;
  if (!kind) return;
  const list = kind === "erase" ? ERASE_TOOLS : EDIT_TOOLS;
  $("toolButtons").innerHTML = "";
  for (const [id, label] of list) {
    $("toolButtons").append(h("button", { "data-tool": id, onclick: () => setTool(id) }, label));
  }
  $("brushRow").hidden = kind === "erase";
  $("toolSwatch").hidden = kind === "erase";
  if (!list.some(([id]) => id === S.tool)) setTool(kind === "erase" ? "erase" : "pan");
  else setTool(S.tool);
}

function setTool(tool) {
  if (tool !== "crop" && S.crop) endCrop();
  S.tool = tool;
  for (const b of document.querySelectorAll("#toolButtons button")) b.classList.toggle("on", b.dataset.tool === tool);
  for (const p of Object.values(panes)) {
    p.viewport.classList.toggle("pencil", ["pencil", "zap", "fill", "erase", "crop"].includes(tool));
    p.viewport.classList.toggle("picker", tool === "color" || tool === "pick");
  }
}

function updateToolSwatch() {
  const sw = $("toolSwatch");
  sw.innerHTML = "";
  if (!S.view) return;
  const cur = S.view.palette[S.label];
  sw.style.background = cur && !cur.transparent ? cur.hex : "repeating-conic-gradient(#bbb 0 25%, #fff 0 50%) 0 0/10px 10px";
  sw.title = "Current color — click to choose another";
  sw.onclick = () => {
    S.label = (S.label + 1) % S.view.palette.length;
    updateToolSwatch();
  };
}

function updateHistoryButtons(canUndo, canRedo) {
  const u = document.querySelector('#toolsFloat [data-cmd="undo"]');
  const r = document.querySelector('#toolsFloat [data-cmd="redo"]');
  if (u) u.disabled = !canUndo;
  if (r) r.disabled = !canRedo;
}

async function history(cmd) {
  try {
    if (cmd === "revert") handleEdit(await busy("Reverting…", () => invoke("reset_edits")));
    else handleEdit(await invoke(cmd, { revectorize: S.autoEdit }));
  } catch (e) {
    setStatus(String(e), true);
  }
}
for (const b of document.querySelectorAll("#toolsFloat [data-cmd]")) b.addEventListener("click", () => history(b.dataset.cmd));
$("brush").addEventListener("input", () => ($("brushOut").textContent = $("brush").value));

// ---------------------------------------------------------------- problem finder
async function findProblems(quiet = false) {
  if (!S.view) return;
  try {
    S.problems = await invoke("find_problems");
  } catch (e) {
    return setStatus(String(e), true);
  }
  S.probIdx = -1;
  drawMarkers();
  if (!quiet) setStatus(S.problems.length ? `${S.problems.length} pinch points found.` : "No pinch points found.");
  if (S.page === "editSeg") renderPage();
}

function clearProblems() {
  if (!S.problems.length) return;
  S.problems = [];
  S.probIdx = -1;
  drawMarkers();
}

function gotoProblem(step) {
  const n = S.problems.length;
  if (!n) return;
  S.probIdx = (S.probIdx + step + n) % n;
  const p = S.problems[S.probIdx];
  if (S.viewMode !== "segments") setViewMode("segments");
  centerOn(p.x, p.y, 6);
  drawMarkers();
  setStatus(`Problem ${S.probIdx + 1} of ${n}`);
}

// ---------------------------------------------------------------- crop
function startCrop() {
  if (!S.info) return;
  setViewMode("original");
  setTool("crop");
  S.crop = null;
  $("cropBar").hidden = false;
  $("cropApply").disabled = true;
}

function endCrop() {
  S.crop = null;
  $("cropBar").hidden = true;
  for (const p of Object.values(panes)) p.stage.querySelectorAll(".crop-rect").forEach((r) => r.remove());
}

function drawCrop() {
  for (const p of Object.values(panes)) {
    if (!p.stage.firstChild) continue;
    const o = overlay(p.stage);
    let r = o.querySelector(".crop-rect");
    if (!r) {
      r = document.createElementNS(SVGNS, "rect");
      r.classList.add("crop-rect");
      o.appendChild(r);
    }
    const c = S.crop;
    r.setAttribute("x", Math.min(c.x0, c.x1));
    r.setAttribute("y", Math.min(c.y0, c.y1));
    r.setAttribute("width", Math.abs(c.x1 - c.x0));
    r.setAttribute("height", Math.abs(c.y1 - c.y0));
  }
}

async function applyCrop() {
  const c = S.crop;
  if (!c) return;
  const clamp = (v, m) => Math.max(0, Math.min(m, Math.round(v)));
  const x0 = clamp(Math.min(c.x0, c.x1), S.info.width), x1 = clamp(Math.max(c.x0, c.x1), S.info.width);
  const y0 = clamp(Math.min(c.y0, c.y1), S.info.height), y1 = clamp(Math.max(c.y0, c.y1), S.info.height);
  if (x1 - x0 < 2 || y1 - y0 < 2) return setStatus("The crop rectangle is too small.", true);
  const { name, path } = S.info;
  endCrop();
  setTool("pan");
  const info = await busy("Cropping…", () => invoke("crop", { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }));
  if (info) {
    Object.assign(info, { name, path });
    showImage(info);
    renderPage();
  }
}
$("cropApply").addEventListener("click", applyCrop);
$("cropCancel").addEventListener("click", () => { endCrop(); setTool("pan"); });

// ---------------------------------------------------------------- pointer handling
function imageCoords(viewport, ev) {
  const r = viewport.getBoundingClientRect();
  return [(ev.clientX - r.left - S.px) / S.z, (ev.clientY - r.top - S.py) / S.z];
}

async function pickIntoPalette(pt, blend) {
  const hex = await invoke("pick_color", { x: pt[0], y: pt[1] }).catch((e) => setStatus(String(e), true));
  if (!hex) return;
  if (!S.palette.length) S.palette = [hex];
  const i = Math.min(S.palSel, S.palette.length - 1);
  if (blend) {
    const a = parseInt(S.palette[i].slice(1), 16), b = parseInt(hex.slice(1), 16);
    const mix = [16, 8, 0].map((sh) => Math.round((((a >> sh) & 255) + ((b >> sh) & 255)) / 2));
    S.palette[i] = "#" + mix.map((v) => v.toString(16).padStart(2, "0")).join("");
  } else {
    S.palette[i] = hex;
  }
  S.activeSugg = -1;
  S.ran = false;
  renderPage();
}

for (const p of Object.values(panes)) {
  const vp = p.viewport;
  let drag = null;
  let stroke = null;

  vp.addEventListener("wheel", (ev) => {
    ev.preventDefault();
    const r = vp.getBoundingClientRect();
    zoomAt(S.z * (ev.deltaY < 0 ? 1.15 : 1 / 1.15), ev.clientX - r.left, ev.clientY - r.top);
  }, { passive: false });

  vp.addEventListener("pointerdown", (ev) => {
    if (!S.info) return;
    vp.setPointerCapture(ev.pointerId);
    const pt = imageCoords(vp, ev);
    const panning = S.tool === "pan" || ev.button === 1 || S.space || (ev.shiftKey && S.tool !== "crop");
    if (S.page === "palette" && ev.button === 0 && !panning) {
      pickIntoPalette(pt, ev.ctrlKey);
      return;
    }
    if (panning || (S.tool === "pick")) {
      drag = { x: ev.clientX - S.px, y: ev.clientY - S.py };
      vp.classList.add("dragging");
      return;
    }
    if (ev.button !== 0) return;
    if (S.tool === "crop") {
      S.crop = { x0: pt[0], y0: pt[1], x1: pt[0], y1: pt[1] };
      stroke = { crop: true };
      drawCrop();
      return;
    }
    if (!S.view) return;
    if (S.tool === "color") {
      invoke("label_at", { x: pt[0], y: pt[1] }).then((l) => {
        S.label = l;
        updateToolSwatch();
        setStatus(`Color ${S.view.palette[l].hex} selected for Pencil and Fill.`);
      }).catch((e) => setStatus(String(e), true));
      return;
    }
    if (S.tool === "fill") return runEdit({ tool: "fill", x: pt[0], y: pt[1], label: S.label });
    if (S.tool === "erase") return runEdit({ tool: "erase", x: pt[0], y: pt[1], whole_color: S.wholeColor });
    if (S.tool === "pencil" || S.tool === "zap") {
      const color = S.view.palette[S.label];
      const zap = S.tool === "zap";
      const line = document.createElementNS(SVGNS, "polyline");
      line.setAttribute("fill", "none");
      line.setAttribute("stroke", zap ? "#ff2bd6" : color.transparent ? "#ff00ff" : color.hex);
      line.setAttribute("stroke-opacity", zap ? "0.5" : "1");
      line.setAttribute("stroke-width", Number($("brush").value) * 2);
      line.setAttribute("stroke-linecap", "round");
      line.setAttribute("stroke-linejoin", "round");
      overlay(p.stage).appendChild(line);
      stroke = { points: [pt], line, zap, start: [ev.clientX, ev.clientY], moved: false };
      line.setAttribute("points", pt.join(","));
    }
  });

  vp.addEventListener("pointermove", (ev) => {
    if (drag) {
      S.px = ev.clientX - drag.x;
      S.py = ev.clientY - drag.y;
      applyTransform();
    } else if (stroke && stroke.crop) {
      const pt = imageCoords(vp, ev);
      S.crop.x1 = pt[0];
      S.crop.y1 = pt[1];
      drawCrop();
    } else if (stroke) {
      stroke.points.push(imageCoords(vp, ev));
      if (Math.hypot(ev.clientX - stroke.start[0], ev.clientY - stroke.start[1]) > 3) stroke.moved = true;
      stroke.line.setAttribute("points", stroke.points.map((q) => q.join(",")).join(" "));
    }
  });

  const end = () => {
    if (drag) {
      drag = null;
      vp.classList.remove("dragging");
    }
    if (stroke && stroke.crop) {
      stroke = null;
      $("cropApply").disabled = false;
      return;
    }
    if (stroke) {
      const s = stroke;
      stroke = null;
      const radius = Number($("brush").value);
      let op;
      if (s.zap && !s.moved) op = { tool: "zap_region", x: s.points[0][0], y: s.points[0][1] };
      else if (s.zap) op = { tool: "zap", points: s.points, radius };
      else op = { tool: "pencil", points: s.points, radius, label: S.label };
      runEdit(op).finally(() => s.line.remove());
    }
  };
  vp.addEventListener("pointerup", end);
  vp.addEventListener("pointercancel", end);
}

// viewer floating controls
for (const b of document.querySelectorAll("#navFloat [data-pan]")) {
  b.addEventListener("click", () => {
    const step = 80;
    const d = { up: [0, step], down: [0, -step], left: [step, 0], right: [-step, 0] }[b.dataset.pan];
    S.px += d[0];
    S.py += d[1];
    applyTransform();
  });
}
document.querySelector('#navFloat [data-act="fit"]').addEventListener("click", fit);
document.querySelector('#navFloat [data-act="center"]').addEventListener("click", () => {
  const vp = visibleViewport();
  if (!vp || !S.info) return;
  const r = vp.getBoundingClientRect();
  S.z = 1;
  S.px = (r.width - S.info.width) / 2;
  S.py = (r.height - S.info.height) / 2;
  applyTransform();
});
$("zoomSlider").addEventListener("input", () => {
  const vp = visibleViewport();
  if (!vp) return;
  const r = vp.getBoundingClientRect();
  zoomAt(Math.pow(2, Number($("zoomSlider").value)), r.width / 2, r.height / 2);
});
for (const b of document.querySelectorAll("#viewFloat button[data-view]")) {
  b.addEventListener("click", () => setViewMode(b.dataset.view));
}
$("pathsCheck").addEventListener("change", () => {
  S.paths = $("pathsCheck").checked;
  renderResult();
});
$("splitCheck").addEventListener("change", () => {
  S.split = $("splitCheck").checked;
  setViewMode(S.viewMode === "original" && S.view ? "result" : S.viewMode);
  requestAnimationFrame(fit);
});
window.addEventListener("resize", () => S.info && fit());

// ---------------------------------------------------------------- recent lists & saved settings
function pushRecent(key, value, max = 8) {
  const list = store.get(key, []).filter((v) => v !== value);
  list.unshift(value);
  store.set(key, list.slice(0, max));
}

function rememberBasic() {
  if (S.mode === "advanced") return;
  const entry = { label: summary(), mode: S.mode, kind: S.kind, quality: S.quality, colorMode: S.colorMode };
  const list = store.get("recentBasic", []).filter((e) => e.label !== entry.label);
  list.unshift(entry);
  store.set("recentBasic", list.slice(0, 6));
}

function snapshotSettings(name) {
  return { name, mode: S.mode, kind: S.kind, quality: S.quality, colorMode: S.colorMode, palette: [...S.palette], adv: { ...S.adv } };
}

function applySnapshot(s) {
  S.mode = s.mode || "basic";
  S.kind = s.kind || S.kind;
  S.quality = s.quality || S.quality;
  S.colorMode = s.colorMode || S.colorMode;
  S.palette = s.palette ? [...s.palette] : [];
  S.adv = { ...ADV_DEFAULTS, ...(s.adv || {}) };
  S.ran = false;
}

const dirOf = (p) => p.replace(/[\\/][^\\/]*$/, "");
const fileOf = (p) => p.split(/[\\/]/).pop();

// ---------------------------------------------------------------- opening images
function showImage(info) {
  S.info = info;
  S.view = null;
  S.ran = false;
  S.problems = [];
  S.probIdx = -1;
  S.sugg = null;
  S.maxK = 12;
  S.activeSugg = -1;
  S.palette = [];
  S.kind = info.suggested_kind;
  S.quality = info.suggested_kind === "photo" ? "medium" : info.exact_colors <= 2000 ? "high" : "medium";
  S.colorMode = info.suggested_kind === "photo" ? "unlimited" : "custom";
  $("drop").hidden = true;
  $("navFloat").hidden = false;
  $("viewFloat").hidden = false;
  setImage("original", info.png, "pixelated");
  panes.result.stage.innerHTML = "";
  panes.segments.stage.innerHTML = "";
  S.z = 1; S.px = 0; S.py = 0;
  setViewMode("original");
  requestAnimationFrame(fit);
  updateStatusInfo();
  if (info.path) {
    pushRecent("recentImages", info.path);
    pushRecent("recentFolders", dirOf(info.path));
  }
}

async function openPath(path) {
  const info = await busy("Opening image…", () => invoke("open_image", { path }));
  if (!info) return;
  showImage(info);
  goTo("wizardMode", true);
}

async function openBytes(bytes) {
  const info = await busy("Opening pasted image…", () => invoke("open_image_bytes", bytes));
  if (!info) return;
  showImage(info);
  goTo("wizardMode", true);
}

async function openDialog(directory) {
  const [path] = await invoke("pick_images", { multiple: false, directory: directory || null });
  if (path) openPath(path);
}

async function pasteFromClipboard() {
  try {
    const items = await navigator.clipboard.read();
    for (const item of items) {
      const type = item.types.find((t) => t.startsWith("image/"));
      if (type) {
        const blob = await item.getType(type);
        return openBytes(new Uint8Array(await blob.arrayBuffer()));
      }
    }
    setStatus("There is no image on the clipboard.", true);
  } catch {
    setStatus("Could not read the clipboard here; press Ctrl+V instead.", true);
  }
}
document.addEventListener("paste", async (ev) => {
  if (document.querySelector("dialog[open]") || ev.target.matches("input, textarea")) return;
  const file = [...(ev.clipboardData?.items || [])].find((i) => i.type.startsWith("image/"))?.getAsFile();
  if (!file) return;
  ev.preventDefault();
  openBytes(new Uint8Array(await file.arrayBuffer()));
});

// ---------------------------------------------------------------- UI building blocks
function card({ icon, title, bullets = [], selected, recommended, disabled, onclick, ondblclick }) {
  return h("div", {
    class: `card${selected ? " selected" : ""}${disabled ? " disabled" : ""}`,
    onclick: disabled ? null : onclick,
    ondblclick: disabled ? null : ondblclick,
    tabindex: 0,
    role: "button",
  },
  h("div", { class: "pic", html: icon }),
  h("div", { class: "txt" },
    h("b", {}, title),
    h("ul", {}, bullets.map((b) => h("li", { html: b }))),
    h("div", { class: "tags" },
      recommended ? h("span", { class: "rec" }, "Recommended") : null,
      selected ? h("span", { class: "sel" }, "Selected") : null)));
}

/** A disclosure button with content shown below it when open. */
const openExpanders = new Set();
function expander(id, label, build) {
  const open = openExpanders.has(id);
  const btn = h("button", {
    class: `expander${open ? " open" : ""}`,
    onclick: () => {
      if (openExpanders.has(id)) openExpanders.delete(id);
      else openExpanders.add(id);
      renderPage();
    },
  }, label);
  return [btn, open ? h("div", { class: "exp-body" }, build()) : null];
}

function group(legend, ...kids) {
  return h("div", { class: "group" }, h("span", { class: "legend" }, legend), ...kids);
}

function slider({ min, max, value, left, right, oninput, onchange }) {
  const out = h("span", {}, `(${value})`);
  const input = h("input", {
    type: "range", min, max, step: 1, value,
    oninput: (e) => { out.textContent = `(${e.target.value})`; oninput && oninput(Number(e.target.value)); },
    onchange: (e) => onchange && onchange(Number(e.target.value)),
  });
  const ticks = h("div", { class: "ticks" }, Array.from({ length: max - min + 1 }, () => h("i")));
  return h("div", { class: "slider-row" }, input, ticks, h("div", { class: "slider-ends" }, h("span", {}, left), out, h("span", {}, right)));
}

function radio(name, options, current, onchange) {
  return h("div", { class: "radios" }, options.map(([value, label]) => h("label", { class: "check" },
    h("input", { type: "radio", name, value, checked: current === value, onchange: () => onchange(value) }), label)));
}

function checkbox(label, checked, onchange, title) {
  return h("label", { class: "check", title }, h("input", { type: "checkbox", checked, onchange: (e) => onchange(e.target.checked) }), label);
}

function selectBox(options, current, onchange) {
  return h("select", { onchange: (e) => onchange(e.target.value) },
    options.map(([v, label]) => h("option", { value: v, selected: v === current }, label)));
}

function miniPalette(hexes) {
  return h("div", { class: "mini-pal" }, hexes.map((c) => h("i", { style: `background:${c}` })));
}

let debounceTimer = null;
function debounced(fn, ms = 350) {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(fn, ms);
}

// ---------------------------------------------------------------- pages
const PAGES = {};

PAGES.load = {
  title: "Load Image",
  render: () => [
    card({ icon: ICON.folder, title: "Import from Folder", bullets: ["Tip: try the recently used images and folders listed below.", "Shortcut: Ctrl-O."], onclick: () => openDialog() }),
    card({ icon: ICON.clipboard, title: "Paste from Clipboard", bullets: ["Copy an image in any program, then click here.", "Shortcut: Ctrl-V."], onclick: pasteFromClipboard }),
    card({ icon: ICON.batch, title: "Batch Processing", bullets: ["Process multiple images in batch with the same settings."], onclick: () => goTo("batchFiles") }),
    expander("recentFolders", "Recent Folders", () => {
      const list = store.get("recentFolders", []);
      return list.length ? list.map((d) => h("button", { class: "item", title: d, onclick: () => openDialog(d) }, d)) : h("div", { class: "empty" }, "No recent folders.");
    }),
    expander("recentImages", "Recent Images", () => {
      const list = store.get("recentImages", []);
      return list.length ? list.map((p) => h("button", { class: "item", title: p, onclick: () => openPath(p) }, fileOf(p))) : h("div", { class: "empty" }, "No recent images.");
    }),
    h("div", { class: "spacer" }),
    h("div", { class: "row", style: "justify-content:flex-end" },
      h("button", { onclick: (e) => openMenu(e.currentTarget) }, "More Options ▾")),
  ],
  canNext: () => !!S.info,
  next: () => "wizardMode",
};

PAGES.wizardMode = {
  title: "Wizard Mode",
  render: () => [
    card({ icon: ICON.auto, title: "Fully Automatic", bullets: ["Single-click vectorization.", "Vector Free answers every question itself."],
      selected: S.mode === "auto", onclick: () => setMode("auto"), ondblclick: () => { setMode("auto"); nav("next"); } }),
    card({ icon: ICON.basic, title: "Basic", bullets: ["A few simple multiple-choice questions.", "Best for most images."],
      selected: S.mode === "basic", onclick: () => setMode("basic"), ondblclick: () => { setMode("basic"); nav("next"); } }),
    card({ icon: ICON.advanced, title: "Advanced", bullets: ["Fine-tuned control.", "More complicated choices."],
      selected: S.mode === "advanced", onclick: () => setMode("advanced"), ondblclick: () => { setMode("advanced"); nav("next"); } }),
    expander("recentBasic", "Recent Basic Settings", () => {
      const list = store.get("recentBasic", []);
      return list.length ? list.map((e) => h("button", { class: "item", title: "Use these settings", onclick: async () => {
        applySnapshot(e);
        if (await runVectorize()) goTo(S.prefs.skipReview ? "export" : "review");
      } }, e.label)) : h("div", { class: "empty" }, "No recent settings yet.");
    }),
    expander("savedAdvanced", "Saved Advanced Settings", () => {
      const list = store.get("savedSettings", []);
      return list.length ? list.map((e) => h("button", { class: "item", onclick: async () => {
        applySnapshot(e);
        if (S.mode === "advanced") goTo("advSegmentation");
        else if (await runVectorize()) goTo("review");
      } }, e.name)) : h("div", { class: "empty" }, "Save settings on the Export Result page.");
    }),
    expander("cropImage", "Crop Image", () => [
      h("div", { class: "empty" }, "Trace only part of the image (useful for very large images)."),
      h("button", { class: "item", onclick: startCrop }, "Start cropping"),
    ]),
  ],
  canNext: () => !!S.info,
  next: async () => {
    if (S.mode === "auto") {
      S.kind = S.info.suggested_kind;
      S.quality = S.kind === "photo" ? "medium" : S.info.exact_colors <= 2000 ? "high" : "medium";
      S.palette = [];
      return (await runVectorize()) ? (S.prefs.skipReview ? "export" : "review") : null;
    }
    return S.mode === "advanced" ? "advSegmentation" : "imageType";
  },
};

function setMode(m) {
  S.mode = m;
  store.set("mode", m);
  S.ran = false;
  renderPage();
  updateStatusInfo();
}

PAGES.imageType = {
  title: "Image Type",
  render: () => {
    const rec = S.info?.suggested_kind;
    const pick = (k) => () => { S.kind = k; S.ran = false; S.sugg = null; renderPage(); updateStatusInfo(); };
    return [
      card({ icon: ICON.photo, title: "Photograph", bullets: ["Taken with a <b>camera</b>.", "Also detailed artwork with gradients."],
        selected: S.kind === "photo", recommended: rec === "photo", onclick: pick("photo"), ondblclick: () => { pick("photo")(); nav("next"); } }),
      card({ icon: ICON.blended, title: "Artwork with Blended Edges", bullets: ["<b>Anti-aliased</b>: edge pixels mix the two colors.", "<em>The most common kind of artwork.</em>"],
        selected: S.kind === "art_blended", recommended: rec === "art_blended", onclick: pick("art_blended"), ondblclick: () => { pick("art_blended")(); nav("next"); } }),
      card({ icon: ICON.flat, title: "Artwork without Blended Edges", bullets: ["<b>Not</b> anti-aliased: hard pixel steps.", "<em>Zoom in on an edge to check.</em>"],
        selected: S.kind === "art_flat", recommended: rec === "art_flat", onclick: pick("art_flat"), ondblclick: () => { pick("art_flat")(); nav("next"); } }),
    ];
  },
  next: () => "quality",
};

PAGES.quality = {
  get title() { return S.kind === "photo" ? "Detail Level" : "Input Quality"; },
  render: () => {
    const photo = S.kind === "photo";
    const rec = photo ? "medium" : S.info && S.info.exact_colors <= 2000 ? "high" : "medium";
    const pick = (q) => () => { S.quality = q; S.ran = false; renderPage(); updateStatusInfo(); };
    const text = photo ? {
      high: ["High fidelity.", "Larger, more complex result."],
      medium: ["Usually the best balance."],
      low: ["Fewer, smoother shapes.", "Looks like a drawing."],
    } : {
      high: ["Is <b>sharp</b>.", "Is <b>not</b> degraded.", "Has <b>no/few</b> compression artifacts."],
      medium: ["Is <b>somewhat</b> degraded.", "Has <b>some</b> compression artifacts or noise."],
      low: ["Is <b>blurry</b>.", "Is <b>significantly</b> degraded.", "Has <b>a lot</b> of artifacts or noise."],
    };
    const icons = { high: ICON.qHigh, medium: ICON.qMedium, low: ICON.qLow };
    return ["high", "medium", "low"].map((q) => card({
      icon: icons[q], title: Q_LABEL[q], bullets: text[q], selected: S.quality === q, recommended: rec === q,
      onclick: pick(q), ondblclick: () => { pick(q)(); nav("next"); },
    }));
  },
  next: () => "colorMode",
};

PAGES.colorMode = {
  title: "Color Mode",
  render: () => {
    const rec = S.kind === "photo" ? "unlimited" : "custom";
    const pick = (m) => () => { S.colorMode = m; S.palette = []; S.activeSugg = -1; S.ran = false; renderPage(); updateStatusInfo(); };
    return [
      card({ icon: ICON.two, title: "Two Colors", bullets: ["Use only two colors.", "For black-and-white art, signatures, stencils."],
        selected: S.colorMode === "two", recommended: rec === "two", onclick: pick("two"), ondblclick: () => { pick("two")(); nav("next"); } }),
      card({ icon: ICON.custom, title: "Custom Colors", bullets: ["Review suggested colors.", "Use only colors you specify.", "<em>Can reduce errors</em> from noise or blur."],
        selected: S.colorMode === "custom", recommended: rec === "custom", onclick: pick("custom"), ondblclick: () => { pick("custom")(); nav("next"); } }),
      card({ icon: ICON.unlimited, title: "Unlimited Colors", bullets: ["Use all colors necessary.", "Good for gradients.", "<em>May leave errors</em> from noise or blur."],
        selected: S.colorMode === "unlimited", recommended: rec === "unlimited", onclick: pick("unlimited"), ondblclick: () => { pick("unlimited")(); nav("next"); } }),
    ];
  },
  next: async () => {
    if (S.colorMode === "unlimited") return (await runVectorize()) ? (S.prefs.skipReview ? "export" : "review") : null;
    return "palette";
  },
};

async function loadSuggestions() {
  const settings = buildSettings();
  const s = await busy("Finding palettes…", () => invoke("suggested_palettes", { settings, maxColors: S.maxK }));
  if (!s) return;
  S.sugg = s;
  if (!S.palette.length) {
    const idx = S.colorMode === "two" ? 0 : s.auto_index;
    S.palette = [...s.palettes[idx]];
    S.activeSugg = idx;
    S.palSel = 0;
  }
  if (S.page === "palette") renderPage();
}

PAGES.palette = {
  title: "Palette Selection",
  onEnter: () => {
    if (!S.sugg) loadSuggestions();
    else if (!S.palette.length) {
      const idx = S.colorMode === "two" ? 0 : S.sugg.auto_index;
      S.palette = [...S.sugg.palettes[idx]];
      S.activeSugg = idx;
    }
  },
  render: () => {
    const sug = h("div", { class: "sugg" });
    if (S.sugg) {
      S.sugg.palettes.forEach((pal, i) => {
        const choose = () => { S.palette = [...pal]; S.activeSugg = i; S.palSel = 0; S.ran = false; renderPage(); updateStatusInfo(); };
        sug.append(h("div", {
          class: `prow${i === S.sugg.auto_index ? " rec" : ""}${i === S.activeSugg ? " active" : ""}`,
          title: i === S.sugg.auto_index ? "Recommended palette" : `${pal.length} colors`,
          onclick: choose,
          ondblclick: () => { choose(); nav("next"); },
        }, pal.map((c) => h("i", { style: `background:${c}` }))));
      });
    } else {
      sug.append(h("div", { class: "empty" }, "Finding palettes…"));
    }
    const cur = S.palette[S.palSel];
    const custom = h("div", { class: "custom-pal" }, S.palette.map((c, i) => h("i", {
      class: i === S.palSel ? "sel" : "", style: `background:${c}`, title: c,
      onclick: () => { S.palSel = i; renderPage(); },
    })));
    return [
      group("Suggested Palettes", sug,
        h("div", { class: "row" },
          h("span", { class: "hint" }, "Double click to choose."), h("span", { class: "spacer" }),
          h("button", { class: "small-btn", disabled: S.maxK >= 40, onclick: () => { S.maxK += 1; loadSuggestions(); } }, "Add Additional Palette"))),
      group("Custom Palette",
        h("div", { class: "custom-wrap" }, custom,
          h("div", { class: "pal-tools" },
            h("button", { title: "Add a color (then click in the image to set it)", onclick: () => { S.palette.push(cur || "#ffffff"); S.palSel = S.palette.length - 1; S.activeSugg = -1; renderPage(); } }, "+"),
            h("button", { title: "Remove the selected color", disabled: S.palette.length <= 1, onclick: () => { S.palette.splice(S.palSel, 1); S.palSel = Math.max(0, S.palSel - 1); S.activeSugg = -1; renderPage(); } }, "✕"))),
        cur ? h("div", { class: "pal-detail" }, h("span", {}, "Selected:"),
          h("span", { class: "muted" }, cur),
          h("input", { type: "color", value: cur, title: "Edit the selected color", onchange: (e) => { S.palette[S.palSel] = e.target.value; S.activeSugg = -1; renderPage(); } })) : null,
        h("p", { class: "hint", style: "margin-top:6px" }, "Select a color above to change it. Click in the image to set the color (Ctrl-click to blend).")),
    ];
  },
  canNext: () => S.palette.length > 0,
  next: async () => ((await runVectorize()) ? (S.prefs.skipReview ? "export" : "review") : null),
};

PAGES.review = {
  title: "Review Result",
  viewer: "result",
  render: () => {
    const rerun = async () => { await runVectorize(); renderPage(); };
    const detail = ["high", "medium", "low"].map((q) => h("button", {
      class: `tbtn${S.quality === q ? " on" : ""}`,
      onclick: async () => { S.quality = q; if (S.mode === "auto") S.mode = "basic"; await rerun(); },
    }, Q_LABEL[q]));
    const unlimited = S.colorMode === "unlimited" || (S.mode === "auto" && !S.palette.length);
    return [
      card({ icon: ICON.done, title: "Done Reviewing", bullets: ["Click here to continue.", "Skip this page next time with the box below."], selected: true, onclick: () => nav("next") }),
      group("Improve Result",
        h("div", { class: "toggle-col" }, h("h4", {}, "Detail Level"), detail),
        h("hr", { class: "sep" }),
        h("div", { class: "toggle-col" }, h("h4", {}, "Colors"),
          h("button", { class: `tbtn${unlimited ? " on" : ""}`, onclick: async () => { S.mode = S.mode === "auto" ? "basic" : S.mode; S.colorMode = "unlimited"; S.palette = []; await rerun(); } }, "Unlimited Colors"),
          h("button", { class: `tbtn${!unlimited ? " on" : ""}`, onclick: () => { S.mode = S.mode === "auto" ? "basic" : S.mode; S.colorMode = "custom"; goTo("palette"); } }, "Custom Colors"),
          h("div", { class: "palette-line" }, "Current Palette (", h("a", { class: "link", onclick: () => { if (S.mode === "auto") S.mode = "basic"; if (S.colorMode === "unlimited") S.colorMode = "custom"; if (!S.palette.length && S.view) S.palette = S.view.palette.filter((p) => !p.source_alpha).map((p) => p.hex); goTo("palette"); } }, "change"), "):"),
          miniPalette(S.view ? S.view.palette.filter((p) => !p.transparent).map((p) => p.hex) : [])),
        h("hr", { class: "sep" }),
        h("div", { class: "toggle-col" }, h("h4", {}, "Advanced"),
          h("button", { class: "tbtn", onclick: () => goTo("removeBg") }, "✂ Remove Background"),
          h("button", { class: "tbtn", onclick: () => goTo("editSeg") }, "✏ Edit Result"),
          h("button", { class: "tbtn", onclick: () => { S.mode = "basic"; goTo("imageType"); } }, "↺ Hand-pick Settings"))),
      checkbox("Skip this page next time", S.prefs.skipReview, (v) => { S.prefs.skipReview = v; store.set("prefs", S.prefs); }),
    ];
  },
  next: () => "export",
};

PAGES.editSeg = {
  title: "Edit Segmentation",
  viewer: "segments",
  tools: "edit",
  render: () => [
    h("div", { class: "edit-text" },
      h("p", {}, "Fix mistakes in the ", h("b", {}, "segmentation"), " — the rough split of the image into flat-colored regions that are then smoothed into vector shapes."),
      h("p", {}, "Flip between ", h("b", {}, "Bitmap (B)"), ", ", h("b", {}, "Segments (S)"), " and ", h("b", {}, "Vector (V)"), " to spot errors."),
      h("p", {}, h("b", {}, "Pinching:"), " when a thin part of a shape is only connected diagonally, the result can break or bridge. Paint pixels with the ", h("b", {}, "Pencil"), " so the region has a clear path."),
      h("p", {}, h("b", {}, "Anti-aliasing leftovers:"), " thin slivers of in-between color. The ", h("b", {}, "Zap"), " tool dissolves the pixels you drag over (or the region you click) into the neighbouring regions."),
      h("p", {}, h("b", {}, "Fill"), " recolors a whole region; ", h("b", {}, "Color"), " picks the current color from a region. Hold ", h("b", {}, "Shift"), " to pan with any tool.")),
    group("Problem Finder",
      h("p", {}, "Highlights pinch points in the segmentation."),
      h("div", { class: "row" },
        h("button", { onclick: () => findProblems() }, "Find Problems"),
        h("span", { class: "muted" }, S.problems.length ? `${S.probIdx >= 0 ? S.probIdx + 1 + " / " : ""}${S.problems.length} found` : ""),
        h("span", { class: "spacer" }),
        h("button", { class: "small-btn", disabled: !S.problems.length, onclick: () => { gotoProblem(-1); renderPage(); } }, "◀"),
        h("button", { class: "small-btn", disabled: !S.problems.length, onclick: () => { gotoProblem(1); renderPage(); } }, "▶"))),
    group("Auto-Update",
      checkbox("Auto-Update Vectorization", S.autoEdit, (v) => { S.autoEdit = v; renderPage(); }),
      S.autoEdit ? null : h("button", { onclick: async () => { const v = await busy("Updating…", () => invoke("update")); if (v) setView(v, true); } }, "Update")),
    h("p", { class: "hint" }, "Note: edits are lost if you vectorize the image again with different settings."),
  ],
  next: () => (S.mode === "advanced" ? "advSmoothing" : "review"),
};

PAGES.removeBg = {
  title: "Remove Background",
  viewer: "result",
  tools: "erase",
  render: () => [
    h("div", { class: "edit-text" },
      h("p", {}, "Remove background shapes from the result. ", h("b", {}, "Removed shapes are not exported.")),
      h("p", {}, "With ", h("b", {}, "Erase (D)"), ", click the shapes to remove. Removed shapes are outlined with a red dotted line; click one again to restore it."),
      h("p", {}, h("b", {}, "Pan (A)"), " moves the view; hold ", h("b", {}, "Shift"), " to pan temporarily."),
      h("p", {}, "Undo, Redo and Revert correct mistakes or restore everything.")),
    group("Options",
      checkbox("Erase all shapes of the clicked color", S.wholeColor, (v) => { S.wholeColor = v; })),
    h("p", { class: "hint" }, "Note: removals are lost if you vectorize the image again with different settings."),
  ],
  next: () => "export",
};

PAGES.advSegmentation = {
  title: "Image Segmentation",
  viewer: "segments",
  onEnter: () => { if (!S.ran) runVectorize("Segmenting…"); },
  render: () => {
    const a = S.adv;
    const changed = () => { S.ran = false; updateStatusInfo(); if (a.autoSeg) debounced(() => runVectorize("Segmenting…")); };
    return [
      h("p", { class: "edit-text" }, "The segmentation is the rough split of the image into flat-colored pieces that are then smoothed into the final vector art."),
      group("Complexity",
        slider({ min: 1, max: 12, value: a.complexity, left: "Fewer Segments", right: "More Segments", onchange: (v) => { a.complexity = v; changed(); } })),
      group("Minimum Number of Pixels",
        h("p", {}, "Regions smaller than this are merged into their neighbours. 0 = automatic."),
        h("div", { class: "field-inline" }, "Minimum Number of Pixels:",
          h("input", { type: "number", min: 0, max: 100000, value: a.minPixels, onchange: (e) => { a.minPixels = Math.max(0, Number(e.target.value) || 0); changed(); } }), "px")),
      group("Anti-Aliasing Artifact Rejection",
        h("p", {}, "Anti-aliasing can leave thin unwanted segments between shapes. Artifact rejection removes them."),
        radio("aar", [["none", "None"], ["moderate", "Moderate"], ["aggressive", "Aggressive"]], a.aaRejection, (v) => { a.aaRejection = v; changed(); })),
      group("Noise Filter",
        h("p", {}, "Smooths out JPEG artifacts and sensor noise before segmenting."),
        radio("dn", [["", "Automatic"], ["on", "On"], ["off", "Off"]], a.denoise, (v) => { a.denoise = v; changed(); })),
      group("Auto-Update",
        checkbox("Auto-Update Segmentation", a.autoSeg, (v) => { a.autoSeg = v; renderPage(); }),
        a.autoSeg ? null : h("button", { onclick: () => runVectorize("Segmenting…") }, "Update")),
    ];
  },
  next: async () => (S.ran || (await runVectorize("Segmenting…")) ? "editSeg" : null),
};

PAGES.advSmoothing = {
  title: "Path Smoothing",
  viewer: "result",
  render: () => {
    const a = S.adv;
    const changed = () => { if (a.autoVec) debounced(() => refit()); };
    return [
      group("Smoothness",
        slider({ min: 1, max: 12, value: a.smoothness, left: "Less Smoothing", right: "More Smoothing", onchange: (v) => { a.smoothness = v; changed(); } })),
      group("Detect Sharp Corners",
        h("p", {}, "Gives crisper results for shapes with sharp corners."),
        checkbox("Detect Sharp Corners", a.corners, (v) => { a.corners = v; changed(); })),
      group("Anti-Aliasing Engine",
        h("p", {}, "Use the blended edge pixels to place edges with sub-pixel precision. Turn off for pixel-exact (aliased) art. Changing this segments the image again."),
        checkbox("Use Anti-Aliasing Engine", a.aaEngine, async (v) => { a.aaEngine = v; await runVectorize("Segmenting…"); })),
      group("Number of Nodes",
        slider({ min: 1, max: 12, value: a.nodes, left: "Fewer Nodes", right: "More Nodes", onchange: (v) => { a.nodes = v; changed(); } })),
      group("Curve Types",
        h("p", {}, "Lines only produces polygons, e.g. for some CNC machines and plotters."),
        radio("curves", [["curves", "Curves and lines"], ["lines", "Lines only"]], a.curves, (v) => { a.curves = v; changed(); })),
      group("Auto-Update",
        checkbox("Auto-Update Vectorization", a.autoVec, (v) => { a.autoVec = v; renderPage(); }),
        a.autoVec ? null : h("button", { onclick: () => refit() }, "Update")),
    ];
  },
  next: () => "removeBg",
};

const EXPORT_FORMATS = [["svg", ".svg"], ["eps", ".eps"], ["pdf", ".pdf"], ["ai", ".ai"], ["dxf", ".dxf"], ["png", ".png"]];
const BITMAP_FORMATS = [["png", "PNG"], ["gif", "GIF"], ["jpg", "JPEG"], ["bmp", "BMP"], ["tif", "TIFF"]];
const baseName = () => (S.info && S.info.name ? S.info.name.replace(/\.[^.]+$/, "") : "vector");
const saveExp = () => store.set("export", S.exp);

async function saveAs(ext = S.exp.format) {
  if (!S.view) return;
  const path = await invoke("pick_save", { defaultName: `${baseName()}.${ext}`, ext });
  if (!path) return;
  const saved = await busy("Saving…", () => invoke("export", { path, options: exportOptions() }));
  if (saved) setStatus(`Saved: ${saved}`);
}

PAGES.export = {
  title: "Export Result",
  viewer: "result",
  render: () => [
    card({ icon: ICON.save, title: "Save As", bullets: ["Choose the file type in the save dialog.", "Shortcut: Ctrl-S."], onclick: () => saveAs() }),
    group("Open in Another Program",
      h("div", { class: "row" },
        selectBox(EXPORT_FORMATS, S.exp.format, (v) => { S.exp.format = v; saveExp(); }),
        h("button", { title: "Save a temporary copy and open it with the program associated with this file type", onclick: async () => {
          const p = await busy("Opening…", () => invoke("open_external", { format: S.exp.format, options: exportOptions() }));
          if (p) setStatus(`Opened ${p}`);
        } }, "Open"),
        h("button", { title: "Copy the SVG markup; paste it into Inkscape, Illustrator, Figma…", onclick: async () => {
          try {
            const text = await invoke("export_svg_text", { options: exportOptions() });
            await navigator.clipboard.writeText(text);
            setStatus(`SVG copied to the clipboard (${Math.round(text.length / 1024)} KB).`);
          } catch (e) { setStatus(`Could not copy: ${e}`, true); }
        } }, "Copy SVG"))),
    h("button", { class: "expander", title: "Save next to the original image, in the format selected above", disabled: !S.info?.path, onclick: async () => {
      const p = await busy("Saving…", () => invoke("quick_save", { format: S.exp.format, options: exportOptions() }));
      if (p) setStatus(`Saved: ${p}`);
    } }, "Quick Save"),
    expander("bitmapExport", "Bitmap Export", () => [
      h("div", { class: "optgrid" },
        "Format:", selectBox(BITMAP_FORMATS, S.exp.bitmapFormat, (v) => { S.exp.bitmapFormat = v; saveExp(); }),
        "Scale:", h("div", { class: "field-inline" }, h("input", { type: "number", min: 0.1, max: 16, step: 0.5, value: S.exp.bitmapScale, onchange: (e) => { S.exp.bitmapScale = Number(e.target.value) || 1; saveExp(); } }), "× original size")),
      h("button", { class: "item", onclick: () => saveAs(S.exp.bitmapFormat) }, "Save Bitmap…"),
    ]),
    expander("exportOptions", "Export Options", () => h("div", { class: "optgrid" },
      "Shape mode:", selectBox([["stack", "Stack shapes on top of each other"], ["cutout", "Place shapes in cut-outs in shapes below"], ["cutout_group", "Place shapes in cut-outs and group by color"]], S.exp.shape, (v) => { S.exp.shape = v; saveExp(); }),
      "Stroking mode:", selectBox([["stroke", "Stroke shape boundaries"], ["none", "Do not stroke shape boundaries"]], S.exp.stroke, (v) => { S.exp.stroke = v; saveExp(); }),
      "DXF mode:", selectBox([["splines", "Lines and spline curves"], ["fewer", "Lines only, convert splines to fewer lines (smaller file)"], ["more", "Lines only, convert splines to more lines (larger file)"]], S.exp.dxf, (v) => { S.exp.dxf = v; saveExp(); }))),
    expander("saveSettings", "Save Settings", () => {
      const nameInput = h("input", { type: "text", value: `Settings ${store.get("savedSettings", []).length + 1}` });
      const list = store.get("savedSettings", []);
      return [
        h("div", { class: "field-inline" }, "Name:", nameInput, h("button", { class: "small-btn", onclick: () => {
          const name = nameInput.value.trim() || "Settings";
          const all = store.get("savedSettings", []).filter((s) => s.name !== name);
          all.unshift(snapshotSettings(name));
          store.set("savedSettings", all);
          setStatus(`Settings "${name}" saved. Find them under Wizard Mode → Saved Advanced Settings.`);
          renderPage();
        } }, "Save")),
        h("div", { class: "saved-list" }, list.map((s) => h("div", { class: "sitem" },
          h("span", {}, s.name), h("span", { class: "muted" }, s.mode),
          h("button", { class: "small-btn", title: "Delete", onclick: () => { store.set("savedSettings", list.filter((x) => x !== s)); renderPage(); } }, "✕")))),
      ];
    }),
  ],
  canNext: () => false,
};

// ---------------- batch
PAGES.batchFiles = {
  title: "Batch Processing",
  render: () => {
    const b = S.batch;
    const list = h("div", { class: "filelist" }, b.files.map((f, i) => h("div", {
      class: i === b.sel ? "sel" : "", title: f, onclick: () => { b.sel = i; renderPage(); },
    }, fileOf(f))));
    const addFrom = async (dir) => {
      const paths = await invoke("pick_images", { multiple: true, directory: dir || null });
      for (const p of paths) if (!b.files.includes(p)) b.files.push(p);
      if (paths.length) pushRecent("recentFolders", dirOf(paths[0]));
      renderPage();
    };
    return [
      card({ icon: ICON.addImages, title: "Add Images", bullets: ["Tip: select more than one file at a time.", "You can also drop files onto the window."], onclick: () => addFrom() }),
      expander("batchRecent", "Recent Folders", () => {
        const dirs = store.get("recentFolders", []);
        return dirs.length ? dirs.map((d) => h("button", { class: "item", title: d, onclick: () => addFrom(d) }, d)) : h("div", { class: "empty" }, "No recent folders.");
      }),
      group("Files to process", list,
        h("div", { class: "row" },
          h("button", { class: "small-btn", onclick: () => { b.files = []; b.sel = -1; renderPage(); } }, "Clear all"),
          h("button", { class: "small-btn", disabled: b.sel < 0, onclick: () => { b.files.splice(b.sel, 1); b.sel = -1; renderPage(); } }, "Remove selected"),
          h("span", { class: "muted" }, `${b.files.length} images`))),
    ];
  },
  canNext: () => S.batch.files.length > 0,
  next: () => "batchRun",
};

PAGES.batchRun = {
  title: "Batch Output",
  render: () => {
    const b = S.batch;
    const fmt = (v) => checkbox(v.toUpperCase(), b.formats.includes(v), (on) => {
      b.formats = on ? [...new Set([...b.formats, v])] : b.formats.filter((x) => x !== v);
      store.set("batchFormats", b.formats);
    });
    const progress = h("progress", { id: "batchProgress", value: b.log.length, max: Math.max(1, b.files.length) });
    return [
      group("Settings",
        radio("bs", [["auto", "Fully automatic (detect each image's type)"], ["current", `Current settings: ${S.info ? summary() : "Basic defaults"}`]], b.auto ? "auto" : "current", (v) => { b.auto = v === "auto"; })),
      group("Formats", h("div", { class: "row" }, ["svg", "pdf", "eps", "ai", "dxf", "png"].map(fmt))),
      group("Output Folder",
        radio("bo", [["same", "Same folder as each image"], ["choose", b.out ? `Folder: ${b.out}` : "Choose a folder…"]], b.outMode, async (v) => {
          b.outMode = v;
          if (v === "choose") {
            const dir = await invoke("pick_folder");
            if (dir) b.out = dir; else b.outMode = "same";
            renderPage();
          }
        })),
      h("div", { class: "row" },
        h("button", { class: "primary", disabled: b.running || !b.files.length, onclick: runBatch }, "Start"),
        h("button", { disabled: !b.running, onclick: () => invoke("cancel_batch") }, "Stop")),
      progress,
      h("ol", { class: "log" }, b.log.map((l) => h("li", { class: l.ok ? "good" : "bad" }, `${fileOf(l.file)}: ${l.message}`))),
    ];
  },
  canNext: () => false,
};

async function runBatch() {
  const b = S.batch;
  if (!b.formats.length) return setStatus("Choose at least one format.", true);
  b.running = true;
  b.log = [];
  renderPage();
  const unlisten = await TAURI.event.listen("batch-progress", ({ payload: p }) => {
    b.log.push(p);
    document.title = `Vector Free — batch ${Math.round((100 * p.index) / p.total)}%`;
    if (S.page === "batchRun") renderPage();
  });
  try {
    // Fully automatic: default settings, the type is detected per image.
    const auto = { kind: "art_blended", quality: "medium", num_colors: null, palette: [], remove_background: false, advanced: {} };
    const settings = b.auto || !S.info ? auto : buildSettings();
    const ok = await invoke("batch", {
      paths: b.files, outDir: b.outMode === "same" ? "" : b.out, formats: b.formats, settings, options: exportOptions(), autoKind: b.auto,
    });
    setStatus(`Batch finished: ${ok} of ${b.files.length} images.`);
  } catch (e) {
    setStatus(String(e), true);
  } finally {
    unlisten();
    b.running = false;
    document.title = S.info ? `${S.info.name} - Vector Free` : "Vector Free";
    renderPage();
  }
}

// ---------------------------------------------------------------- navigation
function goTo(page, resetHistory = false) {
  if (resetHistory) S.history = page === "load" ? [] : ["load"];
  else if (page !== S.page) S.history.push(S.page);
  S.page = page;
  enterPage();
}

async function nav(dir) {
  const page = PAGES[S.page];
  if (dir === "back") {
    if (!S.history.length) return;
    S.page = S.history.pop();
    return enterPage();
  }
  if (dir === "start") {
    if (S.page === "load") return resetAll();
    S.history = [];
    S.page = "load";
    return enterPage();
  }
  if (page.canNext && !page.canNext()) return;
  const target = await page.next();
  if (target) goTo(target);
}

function resetAll() {
  S.mode = "basic";
  store.set("mode", "basic");
  S.adv = { ...ADV_DEFAULTS };
  S.palette = [];
  S.sugg = null;
  S.ran = false;
  if (S.info) {
    S.kind = S.info.suggested_kind;
    S.colorMode = S.kind === "photo" ? "unlimited" : "custom";
  }
  setStatus("Settings reset to their defaults.");
  renderPage();
  updateStatusInfo();
}

function enterPage() {
  const page = PAGES[S.page];
  if (S.page !== "editSeg") clearProblems();
  if (page.onEnter) page.onEnter();
  if (page.viewer && S.view) setViewMode(page.viewer);
  else if (!page.viewer && S.viewMode !== "original" && ["imageType", "quality", "colorMode", "palette", "wizardMode", "load"].includes(S.page)) setViewMode("original");
  showTools(page.tools && S.view ? page.tools : null);
  if (!page.tools && S.tool !== "crop") setTool("pan");
  const batchPage = S.page.startsWith("batch");
  $("drop").hidden = !!S.info && !batchPage;
  $("dropText").innerHTML = batchPage ? "Drag and drop<br>your images here" : "Drag and drop<br>your image here";
  for (const p of Object.values(panes)) p.el.style.visibility = batchPage ? "hidden" : "";
  $("navFloat").hidden = $("viewFloat").hidden = !S.info || batchPage;
  renderPage();
  if (S.helpTopic !== null && !$("helpPane").hidden) showHelp(S.page);
}

function renderPage() {
  const page = PAGES[S.page];
  $("pageTitle").textContent = page.title;
  const body = $("wizBody");
  const scroll = body.scrollTop;
  body.innerHTML = "";
  body.append(...[page.render()].flat(3).filter(Boolean));
  body.scrollTop = scroll;
  $("navStartLabel").textContent = S.page === "load" ? "Reset" : "Start Page";
  $("navBack").disabled = !S.history.length;
  $("navNext").disabled = page.canNext ? !page.canNext() : false;
}

$("navStart").addEventListener("click", () => nav("start"));
$("navBack").addEventListener("click", () => nav("back"));
$("navNext").addEventListener("click", () => nav("next"));

// ---------------------------------------------------------------- More Options menu
function openMenu(anchor) {
  const m = $("moreMenu");
  const r = anchor.getBoundingClientRect();
  m.hidden = false;
  m.style.left = `${Math.min(r.left, window.innerWidth - m.offsetWidth - 8)}px`;
  m.style.top = `${r.bottom + 2}px`;
  setTimeout(() => document.addEventListener("click", closeMenu, { once: true }), 0);
}
function closeMenu() { $("moreMenu").hidden = true; }
for (const b of document.querySelectorAll("#moreMenu button")) {
  b.addEventListener("click", () => {
    closeMenu();
    const a = b.dataset.menu;
    if (a === "about") $("aboutDlg").showModal();
    if (a === "shortcuts") { $("helpPane").hidden = false; $("helpBtn").classList.add("on"); showHelp("shortcuts"); }
    if (a === "restore") {
      for (const k of ["mode", "export", "prefs", "recentBasic", "batchFormats"]) store.set(k, null);
      S.exp = { ...EXPORT_DEFAULTS };
      S.prefs = { skipReview: false };
      resetAll();
    }
  });
}

// ---------------------------------------------------------------- contextual help
const HELP = {
  load: ["Load Image", `<p>Start by loading the bitmap you want to vectorize.</p>
    <h3>Import from Folder</h3><p>Opens a file dialog. PNG, JPG, GIF, BMP, TIFF, WebP and ICO are supported. Shortcut <kbd>Ctrl</kbd>+<kbd>O</kbd>.</p>
    <h3>Paste from Clipboard</h3><p>Copy an image in another program (for example a screenshot) and press <kbd>Ctrl</kbd>+<kbd>V</kbd>.</p>
    <h3>Drag and drop</h3><p>Drop an image file onto the window, or onto the program icon.</p>
    <h3>Batch Processing</h3><p>Vectorize many images with the same settings.</p>
    <h3>Recent Folders / Recent Images</h3><p>Quick access to what you opened before.</p>`],
  wizardMode: ["Wizard Mode", `<p>Choose how much control you want.</p>
    <h3>Fully Automatic</h3><p>Vector Free detects the image type, quality and colors and vectorizes right away. You can still improve the result on the Review page.</p>
    <h3>Basic</h3><p>Answer a few questions about the image: type, quality and colors. Recommended for most images.</p>
    <h3>Advanced</h3><p>Direct control over segmentation and path smoothing.</p>
    <h3>Recent Basic / Saved Advanced Settings</h3><p>Reuse settings from earlier sessions. Settings are saved from the Export Result page.</p>
    <h3>Crop Image</h3><p>Trace only part of the image.</p>`],
  imageType: ["Image Type", `<p>Tell Vector Free what kind of picture this is; it changes how edges are interpreted.</p>
    <h3>Photograph</h3><p>Taken with a camera, or detailed artwork with soft gradients.</p>
    <h3>Artwork with Blended Edges</h3><p>Logos and drawings whose edge pixels mix the two neighbouring colors (anti-aliasing). Vector Free uses those mixed pixels to place edges with sub-pixel precision.</p>
    <h3>Artwork without Blended Edges</h3><p>Pixel art and aliased graphics with hard pixel steps. Zoom in on an edge: if you see only the two flat colors, choose this.</p>`],
  quality: ["Input Quality / Detail Level", `<p>For artwork: how clean the source is (not its resolution). Higher quality keeps finer detail; lower quality rejects more noise and blur.</p>
    <p>For photographs: how much detail you want in the result. Low gives fewer, smoother shapes; High gives a faithful but larger result.</p>`],
  colorMode: ["Color Mode", `<h3>Two Colors</h3><p>Only two colors, e.g. black and white.</p>
    <h3>Custom Colors</h3><p>You review a suggested palette and the result uses only those colors. Best for logos with up to about 12 colors.</p>
    <h3>Unlimited Colors</h3><p>Uses as many colors as needed. Good for gradients and photos.</p>`],
  palette: ["Palette Selection", `<h3>Suggested Palettes</h3><p>Palettes with an increasing number of colors. The recommended one is marked with ▶▶. Click (or double click) to choose; <b>Add Additional Palette</b> offers one with more colors.</p>
    <h3>Custom Palette</h3><p>Select a color, then click in the image to replace it with the color under the cursor. <kbd>Ctrl</kbd>-click blends the clicked color with the current one. <b>+</b> adds a color, <b>✕</b> removes the selected one.</p>`],
  review: ["Review Result", `<p>Check the result: flip between <b>Bitmap (B)</b> and <b>Vector (V)</b>, zoom with the mouse wheel and tick <b>Paths</b> to see the outlines and nodes.</p>
    <h3>Improve Result</h3><p><b>Detail Level</b> and <b>Colors</b> re-run the vectorization. <b>Current Palette (change)</b> opens the palette page.</p>
    <h3>Advanced</h3><p><b>Remove Background</b> deletes shapes, <b>Edit Result</b> fixes the segmentation by hand and <b>Hand-pick Settings</b> goes back to the questions.</p>`],
  editSeg: ["Edit Segmentation", `<p>Tools (keyboard shortcut in brackets):</p>
    <p><b>Undo (Z)</b>, <b>Redo (Y)</b>, <b>Revert (T)</b> — step through or discard your edits.</p>
    <p><b>Pan (A)</b> — move the view. Hold <kbd>Shift</kbd> to pan with any tool.</p>
    <p><b>Zap (D)</b> — drag to dissolve pixels into the surrounding regions; click to dissolve a whole region.</p>
    <p><b>Fill (F)</b> — give a whole region the current color.</p>
    <p><b>Pencil (X)</b> — paint pixels with the current color. The size slider sets the brush.</p>
    <p><b>Color (C)</b> — click a region to make its color the current color. Clicking the color swatch cycles through the palette.</p>
    <h3>Problem Finder</h3><p>Marks pinch points; use ◀ ▶ to visit them.</p>`],
  removeBg: ["Remove Background", `<p><b>Erase (D)</b>: click shapes to remove them from the result; click again to restore. Removed shapes show a red dotted outline and are not exported.</p>
    <p>Tick <b>Erase all shapes of the clicked color</b> to remove a color everywhere at once.</p>
    <p><b>Pan (A)</b> or hold <kbd>Shift</kbd> to move the view.</p>`],
  advSegmentation: ["Image Segmentation", `<h3>Complexity</h3><p>How many color segments the image is split into.</p>
    <h3>Minimum Number of Pixels</h3><p>Regions smaller than this are merged into a neighbour. 0 lets Vector Free decide.</p>
    <h3>Anti-Aliasing Artifact Rejection</h3><p>Removes the thin in-between segments anti-aliasing can leave along edges.</p>
    <h3>Noise Filter</h3><p>Pre-filters JPEG artifacts and noise.</p>
    <h3>Auto-Update</h3><p>Re-segment automatically after every change.</p>`],
  advSmoothing: ["Path Smoothing", `<h3>Smoothness</h3><p>How strongly the pixel staircase is smoothed before curves are fitted.</p>
    <h3>Detect Sharp Corners</h3><p>Keeps corners crisp; turn off for organic shapes.</p>
    <h3>Anti-Aliasing Engine</h3><p>Uses blended edge pixels to locate edges between pixels.</p>
    <h3>Number of Nodes</h3><p>Fewer nodes give simpler, smoother curves; more nodes follow the shapes more closely.</p>
    <h3>Curve Types</h3><p>Lines only outputs polygons.</p>`],
  export: ["Export Result", `<h3>Save As</h3><p>Pick the file type in the save dialog: SVG, EPS, PDF, AI, DXF or a bitmap format. Shortcut <kbd>Ctrl</kbd>+<kbd>S</kbd>.</p>
    <h3>Open in Another Program</h3><p><b>Open</b> saves a temporary copy and opens it in the program Windows associates with that type. <b>Copy SVG</b> puts the vector on the clipboard.</p>
    <h3>Quick Save</h3><p>Saves next to the original image, with the same name and the type selected above.</p>
    <h3>Bitmap Export</h3><p>Renders the vectors to PNG, GIF, JPEG, BMP or TIFF at any scale.</p>
    <h3>Export Options</h3><p><b>Shape mode</b>: stacked shapes are easier to edit; cut-outs never overlap. <b>Stroking mode</b>: stroking hides hairline gaps some viewers draw between shapes. <b>DXF mode</b>: splines, or lines only for programs that cannot read splines.</p>
    <h3>Save Settings</h3><p>Store the current settings under a name to reuse them from the Wizard Mode page.</p>`],
  batchFiles: ["Batch Processing", `<p>Add the images to process with <b>Add Images</b>, a recent folder, or by dropping files onto the window. <b>Remove selected</b> and <b>Clear all</b> edit the list.</p>`],
  batchRun: ["Batch Output", `<p>Choose whether each image is analysed automatically or the current settings are used for all of them, the output formats and where files are written, then press <b>Start</b>. <b>Stop</b> finishes the current image and stops.</p>`],
  shortcuts: ["Keyboard Shortcuts", `<p><kbd>Ctrl</kbd>+<kbd>O</kbd> open · <kbd>Ctrl</kbd>+<kbd>V</kbd> paste · <kbd>Ctrl</kbd>+<kbd>S</kbd> save as</p>
    <p><kbd>B</kbd> bitmap · <kbd>S</kbd> segments · <kbd>V</kbd> vector · <kbd>P</kbd> paths</p>
    <p><kbd>0</kbd> fit · <kbd>1</kbd> actual size · <kbd>+</kbd>/<kbd>-</kbd> zoom · mouse wheel zooms</p>
    <p>Editing pages: <kbd>Z</kbd> undo · <kbd>Y</kbd> redo · <kbd>T</kbd> revert · <kbd>A</kbd> pan · <kbd>D</kbd> zap / erase · <kbd>F</kbd> fill · <kbd>X</kbd> pencil · <kbd>C</kbd> color · hold <kbd>Shift</kbd> to pan</p>
    <p><kbd>F1</kbd> help · <kbd>Enter</kbd> next page · <kbd>Backspace</kbd> previous page</p>`],
};
const HELP_ORDER = ["load", "wizardMode", "imageType", "quality", "colorMode", "palette", "review", "editSeg", "removeBg", "advSegmentation", "advSmoothing", "export", "batchFiles", "batchRun", "shortcuts"];

function showHelp(topic, record = true) {
  if (record && S.helpTopic && S.helpTopic !== topic) S.helpHistory.push(S.helpTopic);
  S.helpTopic = topic;
  const body = $("helpBody");
  if (topic === "contents") {
    body.innerHTML = "<h2>Contents</h2>";
    for (const t of HELP_ORDER) body.append(h("p", {}, h("a", { onclick: () => showHelp(t) }, HELP[t][0])));
    return;
  }
  const [title, html] = HELP[topic] || HELP.load;
  body.innerHTML = `<h2>${title}</h2>${html}`;
}
function toggleHelp() {
  const pane = $("helpPane");
  pane.hidden = !pane.hidden;
  $("helpBtn").classList.toggle("on", !pane.hidden);
  if (!pane.hidden) showHelp(S.page);
  requestAnimationFrame(() => S.info && fit());
}
$("helpBtn").addEventListener("click", toggleHelp);
$("helpClose").addEventListener("click", toggleHelp);
$("helpContents").addEventListener("click", () => showHelp("contents"));
$("helpBack").addEventListener("click", () => { const t = S.helpHistory.pop(); if (t) showHelp(t, false); });
$("helpFwd").addEventListener("click", () => {
  const i = HELP_ORDER.indexOf(S.helpTopic);
  showHelp(HELP_ORDER[(i + 1) % HELP_ORDER.length]);
});

// ---------------------------------------------------------------- keyboard
const TOOL_KEYS = { a: "pan", d: "zap", f: "fill", x: "pencil", c: "color" };
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Shift") S.shift = true;
  if (ev.key === "F1") { ev.preventDefault(); toggleHelp(); return; }
  if (document.querySelector("dialog[open]") || ev.target.matches("input[type=number], input[type=text], select, textarea")) return;
  const ctrl = ev.ctrlKey || ev.metaKey;
  const k = ev.key.toLowerCase();
  const page = PAGES[S.page];
  if (ctrl && k === "o") { ev.preventDefault(); openDialog(); return; }
  if (ctrl && k === "s") { ev.preventDefault(); if (S.view) saveAs(); return; }
  if (ctrl && k === "z") { ev.preventDefault(); if (page.tools) history(ev.shiftKey ? "redo" : "undo"); return; }
  if (ctrl && k === "y") { ev.preventDefault(); if (page.tools) history("redo"); return; }
  if (ctrl) return;
  if (ev.key === " ") { S.space = true; ev.preventDefault(); return; }
  if (S.tool === "crop" && ev.key === "Escape") { endCrop(); setTool("pan"); return; }
  if (S.tool === "crop" && ev.key === "Enter" && S.crop) { applyCrop(); return; }
  if (ev.key === "Enter" && !ev.target.matches("button")) { nav("next"); return; }
  if (ev.key === "Backspace") { nav("back"); return; }
  if (page.tools) {
    if (k === "z") return history("undo");
    if (k === "y") return history("redo");
    if (k === "t") return history("revert");
    if (page.tools === "erase") {
      if (k === "a") return setTool("pan");
      if (k === "d") return setTool("erase");
    } else if (TOOL_KEYS[k]) return setTool(TOOL_KEYS[k]);
  }
  if (k === "b") setViewMode("original");
  else if (k === "s" && S.view) setViewMode("segments");
  else if (k === "v" && S.view) setViewMode("result");
  else if (k === "p" && S.view) { $("pathsCheck").checked = !$("pathsCheck").checked; $("pathsCheck").dispatchEvent(new Event("change")); }
  else if (k === "0") fit();
  else if (k === "1") document.querySelector('#navFloat [data-act="center"]').click();
  else if (ev.key === "+" || ev.key === "=") zoomCenter(1.25);
  else if (ev.key === "-") zoomCenter(1 / 1.25);
});
document.addEventListener("keyup", (ev) => {
  if (ev.key === " ") S.space = false;
  if (ev.key === "Shift") S.shift = false;
});

// ---------------------------------------------------------------- drag & drop
TAURI.webview.getCurrentWebview().onDragDropEvent((ev) => {
  const { type, paths } = ev.payload;
  if (type === "enter" || type === "over") $("drop").classList.add("over");
  if (type === "leave") $("drop").classList.remove("over");
  if (type === "drop") {
    $("drop").classList.remove("over");
    if (!paths || !paths.length) return;
    if (S.page.startsWith("batch") || paths.length > 1) {
      for (const p of paths) if (!S.batch.files.includes(p)) S.batch.files.push(p);
      if (S.page !== "batchFiles") goTo("batchFiles");
      else renderPage();
    } else {
      openPath(paths[0]);
    }
  }
});

// ---------------------------------------------------------------- start
enterPage();
invoke("startup_file").then((p) => p && openPath(p)).catch(() => {});
