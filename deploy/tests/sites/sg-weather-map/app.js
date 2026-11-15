import { fetchAll, LAYERS, colorFor, legendGradient, forecastIcon, compass, fmtTime } from "./data.js";

const cfg = window.SG_WEATHER_CONFIG || {};
const params = new URLSearchParams(location.search);
const API_KEY = params.get("key") || cfg.GOOGLE_MAPS_API_KEY || localStorage.getItem("sgwx.key") || "";
const MAP_ID = params.get("mapId") || cfg.GOOGLE_MAP_ID || localStorage.getItem("sgwx.mapId") || "";
const REFRESH_MS = (cfg.REFRESH_MINUTES || 5) * 60 * 1000;

const SG_CENTER = { lat: 1.3521, lng: 103.8198 };
const SG_BOUNDS = { north: 1.50, south: 1.15, west: 103.58, east: 104.10 };

const state = { layer: "temperature", weather: null, map: null, markers: null, three: null, use3D: !!MAP_ID };
const $ = (id) => document.getElementById(id);

// ---------------- Google Maps loading ----------------
function loadGoogleMaps(key) {
  // Official dynamic bootstrap loader (https://developers.google.com/maps/documentation/javascript/load-maps-js-api)
  ((g) => { var h, a, k, p = "The Google Maps JavaScript API", c = "google", l = "importLibrary", q = "__ib__", m = document, b = window; b = b[c] || (b[c] = {}); var d = b.maps || (b.maps = {}), r = new Set, e = new URLSearchParams, u = () => h || (h = new Promise(async (f, n) => { await (a = m.createElement("script")); e.set("libraries", [...r] + ""); for (k in g) e.set(k.replace(/[A-Z]/g, t => "_" + t[0].toLowerCase()), g[k]); e.set("callback", c + ".maps." + q); a.src = `https://maps.${c}apis.com/maps/api/js?` + e; d[q] = f; a.onerror = () => h = n(Error(p + " could not load.")); a.nonce = m.querySelector("script[nonce]")?.nonce || ""; m.head.append(a) })); d[l] ? console.warn(p + " only loads once. Ignoring:", g) : d[l] = (f, ...n) => r.add(f) && u().then(() => d[l](f, ...n)) })({ key, v: "weekly" });
  return google.maps.importLibrary("maps");
}

async function initMap() {
  const { Map } = await loadGoogleMaps(API_KEY);
  state.map = new Map($("map"), {
    center: SG_CENTER,
    zoom: 11.3,
    minZoom: 10,
    restriction: { latLngBounds: SG_BOUNDS, strictBounds: false },
    mapId: MAP_ID || undefined,
    tilt: MAP_ID ? 50 : 0,
    heading: 0,
    mapTypeControl: false,
    streetViewControl: false,
    fullscreenControl: true,
    clickableIcons: false,
    // Styles only apply without a mapId (cloud styling takes over otherwise).
    styles: MAP_ID ? undefined : [
      { featureType: "poi", stylers: [{ visibility: "off" }] },
      { featureType: "transit", stylers: [{ visibility: "off" }] },
      { featureType: "water", elementType: "geometry", stylers: [{ color: "#a9d3f0" }] },
    ],
  });
  state.info = new google.maps.InfoWindow();
  state.markers = createMarkerOverlay(state.map);

  if (MAP_ID) {
    try {
      const { ThreeWeatherLayer } = await import("./three-layer.js");
      state.three = new ThreeWeatherLayer(state.map);
      state.three.setVisible(state.use3D);
    } catch (e) {
      console.error("three.js layer failed", e);
      state.use3D = false;
      showError("3D layer unavailable: " + e.message);
    }
  }
  setup3DToggle();
}

// ---------------- HTML marker overlay (works with or without a Map ID) ----------------
function createMarkerOverlay(map) {
  class MarkerOverlay extends google.maps.OverlayView {
    constructor() { super(); this.items = []; this.div = document.createElement("div"); this.div.style.position = "absolute"; }
    onAdd() { this.getPanes().overlayMouseTarget.appendChild(this.div); }
    onRemove() { this.div.remove(); }
    setItems(items) {
      this.div.replaceChildren();
      this.items = items.map((it) => {
        const el = document.createElement("div");
        el.className = "wx-marker " + (it.className || "");
        el.innerHTML = it.html;
        if (it.style) Object.assign(el.style, it.style);
        el.title = it.title || "";
        el.addEventListener("click", (ev) => {
          ev.stopPropagation();
          state.info.setContent(it.info);
          state.info.setPosition(it.pos);
          state.info.open({ map });
        });
        this.div.appendChild(el);
        return { el, pos: new google.maps.LatLng(it.pos) };
      });
      this.draw();
    }
    draw() {
      const proj = this.getProjection();
      if (!proj) return;
      for (const { el, pos } of this.items) {
        const p = proj.fromLatLngToDivPixel(pos);
        el.style.left = p.x + "px";
        el.style.top = p.y + "px";
      }
    }
  }
  const o = new MarkerOverlay();
  o.setMap(map);
  return o;
}

