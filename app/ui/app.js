// Vector Free UI. Talks to the Rust side through Tauri commands.
"use strict";

const TAURI = window.__TAURI__;
const invoke = TAURI.core.invoke;
const $ = (id) => document.getElementById(id);
const radios = (name) => document.querySelector(`input[name="${name}"]:checked`).value;
const SVGNS = "http://www.w3.org/2000/svg";

const KIND_LABEL = { photo: "Photograph", art_blended: "Artwork with blended edges", art_flat: "Artwork without blending" };
const QUALITY_TEXT = {
  photo: {
    title: "Detail level",
    low: "Fewer, smoother shapes. Makes a photo look like a drawing.",
    medium: "Usually the best balance.",
    high: "High fidelity, but larger files that are harder to edit.",
  },
  art: {
    title: "Quality",
    low: "Blurry or noisy source: rejects noise and smooths the result, may lose detail.",
    medium: "Fairly clean source with some degradation.",
    high: "Clean, sharp source: recovers fine detail but is sensitive to noise.",
  },
};
const MAX_NODE_MARKERS = 20000;

// ---------------------------------------------------------------- prefs
const prefs = { auto: true };
try {
  Object.assign(prefs, JSON.parse(localStorage.getItem("vf-prefs") || "{}"));
} catch { /* storage unavailable: defaults */ }
function savePrefs() {
  try { localStorage.setItem("vf-prefs", JSON.stringify(prefs)); } catch { /* ignore */ }
}

const state = {
  info: null,         // opened image: {name, width, height, png, suggested_kind, exact_colors}
  view: null,         // last vectorization result
  draft: null,        // edited palette (array of hex) waiting for "Apply palette"
  selected: 0,        // palette index used by pencil and fill
  stale: false,       // segmentation edited since the last vectorization
  z: 1, px: 0, py: 0, // shared zoom / pan for all panes
  tool: "pan",
  space: false,
  mode: "compare",
  problems: [],
  probIdx: -1,
  crop: null,         // {x0, y0, x1, y1} in source pixels while cropping
  suggestions: null,  // {palettes, auto_index}
  activeSuggestion: -1,
};

const panes = {};
for (const el of document.querySelectorAll(".pane")) {
  panes[el.dataset.pane] = { el, viewport: el.querySelector(".viewport"), stage: el.querySelector(".stage") };
}

// ---------------------------------------------------------------- utilities
function setStatus(text, isError = false) {
  const s = $("status");
  s.textContent = text;
  s.classList.toggle("error", isError);
}

async function busy(text, fn) {
  $("busyText").textContent = text;
  $("busy").hidden = false;
  document.title = `Vector Free — ${text}`;
  try {
    return await fn();
  } catch (e) {
    setStatus(String(e), true);
    return undefined;
  } finally {
    $("busy").hidden = true;
    document.title = "Vector Free";
  }
}

const isArt = () => radios("kind") !== "photo";

function advValue(id) {
  const el = $(id);
  return el.dataset.auto === "1" ? null : Number(el.value);
}

function settings() {
  const denoise = $("advDenoise").value;
  return {
    kind: radios("kind"),
    quality: radios("quality"),
    num_colors: radios("colorMode") === "count" ? Number($("numColors").value) : null,
    palette: [],
    remove_background: $("removeBg").checked,
    advanced: {
      tolerance: advValue("advTol"),
      smoothing: advValue("advSmooth"),
      corner_angle: advValue("advCorner"),
      min_area: advValue("advArea"),
      denoise: denoise === "" ? null : denoise === "on",
      curves: $("advCurves").value,
    },
  };
}

function exportOptions() {
  return {
    layout: radios("layout"),
    stroke: radios("render") === "stroke",
    stroke_width: Number($("expStrokeW").value) || 0.5,
    group_by_color: $("expGroup").checked,
    raster_scale: Number($("expScale").value) || 1,
    dxf_splines: radios("dxfMode") === "splines",
  };
}

function baseName() {
  return state.info && state.info.name ? state.info.name.replace(/\.[^.]+$/, "") : "vector";
}

// ---------------------------------------------------------------- viewing
function applyTransform() {
  for (const p of Object.values(panes)) {
    p.stage.style.transform = `translate(${state.px}px, ${state.py}px) scale(${state.z})`;
    p.stage.style.setProperty("--z", state.z);
  }
  $("zoomLabel").textContent = `${Math.round(state.z * 100)}%`;
}

