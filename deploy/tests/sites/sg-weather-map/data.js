// Data layer: fetches and normalises data.gov.sg real-time weather APIs (no key needed, CORS enabled).
const BASE = "https://api-open.data.gov.sg/v2/real-time/api/";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// data.gov.sg rate-limits bursts without a key (HTTP 429), so retry with backoff.
async function getJSON(endpoint, attempts = 5) {
  let res, lastErr;
  for (let i = 0; i < attempts; i++) {
    try {
      res = await fetch(BASE + endpoint, { cache: "no-store" });
      if (res.status !== 429 && res.status < 500) break;
    } catch (e) {
      // A 429 response lacks CORS headers, so browsers surface it as a network TypeError.
      res = null; lastErr = e;
    }
    if (i < attempts - 1) await sleep(1500 * 2 ** i + Math.random() * 500);
  }
  if (!res) throw new Error(`${endpoint}: ${lastErr?.message || "network error"}`);
  if (!res.ok) throw new Error(`${endpoint}: HTTP ${res.status}`);
  const json = await res.json();
  if (json.code !== 0 || !json.data) throw new Error(`${endpoint}: ${json.errorMsg || "bad response"}`);
  return json.data;
}

// Station readings -> [{id, name, lat, lng, value}] plus timestamp/unit.
export function normaliseStations(data) {
  const reading = (data.readings || [])[0] || { data: [] };
  const values = new Map(reading.data.map((d) => [d.stationId, d.value]));
  const stations = (data.stations || [])
    .filter((s) => values.has(s.id))
    .map((s) => ({
      id: s.id,
      name: s.name,
      lat: s.location.latitude,
      lng: s.location.longitude,
      value: values.get(s.id),
    }));
  return { stations, timestamp: reading.timestamp || null, unit: data.readingUnit || "" };
}

export function normaliseTwoHour(data) {
  const item = (data.items || [])[0] || { forecasts: [] };
  const fc = new Map(item.forecasts.map((f) => [f.area, f.forecast]));
  const areas = (data.area_metadata || []).map((a) => ({
    name: a.name,
    lat: a.label_location.latitude,
    lng: a.label_location.longitude,
    forecast: fc.get(a.name) || "N/A",
  }));
  return { areas, validText: item.valid_period?.text || "", timestamp: item.update_timestamp || item.timestamp || null };
}

// Combine wind speed + direction by station.
export function combineWind(speed, dir) {
  const dirMap = new Map(dir.stations.map((s) => [s.id, s.value]));
  return {
    stations: speed.stations
      .filter((s) => dirMap.has(s.id))
      .map((s) => ({ ...s, direction: dirMap.get(s.id) })),
    timestamp: speed.timestamp,
    unit: speed.unit,
  };
}

export async function fetchAll() {
  const endpoints = [
    "air-temperature", "rainfall", "relative-humidity", "wind-speed",
    "wind-direction", "two-hr-forecast", "twenty-four-hr-forecast",
  ];
  // Fetch sequentially with a small gap: the keyless API rate-limits bursts.
  const results = [];
  for (const e of endpoints) {
    results.push(await getJSON(e).then((value) => ({ status: "fulfilled", value }), (reason) => ({ status: "rejected", reason })));
    await sleep(250);
  }
  const r = Object.fromEntries(endpoints.map((e, i) => [e, results[i]]));
  const ok = (e) => r[e].status === "fulfilled";
  const errors = endpoints.filter((e) => !ok(e)).map((e) => String(r[e].reason?.message || r[e].reason));

  const out = { errors };
  if (ok("air-temperature")) out.temperature = normaliseStations(r["air-temperature"].value);
  if (ok("rainfall")) out.rainfall = normaliseStations(r["rainfall"].value);
  if (ok("relative-humidity")) out.humidity = normaliseStations(r["relative-humidity"].value);
  if (ok("wind-speed") && ok("wind-direction")) {
    out.wind = combineWind(normaliseStations(r["wind-speed"].value), normaliseStations(r["wind-direction"].value));
  }
  if (ok("two-hr-forecast")) out.forecast2h = normaliseTwoHour(r["two-hr-forecast"].value);
  if (ok("twenty-four-hr-forecast")) out.forecast24h = (r["twenty-four-hr-forecast"].value.records || [])[0] || null;
  return out;
}