// ---------------- Rendering ----------------
function stationInfo(s, extra = "") {
  const w = state.weather;
  const find = (k) => w[k]?.stations.find((x) => x.id === s.id);
  const t = find("temperature"), h = find("humidity"), r = find("rainfall"), wd = w.wind?.stations.find((x) => x.id === s.id);
  return `<div style="font:13px system-ui;min-width:170px">
    <b>${s.name}</b> <span style="color:#64748b">(${s.id})</span><br/>${extra}
    ${t ? `🌡️ ${t.value.toFixed(1)} °C<br/>` : ""}
    ${h ? `💧 ${Math.round(h.value)} % RH<br/>` : ""}
    ${r ? `🌧️ ${r.value.toFixed(1)} mm (5 min)<br/>` : ""}
    ${wd ? `💨 ${wd.value.toFixed(1)} kn from ${compass(wd.direction)} (${wd.direction}°)<br/>` : ""}
  </div>`;
}

function buildMarkers() {
  const w = state.weather;
  const L = state.layer;
  let items = [];
  const show3D = state.use3D && state.three;

  if (L === "forecast" && w.forecast2h) {
    items = w.forecast2h.areas.map((a) => ({
      pos: { lat: a.lat, lng: a.lng },
      className: "area",
      html: `<span class="e">${forecastIcon(a.forecast)}</span>${a.name}`,
      title: `${a.name}: ${a.forecast}`,
      info: `<div style="font:13px system-ui"><b>${a.name}</b><br/>${forecastIcon(a.forecast)} ${a.forecast}<br/><span style="color:#64748b">${w.forecast2h.validText}</span></div>`,
    }));
  } else if (L === "wind" && w.wind) {
    items = w.wind.stations.map((s) => {
      const toward = (s.direction + 180) % 360;
      return {
        pos: { lat: s.lat, lng: s.lng },
        className: "wind",
        html: show3D
          ? `<span style="font-size:11px;font-weight:700;background:#fff;border-radius:8px;padding:1px 5px">${s.value.toFixed(0)}kn</span>`
          : `<span class="arrow" style="transform:rotate(${toward}deg);color:${colorFor("wind", s.value)}">⬆</span>`,
        title: `${s.name}: ${s.value} kn from ${compass(s.direction)}`,
        info: stationInfo(s),
      };
    });
  } else if (w[L]) {
    const cfgL = LAYERS[L];
    items = w[L].stations.map((s) => {
      const tiny = L === "rainfall" && s.value === 0;
      return {
        pos: { lat: s.lat, lng: s.lng },
        className: tiny ? "dot" : "",
        html: tiny ? "" : cfgL.fmt(s.value),
        style: { background: colorFor(L, s.value), color: L === "temperature" && (s.value > 33 || s.value < 26) ? "#fff" : "#0f172a" },
        title: `${s.name}: ${cfgL.fmt(s.value)} ${cfgL.unit}`,
        info: stationInfo(s),
      };
    });
  }
  state.markers?.setItems(items);
  if (state.three) state.three.update(L, w);
}

const avg = (a) => a.reduce((x, y) => x + y, 0) / a.length;

function renderSummary() {
  const w = state.weather;
  const g = w.forecast24h?.general;
  const temps = w.temperature?.stations.map((s) => s.value) || [];
  const hum = w.humidity?.stations.map((s) => s.value) || [];
  const wind = w.wind?.stations || [];
  const raining = w.rainfall?.stations.filter((s) => s.value > 0).length ?? 0;
  const text = g?.forecast?.text || "";
  $("summary").innerHTML = `
    <div class="big">
      <div class="icon">${forecastIcon(text)}</div>
      <div>
        <div class="temp">${temps.length ? avg(temps).toFixed(1) + "°C" : "–"}</div>
        <div class="muted">Island average now · ${text || "—"}</div>
      </div>
    </div>
    <div class="grid">
      <div>Today range<b>${g ? `${g.temperature.low}–${g.temperature.high}°C` : "–"}</b></div>
      <div>Humidity<b>${hum.length ? Math.round(avg(hum)) + "%" : "–"}</b></div>
      <div>Wind (avg)<b>${wind.length ? avg(wind.map((s) => s.value)).toFixed(1) + " kn" : "–"}</b></div>
      <div>Raining at<b>${w.rainfall ? `${raining}/${w.rainfall.stations.length} stns` : "–"}</b></div>
    </div>`;
}