function visibleViewport() {
  return Object.values(panes).find((p) => !p.el.hidden)?.viewport;
}

function fit() {
  if (!state.info) return;
  const vp = visibleViewport();
  if (!vp) return;
  const { width: w, height: h } = state.info;
  const r = vp.getBoundingClientRect();
  state.z = Math.max(0.02, Math.min(r.width / w, r.height / h) * 0.92);
  state.px = (r.width - w * state.z) / 2;
  state.py = (r.height - h * state.z) / 2;
  applyTransform();
}

function zoomAt(factor, mx, my) {
  const nz = Math.min(64, Math.max(0.02, state.z * factor));
  state.px = mx - (mx - state.px) * (nz / state.z);
  state.py = my - (my - state.py) * (nz / state.z);
  state.z = nz;
  applyTransform();
}

function zoomCenter(factor) {
  const vp = visibleViewport();
  if (!vp) return;
  const r = vp.getBoundingClientRect();
  zoomAt(factor, r.width / 2, r.height / 2);
}

function centerOn(x, y, minZoom) {
  const vp = visibleViewport();
  if (!vp) return;
  const r = vp.getBoundingClientRect();
  state.z = Math.max(state.z, minZoom);
  state.px = r.width / 2 - x * state.z;
  state.py = r.height / 2 - y * state.z;
  applyTransform();
}

function setViewMode(mode) {
  state.mode = mode;
  const show = {
    compare: ["original", "result"],
    overlay: ["result"],
    result: ["result"],
    outlines: ["result"],
    segments: ["segments"],
    original: ["original"],
  }[mode];
  for (const [name, p] of Object.entries(panes)) p.el.hidden = !show.includes(name);
  $("panes").classList.toggle("single", show.length === 1);
  $("overlayRow").hidden = mode !== "overlay";
  $("resultLabel").textContent = { overlay: "Vector over original", outlines: "Outlines and nodes" }[mode] || "Vector";
  renderResult();
  requestAnimationFrame(fit);
}

/** Overlay layer (pencil strokes, markers, crop rectangle) of a pane. */
function overlay(stage) {
  let o = stage.querySelector("svg.overlay");
  if (!o) {
    o = document.createElementNS(SVGNS, "svg");
    o.classList.add("overlay");
    stage.appendChild(o);
  }
  o.setAttribute("viewBox", `0 0 ${state.info.width} ${state.info.height}`);
  o.setAttribute("width", state.info.width);
  o.setAttribute("height", state.info.height);
  return o;
}

function makeImg(src, cls) {
  const img = new Image();
  img.src = src;
  img.width = state.info.width;
  img.height = state.info.height;
  img.draggable = false;
  if (cls) img.className = cls;
  return img;
}

function setImage(pane, src, cls) {
  const { stage } = panes[pane];
  stage.innerHTML = "";
  if (!src) return;
  stage.appendChild(makeImg(src, cls));
  overlay(stage);
  drawMarkers();
}

/** Draw node dots for the outline view. */
function nodeLayer(svg) {
  const layer = document.createElementNS(SVGNS, "svg");
  layer.classList.add("nodes");
  layer.setAttribute("viewBox", `0 0 ${state.info.width} ${state.info.height}`);
  layer.setAttribute("width", state.info.width);
  layer.setAttribute("height", state.info.height);
  let count = 0;
  const frag = document.createDocumentFragment();
  for (const path of svg.querySelectorAll("path")) {
    const d = path.getAttribute("d") || "";
    for (const m of d.matchAll(/([MLC])([^MLCZ]*)/g)) {
      const nums = m[2].trim().split(/[\s,]+/).map(Number);
      if (nums.length < 2) continue;
      const c = document.createElementNS(SVGNS, "circle");
      c.setAttribute("cx", nums[nums.length - 2]);
      c.setAttribute("cy", nums[nums.length - 1]);
      c.style.r = "calc(2.5px / var(--z))";
      frag.appendChild(c);
      if (++count > MAX_NODE_MARKERS) return null;
    }
  }
  layer.appendChild(frag);
  return layer;
}