// ---------- presentation helpers ----------
export function forecastIcon(text = "") {
  const t = text.toLowerCase();
  const night = t.includes("night");
  if (t.includes("thunder")) return "⛈️";
  if (t.includes("heavy rain") || t.includes("heavy showers")) return "🌧️";
  if (t.includes("rain") || t.includes("shower") || t.includes("drizzle")) return "🌦️";
  if (t.includes("haz")) return "🌫️";
  if (t.includes("mist") || t.includes("fog")) return "🌁";
  if (t.includes("wind")) return "💨";
  if (t.includes("partly cloudy")) return night ? "☁️" : "⛅";
  if (t.includes("cloudy") || t.includes("overcast")) return "☁️";
  if (t.includes("fair") || t.includes("sunny") || t.includes("warm")) return night ? "🌙" : "☀️";
  return "🌡️";
}

function lerp(a, b, t) { return a + (b - a) * t; }
function gradient(stops, t) {
  t = Math.min(1, Math.max(0, t));
  const n = stops.length - 1;
  const i = Math.min(n - 1, Math.floor(t * n));
  const f = t * n - i;
  const [a, b] = [stops[i], stops[i + 1]];
  const c = [0, 1, 2].map((k) => Math.round(lerp(a[k], b[k], f)));
  return `rgb(${c[0]},${c[1]},${c[2]})`;
}

// Each layer: value range, colour ramp, label formatting, 3D bar height scale (metres per unit).
export const LAYERS = {
  temperature: {
    label: "Temperature", unit: "°C", min: 24, max: 36,
    stops: [[49, 54, 149], [69, 117, 180], [116, 173, 209], [254, 224, 144], [244, 109, 67], [165, 0, 38]],
    fmt: (v) => `${v.toFixed(1)}°`, height: (v) => Math.max(0, v - 20) * 180,
  },
  rainfall: {
    label: "Rainfall (5 min)", unit: "mm", min: 0, max: 5,
    stops: [[220, 230, 240], [120, 190, 240], [30, 110, 220], [70, 20, 160]],
    fmt: (v) => `${v.toFixed(1)}`, height: (v) => 40 + v * 700,
  },
  humidity: {
    label: "Humidity", unit: "%", min: 40, max: 100,
    stops: [[255, 237, 160], [161, 218, 180], [65, 182, 196], [34, 94, 168]],
    fmt: (v) => `${Math.round(v)}%`, height: (v) => v * 25,
  },
  wind: {
    label: "Wind", unit: "kn", min: 0, max: 20,
    stops: [[180, 230, 180], [90, 180, 90], [250, 200, 50], [220, 60, 40]],
    fmt: (v) => `${v.toFixed(1)}kn`, height: (v) => 100 + v * 120,
  },
  forecast: { label: "2-hour forecast" },
};

export function colorFor(layerKey, v) {
  const L = LAYERS[layerKey];
  return gradient(L.stops, (v - L.min) / (L.max - L.min));
}

export function legendGradient(layerKey) {
  const L = LAYERS[layerKey];
  return `linear-gradient(to right, ${L.stops.map((s) => `rgb(${s.join(",")})`).join(",")})`;
}

const COMPASS = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];
export function compass(deg) { return COMPASS[Math.round(((deg % 360) + 360) % 360 / 22.5) % 16]; }

export function fmtTime(ts) {
  if (!ts) return "–";
  return new Date(ts).toLocaleString("en-SG", { timeZone: "Asia/Singapore", hour: "2-digit", minute: "2-digit", day: "numeric", month: "short" });
}
