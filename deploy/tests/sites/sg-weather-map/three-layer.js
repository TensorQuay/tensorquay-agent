// three.js layer drawn inside Google Maps via WebGLOverlayView (requires a vector Map ID).
import * as THREE from "three";
import { LAYERS, colorFor } from "./data.js";

const ANCHOR = { lat: 1.3521, lng: 103.8198, altitude: 0 };
const M_PER_DEG_LAT = 110574;
const M_PER_DEG_LNG = 111320 * Math.cos((ANCHOR.lat * Math.PI) / 180);

// Local metres relative to the anchor. Frame from transformer.fromLatLngAltitude: x=east, y=north, z=up.
function toLocal(lat, lng) {
  return new THREE.Vector3((lng - ANCHOR.lng) * M_PER_DEG_LNG, (lat - ANCHOR.lat) * M_PER_DEG_LAT, 0);
}

export class ThreeWeatherLayer {
  constructor(map) {
    this.map = map;
    this.group = new THREE.Group();
    this.rain = null;
    this.overlay = new google.maps.WebGLOverlayView();
    this.overlay.onAdd = () => this.onAdd();
    this.overlay.onContextRestored = ({ gl }) => this.onContextRestored(gl);
    this.overlay.onDraw = ({ gl, transformer }) => this.onDraw(gl, transformer);
    this.overlay.onContextLost = () => { this.renderer?.dispose(); this.renderer = null; };
    this.clock = new THREE.Clock();
  }

  onAdd() {
    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera();
    this.scene.add(new THREE.AmbientLight(0xffffff, 0.75));
    const sun = new THREE.DirectionalLight(0xffffff, 0.9);
    sun.position.set(0.5, -1, 2);
    this.scene.add(sun);
    this.scene.add(this.group);
  }

  onContextRestored(gl) {
    this.renderer = new THREE.WebGLRenderer({ canvas: gl.canvas, context: gl, ...gl.getContextAttributes() });
    this.renderer.autoClear = false;
  }

  onDraw(gl, transformer) {
    if (!this.renderer) return;
    const m = transformer.fromLatLngAltitude(ANCHOR);
    this.camera.projectionMatrix = new THREE.Matrix4().fromArray(m);
    this.animate(this.clock.getDelta());
    this.renderer.render(this.scene, this.camera);
    this.renderer.resetState();
    this.overlay.requestRedraw(); // continuous animation
  }

  setVisible(on) { this.overlay.setMap(on ? this.map : null); }

  clear() {
    this.group.traverse((o) => { o.geometry?.dispose(); o.material?.dispose?.(); });
    this.group.clear();
    this.rain = null;
  }

  // Rebuild the scene for the active layer.
  update(layerKey, weather) {
    if (!this.scene) this.onAdd();
    this.clear();
    if (layerKey === "wind" && weather.wind) this.buildWind(weather.wind.stations);
    else if (LAYERS[layerKey]?.height && weather[layerKey]) this.buildBars(layerKey, weather[layerKey].stations);
    if (weather.rainfall) this.buildRain(weather.rainfall.stations);
    this.overlay.requestRedraw();
  }

  buildBars(layerKey, stations) {
    const L = LAYERS[layerKey];
    const radius = layerKey === "rainfall" ? 180 : 320;
    stations.forEach((s) => {
      const h = Math.max(20, L.height(s.value));
      const geo = new THREE.CylinderGeometry(radius, radius, h, 24);
      geo.rotateX(Math.PI / 2); // y-up cylinder -> z-up
      const mat = new THREE.MeshStandardMaterial({ color: colorFor(layerKey, s.value), transparent: true, opacity: 0.88, roughness: 0.5 });
      const mesh = new THREE.Mesh(geo, mat);
      mesh.position.copy(toLocal(s.lat, s.lng)).setZ(h / 2);
      mesh.userData = { targetZ: h, grow: 0 };
      mesh.scale.z = 0.01;
      this.group.add(mesh);
    });
  }

  buildWind(stations) {
    stations.forEach((s) => {
      const len = 600 + s.value * 160;
      const color = colorFor("wind", s.value);
      const mat = new THREE.MeshStandardMaterial({ color, roughness: 0.4 });
      const shaft = new THREE.Mesh(new THREE.CylinderGeometry(70, 70, len, 12), mat);
      const head = new THREE.Mesh(new THREE.ConeGeometry(220, 450, 16), mat);
      shaft.position.y = len / 2;
      head.position.y = len + 225;
      const arrow = new THREE.Group();
      arrow.add(shaft, head);
      // Arrow built along +y (north). Wind direction is where it blows FROM, so point downwind.
      const toward = (s.direction + 180) % 360;
      arrow.rotation.z = -(toward * Math.PI) / 180; // clockwise from north
      const holder = new THREE.Group();
      holder.add(arrow);
      holder.position.copy(toLocal(s.lat, s.lng)).setZ(600);
      holder.userData = { bob: Math.random() * Math.PI * 2 };
      this.group.add(holder);
    });
  }

  // Falling rain particles above stations currently recording rainfall.
  buildRain(stations) {
    const wet = stations.filter((s) => s.value > 0);
    if (!wet.length) return;
    const pts = [];
    const speeds = [];
    wet.forEach((s) => {
      const c = toLocal(s.lat, s.lng);
      const n = Math.min(400, 60 + s.value * 120);
      for (let i = 0; i < n; i++) {
        pts.push(c.x + (Math.random() - 0.5) * 1600, c.y + (Math.random() - 0.5) * 1600, Math.random() * 2500);
        speeds.push(900 + Math.random() * 600);
      }
    });
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.Float32BufferAttribute(pts, 3));
    const mat = new THREE.PointsMaterial({ color: 0x3b82f6, size: 40, transparent: true, opacity: 0.7, sizeAttenuation: true });
    this.rain = new THREE.Points(geo, mat);
    this.rain.userData.speeds = speeds;
    this.group.add(this.rain);
  }

  animate(dt) {
    dt = Math.min(dt, 0.1);
    const t = this.clock.elapsedTime;
    this.group.children.forEach((o) => {
      if (o.userData.targetZ !== undefined && o.scale.z < 1) {
        o.scale.z = Math.min(1, o.scale.z + dt * 1.5);
        o.position.z = (o.userData.targetZ * o.scale.z) / 2;
      }
      if (o.userData.bob !== undefined) o.position.z = 600 + Math.sin(t * 2 + o.userData.bob) * 80;
    });
    if (this.rain) {
      const p = this.rain.geometry.attributes.position;
      const sp = this.rain.userData.speeds;
      for (let i = 0; i < p.count; i++) {
        let z = p.getZ(i) - sp[i] * dt;
        if (z < 0) z += 2500;
        p.setZ(i, z);
      }
      p.needsUpdate = true;
    }
  }
}