function renderResult() {
  const { stage } = panes.result;
  stage.innerHTML = "";
  if (!state.view || !state.info) return;
  if (state.mode === "overlay") stage.appendChild(makeImg(state.info.png, "pixelated under"));
  stage.insertAdjacentHTML("beforeend", state.view.svg.replace(/^<\?xml[^>]*>\s*/, ""));
  const svg = stage.querySelector("svg:not(.overlay)");
  if (svg) {
    svg.classList.add("vector");
    svg.setAttribute("width", state.info.width);
    svg.setAttribute("height", state.info.height);
    if (state.mode === "overlay") {
      svg.style.position = "relative";
      svg.style.opacity = $("overlayOpacity").value;
    }
    if (state.mode === "outlines") {
      svg.classList.add("outlines");
      const nodes = nodeLayer(svg);
      if (nodes) stage.appendChild(nodes);
      else setStatus(`Too many nodes to mark individually (over ${MAX_NODE_MARKERS}); showing outlines only.`);
    }
  }
  overlay(stage);
  drawMarkers();
}

function setTool(tool) {
  if (tool !== "crop" && state.crop) endCrop();
  state.tool = tool;
  const r = document.querySelector(`input[name="tool"][value="${tool}"]`);
  if (r) r.checked = true;
  for (const p of Object.values(panes)) {
    p.viewport.classList.toggle("pencil", ["pencil", "zap", "fill", "crop"].includes(tool));
    p.viewport.classList.toggle("picker", tool === "picker");
  }
}

// ---------------------------------------------------------------- finder markers
function drawMarkers() {
  for (const name of ["result", "segments", "original"]) {
    const { stage } = panes[name];
    if (!stage.firstChild || !state.info) continue;
    const o = overlay(stage);
    o.querySelectorAll(".marker").forEach((m) => m.remove());
    state.problems.forEach((p, i) => {
      const c = document.createElementNS(SVGNS, "circle");
      c.setAttribute("cx", p.x);
      c.setAttribute("cy", p.y);
      c.style.r = "calc(9px / var(--z))";
      c.classList.add("marker");
      if (i === state.probIdx) c.classList.add("current");
      o.appendChild(c);
    });
  }
}

async function findProblems(quiet = false) {
  if (!state.view) return;
  try {
    state.problems = await invoke("find_problems");
  } catch (e) {
    return setStatus(String(e), true);
  }
  state.probIdx = -1;
  const n = state.problems.length;
  $("findCount").textContent = n ? `${n} found` : "none found";
  $("findPrev").disabled = $("findNext").disabled = n === 0;
  drawMarkers();
  if (!quiet) setStatus(n ? `${n} pinch points. Use › to visit them; fix with Pencil or Zap.` : "No pinch points found.");
}

function gotoProblem(step) {
  const n = state.problems.length;
  if (!n) return;
  state.probIdx = (state.probIdx + step + n) % n;
  const p = state.problems[state.probIdx];
  centerOn(p.x, p.y, 6);
  drawMarkers();
  $("findCount").textContent = `${state.probIdx + 1} / ${n}`;
}

// ---------------------------------------------------------------- palette UI
function renderPalette() {
  const ul = $("palette");
  ul.innerHTML = "";
  const v = state.view;
  if (!v) return;

  if (state.draft) {
    state.draft.forEach((hex, i) => {
      const li = document.createElement("li");
      li.className = "draft";
      li.innerHTML = `<input type="color" value="${hex}"><span class="hex">${hex}</span><span></span><span></span><button title="Remove">✕</button>`;
      li.querySelector("input").addEventListener("input", (e) => {
        state.draft[i] = e.target.value;
        li.querySelector(".hex").textContent = e.target.value;
      });
      li.querySelector("button").addEventListener("click", () => {
        state.draft.splice(i, 1);
        renderPalette();
      });
      ul.appendChild(li);
    });
    $("btnApplyPalette").disabled = state.draft.length === 0;
    return;
  }
  $("btnApplyPalette").disabled = true;

  v.palette.forEach((e, i) => {
    const li = document.createElement("li");
    li.classList.toggle("selected", i === state.selected);
    li.classList.toggle("transparent", e.transparent);
    const merges = v.palette
      .map((o, j) => (j === i ? "" : `<option value="m${j}">Merge into ${o.source_alpha ? "transparent" : o.hex}</option>`))
      .join("");
    const op = e.alpha < 255 ? ` <span class="op">${Math.round((e.alpha / 255) * 100)}%</span>` : "";
    li.innerHTML = `
      <input type="color" value="${e.hex}" title="Change color">
      <span class="hex">${e.source_alpha ? "transparent" : e.hex}${op}</span>
      <span class="pct">${e.percent.toFixed(1)}%</span>
      <button title="Toggle transparency">${e.transparent ? "Show" : "Hide"}</button>
      <select title="Opacity and merging"><option value="">More…</option>
        <optgroup label="Opacity"><option value="a255">100%</option><option value="a191">75%</option><option value="a128">50%</option><option value="a64">25%</option></optgroup>
        <optgroup label="Merge">${merges}</optgroup>
      </select>`;
    li.addEventListener("click", (ev) => {
      if (ev.target.closest("input,button,select")) return;
      state.selected = i;
      renderPalette();
    });
    li.querySelector("input").addEventListener("change", (ev) =>
      busy("Changing color…", async () => setView(await invoke("recolor", { index: i, color: ev.target.value }), true)));
    li.querySelector("button").addEventListener("click", () =>
      busy("Updating…", async () => setView(await invoke("set_transparent", { index: i, transparent: !e.transparent }), true)));
    li.querySelector("select").addEventListener("change", (ev) => {
      const v = ev.target.value;
      if (!v) return;
      if (v[0] === "a") {
        busy("Changing opacity…", async () => setView(await invoke("set_alpha", { index: i, alpha: Number(v.slice(1)) }), true));
      } else {
        busy("Merging colors…", async () => {
          state.selected = 0;
          setView(await invoke("merge_colors", { from: i, into: Number(v.slice(1)) }), true);
        });
      }
    });
    ul.appendChild(li);
  });
}