function renderStats() {
  const w = state.weather;
  const L = state.layer;
  const el = $("stats");
  if (L === "forecast" && w.forecast2h) {
    const counts = {};
    w.forecast2h.areas.forEach((a) => (counts[a.forecast] = (counts[a.forecast] || 0) + 1));
    el.innerHTML = `<h2>2-hour forecast · ${w.forecast2h.validText}</h2>` +
      Object.entries(counts).sort((a, b) => b[1] - a[1])
        .map(([f, n]) => `<div class="statrow"><span>${forecastIcon(f)} ${f}</span><b>${n} areas</b></div>`).join("");
    return;
  }
  const data = L === "wind" ? w.wind : w[L];
  if (!data?.stations.length) { el.innerHTML = `<h2>${LAYERS[L].label}</h2><div class="muted">No data</div>`; return; }
  const sorted = [...data.stations].sort((a, b) => b.value - a.value);
  const fmt = LAYERS[L].fmt;
  const top = sorted.slice(0, 5).map((s) => `<div class="statrow"><span>${s.name}</span><b>${fmt(s.value)}</b></div>`).join("");
  const lo = sorted[sorted.length - 1];
  el.innerHTML = `<h2>${LAYERS[L].label} · ${data.stations.length} stations</h2>
    <div class="statrow"><span>Average</span><b>${fmt(avg(sorted.map((s) => s.value)))}</b></div>
    <div class="statrow"><span>Lowest – ${lo.name}</span><b>${fmt(lo.value)}</b></div>
    <hr style="border:none;border-top:1px solid #e2e8f0"/>
    <div class="muted">Highest</div>${top}`;
}

function renderOutlook() {
  const rec = state.weather.forecast24h;
  if (!rec) { $("outlook").innerHTML = ""; return; }
  const g = rec.general;
  $("outlook").innerHTML = `<h2>24-hour outlook</h2>
    <div class="muted" style="margin-bottom:6px">${g.validPeriod?.text || ""} · Wind ${g.wind?.direction || ""} ${g.wind?.speed?.low ?? ""}–${g.wind?.speed?.high ?? ""} km/h · RH ${g.relativeHumidity?.low}–${g.relativeHumidity?.high}%</div>
    <div class="periods">${(rec.periods || []).map((p) => `
      <div class="period"><b>${p.timePeriod.text}</b>
        <div class="regions">${Object.entries(p.regions).map(([r, v]) => `<span title="${v.text}">${r[0].toUpperCase() + r.slice(1)} ${forecastIcon(v.text)}</span>`).join("")}</div>
      </div>`).join("")}</div>`;
}

function renderLegend() {
  const L = LAYERS[state.layer];
  if (!L.stops) { $("legend").innerHTML = `<div class="muted">Icons show NEA 2-hour nowcast per planning area.</div>`; return; }
  $("legend").innerHTML = `<div class="bar" style="background:${legendGradient(state.layer)}"></div>
    <div class="ticks"><span>${L.min}</span><span>${L.unit}</span><span>${L.max}+</span></div>
    ${state.use3D && state.three ? `<div class="muted">3D: ${state.layer === "wind" ? "arrows point downwind" : "column height ∝ value"}${state.weather?.rainfall?.stations.some((s) => s.value > 0) ? " · blue particles = active rain" : ""}</div>` : ""}`;
}

function renderLayerButtons() {
  $("layer-buttons").innerHTML = Object.entries(LAYERS)
    .map(([k, v]) => `<button data-layer="${k}" class="${k === state.layer ? "active" : ""}">${v.label}</button>`).join("");
}

function renderAll() {
  if (!state.weather) return;
  renderLayerButtons();
  renderSummary();
  renderStats();
  renderOutlook();
  renderLegend();
  buildMarkers();
  const ts = state.weather.temperature?.timestamp || state.weather.forecast2h?.timestamp;
  $("updated").textContent = "Updated " + fmtTime(ts) + " SGT";
}

function showError(msg) { $("errors").textContent = msg; }

async function refresh() {
  $("refresh").disabled = true;
  try {
    state.weather = await fetchAll();
    showError(state.weather.errors.length ? "Some feeds failed: " + state.weather.errors.join("; ") : "");
    renderAll();
  } catch (e) {
    showError("Failed to load weather: " + e.message);
  } finally {
    $("refresh").disabled = false;
  }
}

function setup3DToggle() {
  const box = $("toggle-3d");
  const wrap = $("toggle-3d-wrap");
  if (!state.three) {
    box.checked = false; box.disabled = true;
    wrap.classList.add("disabled");
    wrap.title = "Add a vector Map ID in config.js (or ?mapId=) to enable the three.js 3D layer";
    wrap.lastChild.textContent = " 3D view (needs vector Map ID)";
    return;
  }
  box.addEventListener("change", () => {
    state.use3D = box.checked;
    state.three.setVisible(state.use3D);
    state.map.moveCamera({ tilt: state.use3D ? 50 : 0 });
    renderAll();
  });
}

// ---------------- Boot ----------------
$("layer-buttons").addEventListener("click", (e) => {
  const k = e.target.closest("button")?.dataset.layer;
  if (!k) return;
  state.layer = k;
  renderAll();
});
$("refresh").addEventListener("click", refresh);

async function start() {
  await initMap().catch((e) => showError("Google Maps failed to load: " + e.message));
  await refresh();
  setInterval(refresh, REFRESH_MS);
}

if (API_KEY) {
  start();
} else {
  $("key-dialog").hidden = false;
  $("key-form").addEventListener("submit", (e) => {
    e.preventDefault();
    localStorage.setItem("sgwx.key", $("key-input").value.trim());
    localStorage.setItem("sgwx.mapId", $("mapid-input").value.trim());
    location.reload();
  });
  refresh(); // panel still shows live data behind the dialog
}