function startDraft() {
  if (!state.draft && state.view) {
    state.draft = state.view.palette.filter((e) => !e.source_alpha).map((e) => e.hex);
  }
}

// ---------------------------------------------------------------- suggested palettes
async function loadSuggestions() {
  if (!state.info) return;
  const ul = $("suggested");
  ul.innerHTML = "<li class='muted'>Computing palettes…</li>";
  try {
    state.suggestions = await invoke("suggested_palettes", { settings: settings() });
  } catch (e) {
    ul.innerHTML = "";
    return setStatus(String(e), true);
  }
  state.activeSuggestion = -1;
  renderSuggestions();
}

function renderSuggestions() {
  const ul = $("suggested");
  ul.innerHTML = "";
  if (!state.suggestions) return;
  state.suggestions.palettes.forEach((pal, i) => {
    const li = document.createElement("li");
    li.title = i === state.suggestions.auto_index ? "Most likely palette" : `${pal.length} colors`;
    li.classList.toggle("auto", i === state.suggestions.auto_index);
    li.classList.toggle("active", i === state.activeSuggestion);
    li.innerHTML = `<span class="n">${pal.length}</span><span class="sw">${pal.map((c) => `<i style="background:${c}"></i>`).join("")}</span>`;
    li.addEventListener("click", () => applySuggestion(i));
    ul.appendChild(li);
  });
}

async function applySuggestion(i) {
  state.activeSuggestion = i;
  renderSuggestions();
  const pal = state.suggestions.palettes[i];
  const t0 = performance.now();
  const v = await busy("Vectorizing…", () => invoke("resegment", { palette: pal, settings: settings() }));
  if (v) {
    setView(v);
    appendTiming(t0);
  }
}

// ---------------------------------------------------------------- results
function updateHistoryButtons(canUndo, canRedo) {
  $("btnUndo").disabled = !canUndo;
  $("btnRedo").disabled = !canRedo;
}

function setView(v, keepZoom = false) {
  state.view = v;
  state.draft = null;
  state.stale = false;
  if (state.selected >= v.palette.length) state.selected = 0;
  renderResult();
  setImage("segments", v.seg_png, "pixelated");
  renderPalette();
  updateHistoryButtons(v.can_undo, v.can_redo);
  $("paletteSection").hidden = false;
  $("editSection").hidden = false;
  $("btnExport").disabled = false;
  const visible = v.palette.filter((p) => !p.transparent).length;
  setStatus(`${visible} colors · ${v.paths} paths · ${v.nodes} nodes · ${state.info.width}×${state.info.height} px`);
  if (!keepZoom) requestAnimationFrame(fit);
  else applyTransform();
}

function appendTiming(t0) {
  setStatus($("status").textContent + ` · ${((performance.now() - t0) / 1000).toFixed(2)} s`);
}

function handleEdit(r) {
  if (r.view) {
    setView(r.view, true);
  } else {
    setImage("segments", r.seg_png, "pixelated");
    state.stale = true;
    setStatus("Segmentation edited. Click \"Update vectors\" (U) to see the result.");
  }
  updateHistoryButtons(r.can_undo, r.can_redo);
  if (state.problems.length) findProblems(true);
}

async function runEdit(op) {
  try {
    handleEdit(await invoke("edit", { op, revectorize: $("autoUpdate").checked }));
  } catch (e) {
    setStatus(String(e), true);
  }
}

function showImage(info) {
  state.info = info;
  state.view = null;
  state.draft = null;
  state.problems = [];
  state.probIdx = -1;
  state.suggestions = null;
  $("findCount").textContent = "";
  $("findPrev").disabled = $("findNext").disabled = true;
  $("fileName").textContent = `${info.name || "image"} — ${info.width}×${info.height}`;
  $("drop").hidden = true;
  $("btnProcess").disabled = false;
  $("btnCrop").disabled = false;
  $("btnExport").disabled = true;
  $("paletteSection").hidden = true;
  $("editSection").hidden = true;
  document.querySelector(`input[name="kind"][value="${info.suggested_kind}"]`).checked = true;
  updateQualityText();
  $("kindHint").textContent = `Suggested: ${KIND_LABEL[info.suggested_kind]} (${
    info.exact_colors > 20000 ? "more than 20,000" : info.exact_colors.toLocaleString()} distinct colors).`;
  setImage("original", info.png, "pixelated");
  panes.result.stage.innerHTML = "";
  panes.segments.stage.innerHTML = "";
  setViewMode(state.mode);
  if (radios("colorMode") === "suggested") loadSuggestions();
}

async function afterOpen(info) {
  if (!info) return;
  showImage(info);
  if (prefs.auto) await process();
  else setStatus("Choose the settings, then click Vectorize.");
}

async function openPath(path) {
  const info = await busy("Opening image…", () => invoke("open_image", { path }));
  if (info) await afterOpen(info);
}

async function openBytes(bytes) {
  const info = await busy("Opening pasted image…", () => invoke("open_image_bytes", bytes));
  if (info) await afterOpen(info);
}

async function process() {
  if (!state.info) return;
  if (radios("colorMode") === "suggested" && state.suggestions && state.activeSuggestion >= 0) {
    return applySuggestion(state.activeSuggestion);
  }
  const t0 = performance.now();
  const v = await busy("Vectorizing…", () => invoke("process", { settings: settings() }));
  if (v) {
    setView(v);
    appendTiming(t0);
  }
}

// ---------------------------------------------------------------- crop
function startCrop() {
  if (!state.info) return;
  setTool("crop");
  state.crop = null;
  $("cropBar").hidden = false;
  $("cropApply").disabled = true;
}

function endCrop() {
  state.crop = null;
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
    const c = state.crop;
    r.setAttribute("x", Math.min(c.x0, c.x1));
    r.setAttribute("y", Math.min(c.y0, c.y1));
    r.setAttribute("width", Math.abs(c.x1 - c.x0));
    r.setAttribute("height", Math.abs(c.y1 - c.y0));
  }
}

async function applyCrop() {
  const c = state.crop;
  if (!c) return;
  const clamp = (v, m) => Math.max(0, Math.min(m, Math.round(v)));
  const x0 = clamp(Math.min(c.x0, c.x1), state.info.width), x1 = clamp(Math.max(c.x0, c.x1), state.info.width);
  const y0 = clamp(Math.min(c.y0, c.y1), state.info.height), y1 = clamp(Math.max(c.y0, c.y1), state.info.height);
  if (x1 - x0 < 2 || y1 - y0 < 2) return setStatus("The crop rectangle is too small.", true);
  const name = state.info.name;
  endCrop();
  setTool("pan");
  const info = await busy("Cropping…", () => invoke("crop", { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }));
  if (info) {
    info.name = name;
    await afterOpen(info);
  }
}

// ---------------------------------------------------------------- pointer
function imageCoords(viewport, ev) {
  const r = viewport.getBoundingClientRect();
  return [(ev.clientX - r.left - state.px) / state.z, (ev.clientY - r.top - state.py) / state.z];
}

for (const p of Object.values(panes)) {
  const vp = p.viewport;
  let drag = null;
  let stroke = null;

  vp.addEventListener("wheel", (ev) => {
    ev.preventDefault();
    const r = vp.getBoundingClientRect();
    zoomAt(ev.deltaY < 0 ? 1.15 : 1 / 1.15, ev.clientX - r.left, ev.clientY - r.top);
  }, { passive: false });

  vp.addEventListener("pointerdown", (ev) => {
    if (!state.info) return;
    vp.setPointerCapture(ev.pointerId);
    const panning = state.tool === "pan" || ev.button === 1 || state.space;
    if (panning) {
      drag = { x: ev.clientX - state.px, y: ev.clientY - state.py };
      vp.classList.add("dragging");
      return;
    }
    if (ev.button !== 0) return;
    const pt = imageCoords(vp, ev);
    if (state.tool === "crop") {
      state.crop = { x0: pt[0], y0: pt[1], x1: pt[0], y1: pt[1] };
      stroke = { crop: true };
      drawCrop();
      return;
    }
    if (!state.view) return;
    if (state.tool === "picker") {
      busy("Reading color…", async () => {
        const hex = await invoke("pick_color", { x: pt[0], y: pt[1] });
        startDraft();
        if (!state.draft.includes(hex)) state.draft.push(hex);
        renderPalette();
        setStatus(`Added ${hex}. Click "Apply palette" to recompute.`);
      });
      return;
    }
    if (state.tool === "fill") {
      runEdit({ tool: "fill", x: pt[0], y: pt[1], label: state.selected });
      return;
    }
    if (state.tool === "pencil" || state.tool === "zap") {
      const color = state.view.palette[state.selected];
      const line = document.createElementNS(SVGNS, "polyline");
      line.setAttribute("fill", "none");
      const zap = state.tool === "zap";
      line.setAttribute("stroke", zap ? "#ff2bd6" : color.transparent ? "#ff00ff" : color.hex);
      line.setAttribute("stroke-opacity", zap || color.transparent ? "0.5" : "1");
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
      state.px = ev.clientX - drag.x;
      state.py = ev.clientY - drag.y;
      applyTransform();
    } else if (stroke && stroke.crop) {
      const pt = imageCoords(vp, ev);
      state.crop.x1 = pt[0];
      state.crop.y1 = pt[1];
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
      else op = { tool: "pencil", points: s.points, radius, label: state.selected };
      runEdit(op).finally(() => s.line.remove());
    }
  };
  vp.addEventListener("pointerup", end);
  vp.addEventListener("pointercancel", end);
}

// ---------------------------------------------------------------- export
$("btnExport").addEventListener("click", () => {
  if (!state.view) return;
  syncExportRows();
  $("exportDlg").showModal();
});
function syncExportRows() {
  const f = $("expFormat").value;
  $("expScaleRow").hidden = !["png", "gif", "jpg", "bmp", "tif"].includes(f);
  $("dxfRow").hidden = f !== "dxf";
}
$("expFormat").addEventListener("change", syncExportRows);
$("exportDlg").addEventListener("close", async () => {
  if ($("exportDlg").returnValue !== "default") return;
  const ext = $("expFormat").value;
  const path = await invoke("pick_save", { defaultName: `${baseName()}.${ext}`, ext });
  if (!path) return;
  const saved = await busy("Exporting…", () => invoke("export", { path, options: exportOptions() }));
  if (saved) setStatus(`Saved: ${saved}`);
});
$("expCopy").addEventListener("click", async () => {
  try {
    const text = await invoke("export_svg_text", { options: exportOptions() });
    await navigator.clipboard.writeText(text);
    setStatus(`SVG copied to the clipboard (${Math.round(text.length / 1024)} KB).`);
    $("exportDlg").close("cancel");
  } catch (e) {
    setStatus(`Could not copy: ${e}`, true);
  }
});

// ---------------------------------------------------------------- batch
const batch = { paths: [], out: null, running: false };
function refreshBatch() {
  $("batchCount").textContent = `${batch.paths.length} images`;
  $("batchOut").textContent = batch.out || "—";
  $("batchGo").disabled = batch.running || !(batch.paths.length && batch.out);
  $("batchCancel").disabled = !batch.running;
}
function openBatch(paths = []) {
  if (paths.length) batch.paths = paths;
  if (!batch.running) {
    $("batchLog").innerHTML = "";
    $("batchProgress").value = 0;
  }
  refreshBatch();
  $("batchDlg").showModal();
}
$("btnBatch").addEventListener("click", () => openBatch());
$("batchPick").addEventListener("click", async () => {
  const paths = await invoke("pick_images", { multiple: true });
  if (paths.length) batch.paths = paths;
  refreshBatch();
});
$("batchFolder").addEventListener("click", async () => {
  const dir = await invoke("pick_folder");
  if (dir) batch.out = dir;
  refreshBatch();
});
$("batchCancel").addEventListener("click", () => invoke("cancel_batch"));
$("batchGo").addEventListener("click", async () => {
  const formats = [...document.querySelectorAll("#batchFormats input:checked")].map((i) => i.value);
  if (!formats.length) return setStatus("Choose at least one format.", true);
  batch.running = true;
  refreshBatch();
  $("batchLog").innerHTML = "";
  const unlisten = await TAURI.event.listen("batch-progress", ({ payload: p }) => {
    $("batchProgress").max = p.total;
    $("batchProgress").value = p.index;
    document.title = `Vector Free — ${Math.round((100 * p.index) / p.total)}%`;
    const li = document.createElement("li");
    li.className = p.ok ? "good" : "bad";
    li.textContent = `${p.file.split(/[\\/]/).pop()}: ${p.message}`;
    $("batchLog").appendChild(li);
    li.scrollIntoView({ block: "nearest" });
  });
  try {
    const ok = await invoke("batch", {
      paths: batch.paths, outDir: batch.out, formats, settings: settings(), options: exportOptions(),
      autoKind: $("batchAuto").checked,
    });
    setStatus(`Batch finished: ${ok} of ${batch.paths.length} images saved to ${batch.out}`);
  } catch (e) {
    setStatus(String(e), true);
  } finally {
    unlisten();
    batch.running = false;
    document.title = "Vector Free";
    refreshBatch();
  }
});

// ---------------------------------------------------------------- paste
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
    setStatus("The clipboard does not contain an image.", true);
  } catch {
    setStatus("Could not read the clipboard here; press Ctrl+V instead.", true);
  }
}
document.addEventListener("paste", async (ev) => {
  if (document.querySelector("dialog[open]")) return;
  const file = [...(ev.clipboardData?.items || [])].find((i) => i.type.startsWith("image/"))?.getAsFile();
  if (!file) return;
  ev.preventDefault();
  openBytes(new Uint8Array(await file.arrayBuffer()));
});

// ---------------------------------------------------------------- controls
$("btnOpen").addEventListener("click", async () => {
  const [path] = await invoke("pick_images", { multiple: false });
  if (path) openPath(path);
});
$("btnPaste").addEventListener("click", pasteFromClipboard);
$("btnCrop").addEventListener("click", () => (state.tool === "crop" ? (endCrop(), setTool("pan")) : startCrop()));
$("cropApply").addEventListener("click", applyCrop);
$("cropCancel").addEventListener("click", () => { endCrop(); setTool("pan"); });
$("btnProcess").addEventListener("click", process);

async function history(cmd) {
  try {
    handleEdit(await invoke(cmd, { revectorize: $("autoUpdate").checked }));
  } catch (e) {
    setStatus(String(e), true);
  }
}
$("btnUndo").addEventListener("click", () => history("undo"));
$("btnRedo").addEventListener("click", () => history("redo"));
$("btnReset").addEventListener("click", () =>
  busy("Resetting edits…", async () => handleEdit(await invoke("reset_edits"))));
$("btnUpdate").addEventListener("click", () =>
  busy("Recomputing vectors…", async () => setView(await invoke("update"), true)));
$("btnFind").addEventListener("click", () => findProblems());
$("findNext").addEventListener("click", () => gotoProblem(1));
$("findPrev").addEventListener("click", () => gotoProblem(-1));
$("btnAddColor").addEventListener("click", () => {
  startDraft();
  renderPalette();
  setTool("picker");
  setStatus("Click the image to add that color to the palette.");
});
$("btnApplyPalette").addEventListener("click", () =>
  busy("Applying palette…", async () => setView(await invoke("resegment", { palette: state.draft, settings: settings() }), true)));

$("numColors").addEventListener("input", () => ($("numColorsOut").textContent = $("numColors").value));
$("brush").addEventListener("input", () => ($("brushOut").textContent = $("brush").value));
$("overlayOpacity").addEventListener("input", () => {
  const svg = panes.result.stage.querySelector("svg.vector");
  if (svg) svg.style.opacity = $("overlayOpacity").value;
});

function updateQualityText() {
  const t = QUALITY_TEXT[isArt() ? "art" : "photo"];
  $("qualityTitle").textContent = t.title;
  $("qualityHint").textContent = t[radios("quality")];
}
for (const r of document.querySelectorAll('input[name="kind"], input[name="quality"]')) {
  r.addEventListener("change", () => {
    updateQualityText();
    if (radios("colorMode") === "suggested") loadSuggestions();
  });
}
for (const r of document.querySelectorAll('input[name="colorMode"]')) {
  r.addEventListener("change", () => {
    const mode = radios("colorMode");
    $("countRow").hidden = mode !== "count";
    $("suggested").hidden = mode !== "suggested";
    if (mode === "suggested" && !state.suggestions) loadSuggestions();
  });
}
for (const r of document.querySelectorAll('input[name="tool"]')) {
  r.addEventListener("change", () => setTool(radios("tool")));
}
for (const r of document.querySelectorAll('input[name="view"]')) {
  r.addEventListener("change", () => setViewMode(radios("view")));
}

// Advanced sliders show "auto" until touched.
const ADV = [["advTol", "advTolOut", (v) => `${v} px`], ["advSmooth", "advSmoothOut", (v) => `×${v}`],
  ["advCorner", "advCornerOut", (v) => `${v}°`], ["advArea", "advAreaOut", (v) => `${v} px`]];
for (const [id, out, fmt] of ADV) {
  $(id).addEventListener("input", () => {
    $(id).dataset.auto = "0";
    $(out).textContent = fmt($(id).value);
  });
}
$("advReset").addEventListener("click", () => {
  for (const [id, out] of ADV) {
    $(id).dataset.auto = "1";
    $(out).textContent = "auto";
  }
  $("advDenoise").value = "";
  $("advCurves").value = "curves";
});

$("zoomIn").addEventListener("click", () => zoomCenter(1.25));
$("zoomOut").addEventListener("click", () => zoomCenter(1 / 1.25));
$("zoomFit").addEventListener("click", fit);
window.addEventListener("resize", () => state.info && fit());

// Help & preferences
$("btnHelp").addEventListener("click", () => $("helpDlg").showModal());
$("prefAuto").checked = prefs.auto;
$("prefAuto").addEventListener("change", () => {
  prefs.auto = $("prefAuto").checked;
  savePrefs();
});

document.addEventListener("keydown", (ev) => {
  if (ev.key === "F1") { ev.preventDefault(); $("helpDlg").showModal(); return; }
  if (ev.target.matches("input[type=number], input[type=text], select") || document.querySelector("dialog[open]")) return;
  const ctrl = ev.ctrlKey || ev.metaKey;
  const k = ev.key.toLowerCase();
  if (ctrl && k === "o") { ev.preventDefault(); $("btnOpen").click(); }
  else if (ctrl && k === "s") { ev.preventDefault(); $("btnExport").click(); }
  else if (ctrl && k === "z" && ev.shiftKey) { ev.preventDefault(); $("btnRedo").click(); }
  else if (ctrl && k === "z") { ev.preventDefault(); $("btnUndo").click(); }
  else if (ctrl && k === "y") { ev.preventDefault(); $("btnRedo").click(); }
  else if (ctrl) return;
  else if (ev.key === " ") { state.space = true; ev.preventDefault(); }
  else if (ev.key === "Escape" && state.tool === "crop") { endCrop(); setTool("pan"); }
  else if (ev.key === "Enter" && state.tool === "crop" && state.crop) applyCrop();
  else if (k === "b") setTool("pencil");
  else if (k === "f") setTool("fill");
  else if (k === "z") setTool("zap");
  else if (k === "i") setTool("picker");
  else if (k === "h") setTool("pan");
  else if (k === "c" && state.info) startCrop();
  else if (k === "u" && state.view) $("btnUpdate").click();
  else if (k === "n") gotoProblem(ev.shiftKey ? -1 : 1);
  else if (ev.key === "+" || ev.key === "=") zoomCenter(1.25);
  else if (ev.key === "-") zoomCenter(1 / 1.25);
  else if (ev.key === "0") fit();
});
document.addEventListener("keyup", (ev) => { if (ev.key === " ") state.space = false; });

// Drag & drop files onto the window.
TAURI.webview.getCurrentWebview().onDragDropEvent((ev) => {
  const { type, paths } = ev.payload;
  if (type === "enter" || type === "over") $("drop").classList.add("over");
  if (type === "leave") $("drop").classList.remove("over");
  if (type === "drop") {
    $("drop").classList.remove("over");
    if (!paths || !paths.length) return;
    if (paths.length === 1) openPath(paths[0]);
    else openBatch(paths);
  }
});

updateQualityText();
setViewMode("compare");
// An image passed on the command line (e.g. dropped onto the program icon).
invoke("startup_file").then((p) => p && openPath(p)).catch(() => {});
