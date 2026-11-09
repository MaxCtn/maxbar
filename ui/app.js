'use strict';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const TOPICS = ['config', 'geometry', 'pins', 'tasks', 'apps', 'media', 'art', 'stats', 'claude', 'weather', 'volume', 'keyboard', 'tray'];
const EMPTY = { pins: [], tasks: { windows: [], active: 0 }, apps: {}, tray: [] };
const S = {
  config: null, geometry: null, pins: [], tasks: EMPTY.tasks, apps: {}, media: null, art: null,
  stats: null, claude: null, weather: null, volume: null, keyboard: null, tray: [],
};
const $ = (id) => document.getElementById(id);
const root = $('root');
const HISTORY = 90; // 3 min d'historique à 2 s
const hist = { cpu: [], ram: [], gpu: [] };
/** Marge de région autour des surfaces flottantes, pour leur ombre (fenêtre transparente). */
const SHADOW = 22;
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

// ================================================================= utilitaires
function h(tag, props, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'style') el.style.cssText = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const c of children.flat()) if (c != null && c !== false) el.append(c.nodeType ? c : String(c));
  return el;
}
const svg = (tag, attrs) => {
  const el = document.createElementNS('http://www.w3.org/2000/svg', tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  return el;
};
/** Icône à partir d'un balisage SVG constant (jamais de données externes). */
function icon(markup) {
  const t = document.createElement('template');
  t.innerHTML = markup.trim();
  return t.content.firstChild;
}
function setIcon(el, markup) {
  if (el._markup === markup) return;
  el._markup = markup;
  el.replaceChildren(icon(markup));
}
/** N'écrit dans le DOM que si le texte change (évite des redessins inutiles). */
function setText(id, text) {
  const el = typeof id === 'string' ? $(id) : id;
  if (el.textContent !== text) el.textContent = text;
}
/** Au plus un appel tous les `ms`, le dernier étant toujours envoyé (glissement d'un curseur). */
function throttle(fn, ms) {
  let last = 0;
  let timer = 0;
  let args = null;
  return (...a) => {
    args = a;
    const wait = last + ms - Date.now();
    if (wait <= 0) {
      last = Date.now();
      fn(...args);
    } else if (!timer) {
      timer = setTimeout(() => { timer = 0; last = Date.now(); fn(...args); }, wait);
    }
  };
}
const level = (p) => (p >= 85 ? 'crit' : p >= 60 ? 'warn' : '');
const pct = (p) => `${Math.round(p)} %`;
const signed = (v) => `+${v < 10 && v % 1 ? v.toFixed(1).replace('.', ',') : Math.round(v)} %`;

function bytes(n) {
  if (n < 1024) return `${n} o/s`;
  if (n < 1048576) return `${(n / 1024).toFixed(n < 10240 ? 1 : 0)} Ko/s`;
  return `${(n / 1048576).toFixed(n < 10485760 ? 1 : 0)} Mo/s`;
}
function duration(sec) {
  sec = Math.max(0, Math.round(sec));
  const d = Math.floor(sec / 86400), hh = Math.floor((sec % 86400) / 3600), m = Math.floor((sec % 3600) / 60);
  if (d) return `${d} j ${hh} h`;
  if (hh) return `${hh} h ${String(m).padStart(2, '0')}`;
  if (m) return `${m} min`;
  return `${sec} s`;
}
function mmss(ms) {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}
function hue(str) {
  let x = 0;
  for (const c of str) x = (x * 31 + c.charCodeAt(0)) | 0;
  return Math.abs(x) % 360;
}

// ====================================================================== icônes
const I = {
  speaker: (v) => {
    const base = '<path d="M3.5 7.6h2.9l4.1-3.4v11.6l-4.1-3.4H3.5z"/>';
    if (!v || v.muted) return `<svg viewBox="0 0 20 20">${base}<path d="m13.6 8 4 4m0-4-4 4"/></svg>`;
    const w1 = v.level > 0 ? '<path d="M13.4 7.9a3 3 0 0 1 0 4.2"/>' : '';
    const w2 = v.level > 40 ? '<path d="M15.6 5.7a6.1 6.1 0 0 1 0 8.6"/>' : '';
    return `<svg viewBox="0 0 20 20">${base}${w1}${w2}</svg>`;
  },
  wifi: '<svg viewBox="0 0 20 20"><path d="M2.6 7.7a10.6 10.6 0 0 1 14.8 0M5.2 10.4a6.8 6.8 0 0 1 9.6 0M7.9 13.1a3 3 0 0 1 4.2 0"/><circle cx="10" cy="15.7" r="0.6"/></svg>',
  ethernet: '<svg viewBox="0 0 20 20"><rect x="3" y="3.8" width="14" height="9.4" rx="1.6"/><path d="M7 16.4h6M10 13.2v3.2"/></svg>',
  offline: '<svg viewBox="0 0 20 20"><circle cx="10" cy="10" r="7"/><path d="m4.5 4.5 11 11"/></svg>',
  chevL: '<svg viewBox="0 0 20 20" class="stroke"><path d="m12 5-5 5 5 5"/></svg>',
  chevR: '<svg viewBox="0 0 20 20" class="stroke"><path d="m8 5 5 5-5 5"/></svg>',
  bell: '<svg viewBox="0 0 20 20"><path d="M5.2 13.6V9a4.8 4.8 0 0 1 9.6 0v4.6l1.4 1.6H3.8z"/><path d="M8.3 17.2a1.9 1.9 0 0 0 3.4 0"/></svg>',
  toggles: '<svg viewBox="0 0 20 20"><rect x="2.8" y="4" width="14.4" height="5.2" rx="2.6"/><circle cx="14.6" cy="6.6" r="1.3"/><rect x="2.8" y="10.8" width="14.4" height="5.2" rx="2.6"/><circle cx="5.4" cy="13.4" r="1.3"/></svg>',
  gear: '<svg viewBox="0 0 20 20"><circle cx="10" cy="10" r="2.6"/><path d="M10 2.6v2M10 15.4v2M17.4 10h-2M4.6 10h-2M15.2 4.8l-1.4 1.4M6.2 13.8l-1.4 1.4M15.2 15.2l-1.4-1.4M6.2 6.2 4.8 4.8"/></svg>',
  pulse: '<svg viewBox="0 0 20 20"><path d="M2.5 10.5h3.2l2-5 3.6 9 2-4h4.2"/></svg>',
  claude: '<svg viewBox="0 0 20 20"><path d="M10 3v14M3 10h14M5 5l10 10M15 5 5 15"/></svg>',
};

/** Codes météo WMO -> libellé et famille d'icône. */
const WMO = {
  0: ['Ciel dégagé', 'clear'], 1: ['Plutôt dégagé', 'partly'], 2: ['Partiellement nuageux', 'partly'], 3: ['Couvert', 'cloud'],
  45: ['Brouillard', 'fog'], 48: ['Brouillard givrant', 'fog'],
  51: ['Bruine légère', 'drizzle'], 53: ['Bruine', 'drizzle'], 55: ['Bruine dense', 'drizzle'], 56: ['Bruine verglaçante', 'drizzle'], 57: ['Bruine verglaçante', 'drizzle'],
  61: ['Pluie faible', 'rain'], 63: ['Pluie', 'rain'], 65: ['Forte pluie', 'rain'], 66: ['Pluie verglaçante', 'rain'], 67: ['Pluie verglaçante', 'rain'],
  71: ['Neige faible', 'snow'], 73: ['Neige', 'snow'], 75: ['Forte neige', 'snow'], 77: ['Grains de neige', 'snow'],
  80: ['Averses', 'rain'], 81: ['Averses', 'rain'], 82: ['Fortes averses', 'rain'], 85: ['Averses de neige', 'snow'], 86: ['Averses de neige', 'snow'],
  95: ['Orage', 'storm'], 96: ['Orage et grêle', 'storm'], 99: ['Orage et grêle', 'storm'],
};
const wmo = (code) => WMO[code] || ['—', 'cloud'];

function sunSvg(cx, cy, r) {
  const rays = [0, 45, 90, 135, 180, 225, 270, 315].map((a) => {
    const t = (a * Math.PI) / 180;
    const p = (k) => `${(cx + Math.cos(t) * k).toFixed(2)}`;
    const q = (k) => `${(cy + Math.sin(t) * k).toFixed(2)}`;
    return `<line x1="${p(r + 2)}" y1="${q(r + 2)}" x2="${p(r + 3.7)}" y2="${q(r + 3.7)}"/>`;
  }).join('');
  return `<circle cx="${cx}" cy="${cy}" r="${r}" fill="#f7c24d"/><g stroke="#f7c24d" stroke-width="1.5" stroke-linecap="round">${rays}</g>`;
}
const moonSvg = (t = '') => `<path transform="${t}" d="M15.4 3.4a8.6 8.6 0 1 0 5.2 14.2A7.4 7.4 0 0 1 15.4 3.4z" fill="#cfc9ff"/>`;
const cloudSvg = (t = '', fill = '#e1e4ee') => `<path transform="${t}" d="M6.6 19.2h11.1a4.5 4.5 0 0 0 .5-8.97A6.1 6.1 0 0 0 6.5 9.3a4.95 4.95 0 0 0 .1 9.9z" fill="${fill}"/>`;

function weatherSvg(code, day = true) {
  const kind = wmo(code)[1];
  const drops = (n, color = '#6cb2ff') => `<g stroke="${color}" stroke-width="1.7" stroke-linecap="round">${
    [8, 12, 16].slice(0, n).map((x) => `<line x1="${x}" y1="17.6" x2="${x - 1.1}" y2="21"/>`).join('')}</g>`;
  let body;
  switch (kind) {
    case 'clear': body = day ? sunSvg(12, 12, 4.6) : moonSvg(); break;
    case 'partly': body = (day ? sunSvg(8.6, 8.4, 3.3) : moonSvg('translate(-2.5 -2.5) scale(0.62)')) + cloudSvg('translate(3.2 3.2) scale(0.84)'); break;
    case 'fog': body = `${cloudSvg('translate(0 -4)', '#c9ccd8')}<g stroke="#b9bccb" stroke-width="1.6" stroke-linecap="round"><line x1="4" y1="17.6" x2="20" y2="17.6"/><line x1="6.5" y1="21" x2="17.5" y2="21"/></g>`; break;
    case 'drizzle': body = cloudSvg('translate(0 -4)') + drops(2); break;
    case 'rain': body = cloudSvg('translate(0 -4)', '#cdd1de') + drops(3); break;
    case 'snow': body = `${cloudSvg('translate(0 -4)')}<g fill="#ffffff"><circle cx="8" cy="19" r="1.2"/><circle cx="12" cy="21" r="1.2"/><circle cx="16" cy="19" r="1.2"/></g>`; break;
    case 'storm': body = `${cloudSvg('translate(0 -4)', '#a9adbd')}<path d="M12.6 14.4 9.7 18.8h3l-1.5 3.6 4.2-5.4h-3l1.6-2.6z" fill="#f7c24d"/>`; break;
    default: body = cloudSvg('translate(0 -1)');
  }
  return `<svg viewBox="0 0 24 24">${body}</svg>`;
}

// =================================================================== géométrie
function geom() {
  return S.geometry || { bar_top: 0, bar_height: S.config?.height || 42, bottom: S.config?.edge === 'bottom' };
}

function applyGeometry() {
  const g = geom();
  const st = document.documentElement.style;
  st.setProperty('--bar-top', `${g.bar_top}px`);
  st.setProperty('--bar-h', `${g.bar_height}px`);
  root.classList.toggle('edge-bottom', !!g.bottom);
  root.classList.toggle('edge-top', !g.bottom);
  layoutIsland();
  if (pop.name) placePop();
}

// Régions : la fenêtre couvre l'écran, seules ces zones (et la barre) sont visibles et cliquables.
const shapes = new Map();
let shapesKey = '';
function rectOf(el) {
  return { x: el.offsetLeft - SHADOW, y: el.offsetTop - SHADOW, w: el.offsetWidth + SHADOW * 2, h: el.offsetHeight + SHADOW * 2, r: 0 };
}
function setShape(name, rect) {
  if (rect) shapes.set(name, rect);
  else shapes.delete(name);
  const list = [...shapes.values()].map((s) => ({ x: Math.round(s.x), y: Math.round(s.y), w: Math.round(s.w), h: Math.round(s.h), r: s.r || 0 }));
  const key = JSON.stringify(list);
  if (key === shapesKey) return Promise.resolve();
  shapesKey = key;
  return invoke('set_shapes', { shapes: list }).catch(() => {});
}

// ======================================================================== dock
const dockEls = new Map();
const seenOrder = [];
let dockReady = false;

function dockModel() {
  const groups = new Map();
  for (const w of S.tasks.windows) {
    if (!groups.has(w.key)) groups.set(w.key, []);
    groups.get(w.key).push(w);
  }
  for (const k of groups.keys()) if (!seenOrder.includes(k)) seenOrder.push(k);
  for (let i = seenOrder.length - 1; i >= 0; i--) if (!groups.has(seenOrder[i])) seenOrder.splice(i, 1);
  const pinned = new Set();
  const head = [];
  for (const p of S.pins || []) {
    if (pinned.has(p.key)) continue;
    pinned.add(p.key);
    head.push({ key: p.key, path: p.path, wins: groups.get(p.key) || [] });
  }
  const tail = seenOrder.filter((k) => !pinned.has(k)).map((k) => ({ key: k, path: S.apps[k]?.path || '', wins: groups.get(k) }));
  return { head, tail };
}

function appButton(item) {
  let el = dockEls.get(item.key);
  if (!el) {
    el = h('button', { class: 'app', 'data-key': item.key },
      h('img', { alt: '', draggable: 'false' }),
      h('span', { class: 'letter' }),
      h('i', { class: 'dot' }));
    el.addEventListener('click', () => onAppClick(el._item, el));
    el.addEventListener('auxclick', (e) => { if (e.button === 1 && el._item.path) launch(el._item, el); });
    el.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      const it = el._item;
      invoke('task_menu', { path: it.path, hwnds: it.wins.map((w) => w.hwnd), titles: it.wins.map((w) => w.title) });
    });
    if (dockReady) {
      el.classList.add('enter');
      setTimeout(() => el.classList.remove('enter'), 500);
    }
    dockEls.set(item.key, el);
  }
  el._item = item;
  const info = S.apps[item.key] || {};
  const name = info.name || item.path.split(/[\\/]/).pop() || 'Application';
  const img = el.querySelector('img');
  const letter = el.querySelector('.letter');
  if (info.icon) {
    if (img.getAttribute('src') !== info.icon) img.setAttribute('src', info.icon);
    img.hidden = false;
    letter.hidden = true;
  } else {
    img.hidden = true;
    letter.hidden = false;
    letter.textContent = name.trim().charAt(0).toUpperCase();
    letter.style.background = `hsl(${hue(item.key)} 45% 42%)`;
  }
  const n = item.wins.length;
  if (n > 0) el.classList.remove('launching');
  el.classList.toggle('running', n > 0);
  el.classList.toggle('multi', n > 1);
  el.classList.toggle('active', item.wins.some((w) => w.hwnd === S.tasks.active));
  el.classList.toggle('minimized', n > 0 && item.wins.every((w) => w.minimized));
  el.querySelector('.dot').hidden = n === 0;
  el.title = n === 0 ? name : n === 1 ? `${name} — ${item.wins[0].title}` : `${name} — ${n} fenêtres`;
  el.setAttribute('aria-label', el.title);
  return el;
}

function renderDock() {
  const { head, tail } = dockModel();
  const dock = $('dock');
  const nodes = [...head.map(appButton)];
  if (head.length && tail.length) nodes.push(dock.querySelector('.sep') || h('span', { class: 'sep' }));
  nodes.push(...tail.map(appButton));
  const keep = new Set(nodes);
  for (const [k, el] of dockEls) if (!keep.has(el)) { el.remove(); dockEls.delete(k); }
  nodes.forEach((node, i) => { if (dock.children[i] !== node) dock.insertBefore(node, dock.children[i] || null); });
  while (dock.children.length > nodes.length) dock.lastChild.remove();
  dockReady = true;
}

function launch(item, el) {
  invoke('launch', { path: item.path });
  el.classList.remove('launching');
  void el.offsetWidth; // relance l'animation
  el.classList.add('launching');
  setTimeout(() => el.classList.remove('launching'), 1900);
}

function onAppClick(item, el) {
  const wins = item.wins;
  if (!wins.length) return item.path && launch(item, el);
  if (wins.length === 1) return invoke('activate', { hwnd: wins[0].hwnd });
  const i = wins.findIndex((w) => w.hwnd === S.tasks.active);
  invoke('activate', { hwnd: (i < 0 ? wins[0] : wins[(i + 1) % wins.length]).hwnd });
}

// ======================================================================== îlot
const island = { mode: 'hidden', expanded: false, peek: false, peekTimer: 0, toast: null, toastTimer: 0, shapeTimer: 0 };
let lastTrack = null;
let lastRatio = 0;

const mediaVisible = () => !!(S.media && (S.media.title || S.media.artist)) && S.config?.show_music !== false;

function islandMode() {
  if (island.toast) return 'toast';
  if (!mediaVisible()) return 'hidden';
  return island.expanded ? 'full' : 'compact';
}

function layoutIsland() {
  const el = $('island');
  const mode = islandMode();
  const g = geom();
  const ch = Math.max(24, g.bar_height - 10);
  el.style.setProperty('--ih', `${ch}px`);
  let w = 150;
  let hh = ch;
  if (mode === 'full') { w = 392; hh = $('iv-full').offsetHeight; }
  else if (mode === 'toast') w = 280;
  else if (mode === 'compact') w = clamp($('iv-compact').offsetWidth, 140, 420);
  const left = Math.round((innerWidth - w) / 2);
  const top = g.bottom ? g.bar_top + g.bar_height - 5 - hh : g.bar_top + 5;
  island.mode = mode;
  el.dataset.mode = mode;
  el.style.width = `${w}px`;
  el.style.height = `${hh}px`;
  el.style.left = `${left}px`;
  el.style.top = `${top}px`;
  el.style.borderRadius = `${mode === 'full' ? 30 : hh / 2}px`;
  clearTimeout(island.shapeTimer);
  if (mode === 'full') {
    setShape('island', { x: left - SHADOW, y: top - SHADOW, w: w + SHADOW * 2, h: hh + SHADOW * 2 + 12, r: 0 });
    tickMedia();
  } else {
    island.shapeTimer = setTimeout(() => setShape('island', null), 560);
  }
}

function expandIsland() {
  if (!mediaVisible()) return;
  closePop();
  closeDrawer();
  island.expanded = true;
  island.peek = false;
  clearTimeout(island.peekTimer);
  layoutIsland();
}

function collapseIsland() {
  if (!island.expanded) return;
  island.expanded = false;
  island.peek = false;
  clearTimeout(island.peekTimer);
  layoutIsland();
}

/** Changement de morceau : l'îlot s'ouvre quelques secondes, comme une notification. */
function peekIsland() {
  if (pop.name || drawer.open || island.expanded) return;
  island.expanded = true;
  island.peek = true;
  layoutIsland();
  clearTimeout(island.peekTimer);
  island.peekTimer = setTimeout(() => { if (island.peek) collapseIsland(); }, 4200);
}

function showToast(t, ms = 1500) {
  const el = $('island');
  island.toast = t;
  el.classList.toggle('toast-claude', t.kind === 'claude');
  el.classList.toggle('crit', !!t.crit);
  setIcon($('t-icon'), t.icon);
  setText('t-label', t.label);
  $('t-bar').style.transform = `scaleX(${clamp((t.value || 0) / 100, 0, 1)})`;
  setText('t-val', t.text || '');
  layoutIsland();
  clearTimeout(island.toastTimer);
  island.toastTimer = setTimeout(() => {
    island.toast = null;
    el.classList.remove('toast-claude', 'crit');
    layoutIsland();
  }, ms);
}

// ======================================================================= média
function mediaPosition() {
  const m = S.media;
  if (!m) return 0;
  let pos = m.position_ms;
  if (m.playing && m.updated_ms > 0) pos += Date.now() - m.updated_ms;
  return m.duration_ms > 0 ? Math.min(pos, m.duration_ms) : pos;
}

function renderMedia() {
  const m = S.media;
  const el = $('island');
  const vis = mediaVisible();
  el.classList.toggle('playing', !!(vis && m.playing));
  if (vis) {
    const title = m.title || 'Lecture en cours';
    const artist = m.artist || m.source || '';
    setText('i-title', title);
    setText('i-artist', artist);
    $('i-artist').hidden = !artist;
    setText('i-title-l', title);
    setText('i-artist-l', [m.artist, m.album].filter(Boolean).join(' — '));
    setText('i-src', m.source ? `${m.playing ? 'En lecture' : 'En pause'} · ${m.source}` : '');
    setText('i-dur', m.duration_ms > 0 ? mmss(m.duration_ms) : '');
    el.querySelector('[data-media="prev"]').disabled = !m.can_prev;
    el.querySelector('[data-media="next"]').disabled = !m.can_next;
    el.title = [m.title, m.artist, m.source && `via ${m.source}`].filter(Boolean).join(' — ');
  }
  const key = vis ? `${m.title}\u0001${m.artist}` : '';
  if (lastTrack !== null && key && key !== lastTrack && !island.toast && performance.now() > 4000) peekIsland();
  lastTrack = key;
  renderArt();
  layoutIsland();
}

function renderArt() {
  const img = $('i-art');
  const cover = $('i-cover');
  if (S.art) {
    if (img.getAttribute('src') !== S.art) img.setAttribute('src', S.art);
    cover.style.backgroundImage = `url("${S.art}")`;
  } else {
    img.removeAttribute('src');
    cover.style.backgroundImage = '';
  }
  cover.classList.toggle('has-art', !!S.art);
}

function tickMedia() {
  if (island.mode !== 'full') return;
  const m = S.media;
  const ratio = m && m.duration_ms > 0 ? clamp(mediaPosition() / m.duration_ms, 0, 1) : 0;
  const bar = $('i-bar');
  // Progression continue (transition linéaire d'1 s entre deux mises à jour), sans saut arrière animé.
  bar.style.transition = ratio < lastRatio || !m?.playing ? 'none' : '';
  bar.style.transform = `scaleX(${ratio})`;
  lastRatio = ratio;
  setText('i-pos', mmss(mediaPosition()));
}

// ====================================================================== claude
function claudeCalc() {
  const c = S.claude;
  if (!c) return null;
  const rows = c.history || [];
  const now = Date.now() / 1000;
  // Conso = somme des hausses (une baisse = remise à zéro de la fenêtre, ignorée).
  const consumed = (idx, since) => {
    let sum = 0;
    let prev = null;
    for (const r of rows) {
      if (r[0] <= since) { prev = r[idx]; continue; }
      if (prev != null && r[idx] > prev) sum += r[idx] - prev;
      prev = r[idx];
    }
    return sum;
  };
  const five = c.five_hour;
  const week = c.seven_day;
  // La conso 5 h ne compte que la fenêtre en cours (avant la remise à zéro, c'est une autre fenêtre).
  const start = five?.resets_at ? five.resets_at - 5 * 3600 : 0;
  const d15 = consumed(1, Math.max(start, now - 900));
  const d30 = consumed(1, Math.max(start, now - 1800));
  const d1h = consumed(1, Math.max(start, now - 3600));
  const rate = d30 > 0 ? d30 * 2 : d1h; // % par heure
  const midnight = new Date();
  midnight.setHours(0, 0, 0, 0);
  const today7 = consumed(2, midnight.getTime() / 1000);
  const resetIn = five?.resets_at ? five.resets_at - now : null;
  const eta = five && rate > 0.3 ? ((100 - five.used) / rate) * 3600 : null;
  return { c, rows, now, five, week, d15, d1h, rate, today7, resetIn, eta };
}

let lastFive = null;
function renderClaude() {
  const k = claudeCalc();
  const el = $('claude');
  el.hidden = S.config?.show_claude === false;
  const five = k?.five;
  const week = k?.week;
  const used = five?.used ?? 0;
  $('claude-ring').style.strokeDashoffset = String(87.96 * (1 - Math.min(used, 100) / 100));
  setText('claude-5h', five ? pct(five.used) : '–');
  setText('claude-7d', `7 j ${week ? pct(week.used) : '–'}`);
  const delta = $('claude-delta');
  const show = !!(k && k.d1h >= 0.5);
  delta.hidden = !show;
  if (show) {
    setText(delta, `+${Math.round(k.d1h)}`);
    delta.className = `delta${k.eta != null && k.resetIn != null && k.eta < k.resetIn ? ' crit' : k.rate >= 15 ? ' hot' : ''}`;
  }
  const worst = Math.max(used, week?.used ?? 0);
  el.classList.toggle('warn', level(worst) === 'warn');
  el.classList.toggle('crit', level(worst) === 'crit');
  const age = k?.c.updated ? Date.now() / 1000 - k.c.updated : Infinity;
  el.classList.toggle('stale', !five || age > 6 * 3600);
  el.title = five
    ? `Claude — 5 h : ${pct(five.used)}${show ? ` (${signed(k.d1h)} sur 1 h)` : ''} · 7 j : ${week ? pct(week.used) : '–'}`
    : 'Claude — aucune donnée de quota pour le moment';
  // Alerte dans l'îlot au franchissement de 80 % puis 95 %.
  if (five && lastFive != null) {
    for (const seuil of [80, 95]) {
      if (lastFive < seuil && five.used >= seuil) {
        showToast({ kind: 'claude', crit: seuil >= 95, icon: I.claude, label: 'Claude 5 h', value: five.used, text: pct(five.used) }, 4000);
      }
    }
  }
  if (five) lastFive = five.used;
}

function claudeSpark(k) {
  const s = svg('svg', { class: 'spark', viewBox: '0 0 300 100', preserveAspectRatio: 'none' });
  const t1 = k.five?.resets_at || k.now;
  const t0 = t1 - 5 * 3600;
  const x = (t) => clamp(((t - t0) / (t1 - t0)) * 300, 0, 300).toFixed(1);
  const rows = k.rows.filter((r) => r[0] >= t0);
  const used = k.five?.used ?? 0;
  const left = k.resetIn != null ? Math.max(k.resetIn, 0) / 3600 : 0;
  const proj = k.rate > 0 ? clamp(used + k.rate * left, 0, 100) : used;
  // Échelle adaptative : 3 % ne doit pas être un trait plat collé au bas du graphe.
  const top = clamp(Math.max(...rows.map((r) => r[1]), proj, used) * 1.25, 20, 100);
  const y = (v) => (96 - (clamp(v, 0, 100) / top) * 90).toFixed(1);
  s.append(svg('line', { class: 'track', x1: 0, y1: 96, x2: 300, y2: 96 }));
  const pts = rows.map((r) => [x(r[0]), y(r[1])]);
  if (pts.length) {
    const nowPt = [x(k.now), y(used)];
    pts.push(nowPt);
    const line = pts.map((p) => p.join(',')).join(' ');
    s.append(svg('polygon', { points: `${pts[0][0]},96 ${line} ${nowPt[0]},96`, fill: 'var(--claude)' }));
    s.append(svg('polyline', { points: line, stroke: 'var(--claude)' }));
    if (left > 0) s.append(svg('polyline', { class: 'proj', points: `${nowPt.join(',')} 300,${y(proj)}`, stroke: 'var(--claude)' }));
    // Point "maintenant" : ligne de longueur nulle à bout rond (reste rond malgré preserveAspectRatio=none).
    s.append(svg('line', { class: 'now', x1: nowPt[0], y1: nowPt[1], x2: nowPt[0], y2: nowPt[1], stroke: 'var(--claude)' }));
  }
  return s;
}

function claudeProjection(k) {
  if (!k.five) return null;
  if (k.five.used >= 100) return h('div', { class: 'p-hint crit' }, `Limite atteinte — réinitialisation dans ${duration(k.resetIn ?? 0)}`);
  if (k.eta == null) return h('div', { class: 'p-hint ok' }, 'Pas de consommation sur la dernière heure.');
  if (k.resetIn != null && k.eta < k.resetIn) {
    return h('div', { class: 'p-hint crit' }, `À ce rythme : 100 % dans ${duration(k.eta)}, avant la réinitialisation (${duration(k.resetIn)}).`);
  }
  return h('div', { class: 'p-hint ok' }, `À ce rythme, tu tiens jusqu'à la réinitialisation (${duration(k.resetIn ?? 0)}).`);
}

function bar(value, cls) {
  return h('div', { class: 'p-bar' }, h('b', { class: cls || level(value), style: `width:${clamp(value, 0, 100)}%` }));
}

function claudeBody(k, compact) {
  const row = (label, w, hint) => h('div', { class: 'p-row' },
    h('div', { class: 'p-line' }, h('span', { class: 'p-label' }, label), h('span', { class: 'p-value' }, w ? pct(w.used) : '–')),
    bar(w?.used ?? 0),
    h('div', { class: 'p-hint' }, hint));
  const chip = (key, value, up) => h('div', { class: 'chip-stat' }, h('div', { class: 'k' }, key), h('div', { class: `v${up ? ' up' : ''}` }, value));
  return [
    row('Session (5 h)', k.five, k.five?.resets_at ? `Réinitialisation dans ${duration(k.resetIn)}` : 'Pas encore de données'),
    h('div', { class: 'chips' },
      chip('15 min', k.d15 ? signed(k.d15) : '0 %', k.d15 > 0),
      chip('1 heure', k.d1h ? signed(k.d1h) : '0 %', k.d1h > 0),
      chip('Rythme', k.rate > 0 ? `${Math.round(k.rate * 10) / 10} %/h`.replace('.', ',') : '—', k.rate > 0)),
    claudeProjection(k),
    claudeSpark(k),
    row('Semaine (7 j)', k.week,
      `${k.today7 ? `${signed(k.today7)} aujourd'hui · ` : ''}${k.week?.resets_at ? `réinitialisation dans ${duration(k.week.resets_at - k.now)}` : ''}`),
    !compact && h('div', { class: 'p-sep' }),
  ];
}

function claudePanel() {
  const k = claudeCalc();
  if (!k) return [h('div', { class: 'p-empty' }, 'Aucune donnée Claude pour le moment.')];
  const sessions = k.c.sessions || [];
  const updated = k.c.updated ? `mis à jour il y a ${duration(k.now - k.c.updated)}` : 'aucune donnée';
  return [
    h('div', { class: 'p-head' }, h('span', { class: 'p-title' }, 'Claude Code'), h('span', { class: 'p-sub' }, updated)),
    claudeBody(k, false),
    h('div', { class: 'p-line' }, h('span', { class: 'p-label' }, 'Sessions actives'), h('span', { class: 'p-value' }, String(sessions.length))),
    sessions.length
      ? h('div', { class: 'p-list' }, sessions.slice(0, 8).map((s) => h('div', { class: 'p-item' },
          h('span', { class: 'name' }, h('i', { class: 'live' }), s.project),
          h('span', { class: 'meta' }, s.idle_s < 15 ? 'en cours' : `il y a ${duration(s.idle_s)}`))))
      : h('div', { class: 'p-empty' }, 'Aucune session Claude Code en cours.'),
    !k.five && h('div', { class: 'p-hint' }, 'Le quota est lu dans ~/.claude/monitor/quota.json, écrit par la statusline de Claude Code.'),
  ];
}

// ===================================================================== système
function meter(id, value) {
  const b = $(id);
  const t = `scaleX(${clamp(value / 100, 0, 1)})`;
  if (b.style.transform !== t) b.style.transform = t;
  const cls = level(value);
  if (b.className !== cls) b.className = cls;
}

function renderStats() {
  const s = S.stats;
  $('sys').hidden = S.config?.show_system === false;
  if (!s) return;
  for (const k of ['cpu', 'ram', 'gpu']) {
    if (s[k] == null) continue;
    hist[k].push(s[k]);
    if (hist[k].length > HISTORY) hist[k].shift();
  }
  setText('cpu', pct(s.cpu));
  setText('ram', pct(s.ram));
  meter('cpu-m', s.cpu);
  meter('ram-m', s.ram);
  $('gpu-cell').hidden = s.gpu == null;
  if (s.gpu != null) { setText('gpu', pct(s.gpu)); meter('gpu-m', s.gpu); }
  setText('down', `↓ ${bytes(s.net_down)}`);
  setText('up', `↑ ${bytes(s.net_up)}`);
  $('sys').title = `CPU ${pct(s.cpu)} · RAM ${s.ram_used_gb} / ${s.ram_total_gb} Go${s.gpu != null ? ` · GPU ${pct(s.gpu)}` : ''}`;
  const net = $('net-icon');
  setIcon(net, s.net === 'wifi' ? I.wifi : s.net === 'ethernet' ? I.ethernet : I.offline);
  net.classList.toggle('off', s.net === 'none');
  net.title = s.net === 'wifi' ? 'Wi-Fi' : s.net === 'ethernet' ? 'Ethernet' : 'Pas de connexion';
  const b = $('bat');
  b.hidden = !s.battery;
  if (s.battery) setText(b, `${s.battery.percent} %${s.battery.charging ? ' ⚡' : ''}`);
}

function spark(values, color) {
  const s = svg('svg', { class: 'spark', viewBox: `0 0 ${HISTORY - 1} 100`, preserveAspectRatio: 'none' });
  if (values.length > 1) {
    const off = HISTORY - values.length;
    const pts = values.map((v, i) => `${off + i},${100 - clamp(v, 0, 100)}`).join(' ');
    s.append(svg('polygon', { points: `${off},100 ${pts} ${HISTORY - 1},100`, fill: color }));
    s.append(svg('polyline', { points: pts, stroke: color }));
  }
  return s;
}

function sysPanel() {
  const s = S.stats;
  if (!s) return [h('div', { class: 'p-empty' }, 'Mesures en cours…')];
  const row = (label, detail, values, color) => h('div', { class: 'p-row' },
    h('div', { class: 'p-line' }, h('span', { class: 'p-label' }, label), h('span', { class: 'p-value' }, detail)),
    spark(values, color));
  return [
    h('div', { class: 'p-head' }, h('span', { class: 'p-title' }, 'Système'), h('span', { class: 'p-sub' }, '3 dernières minutes')),
    row('Processeur', pct(s.cpu), hist.cpu, 'var(--violet)'),
    row('Mémoire', `${s.ram_used_gb} / ${s.ram_total_gb} Go`, hist.ram, 'var(--green)'),
    s.gpu != null && row('Carte graphique (3D)', pct(s.gpu), hist.gpu, 'var(--claude)'),
    h('div', { class: 'p-sep' }),
    h('div', { class: 'p-line' }, h('span', { class: 'p-label' }, 'Réseau'), h('span', { class: 'p-value' }, `↓ ${bytes(s.net_down)}   ↑ ${bytes(s.net_up)}`)),
    s.battery && h('div', { class: 'p-line' }, h('span', { class: 'p-label' }, 'Batterie'), h('span', { class: 'p-value' }, `${s.battery.percent} %${s.battery.charging ? ' (en charge)' : ''}`)),
    h('button', { class: 'act', style: 'margin-top:10px;flex-direction:row;justify-content:center;width:100%', onclick: () => { closeAll(); invoke('shortcut', { name: 'taskmgr' }); } },
      icon(I.pulse), 'Gestionnaire des tâches'),
  ];
}

// ====================================================================== volume
let lastVol = null;
let volTouched = 0;

function renderVolume() {
  const v = S.volume;
  const el = $('vol-icon');
  setIcon(el, I.speaker(v));
  el.classList.toggle('off', !v || v.muted);
  $('vol-btn').title = v ? `${v.muted ? 'Son coupé' : `Volume ${v.level} %`} — molette : volume` : 'Aucune sortie audio';
  const changed = lastVol && v && (v.level !== lastVol.level || v.muted !== lastVol.muted);
  // Toast seulement pour un changement venu d'ailleurs (touches multimédia, autre app).
  if (changed && Date.now() - volTouched > 900 && island.mode !== 'full' && pop.name !== 'sound') {
    showToast({ icon: I.speaker(v), label: v.muted ? 'Son coupé' : 'Volume', value: v.muted ? 0 : v.level, text: v.muted ? '' : String(v.level) }, 1400);
  }
  lastVol = v;
  updateMasterRows();
}

/** Curseur 0-100 qui ne se laisse pas écraser par les mises à jour pendant qu'on le fait glisser. */
function slider(label, onInput) {
  const r = h('input', { type: 'range', min: '0', max: '100', step: '1', 'aria-label': label });
  r.addEventListener('input', () => {
    r._touched = Date.now();
    r.style.setProperty('--v', `${r.value}%`);
    onInput(Number(r.value));
  });
  return r;
}

function setSlider(r, value, muted) {
  r.classList.toggle('muted', !!muted);
  if (Date.now() - (r._touched || 0) < 700) return;
  r.value = String(value);
  r.style.setProperty('--v', `${value}%`);
}

// Curseurs du volume général (volet et popup son).
const masterRows = new Set();

function masterRow() {
  const value = h('span', { class: 'p-value' });
  const mute = h('button', {
    class: 'icon-btn', 'aria-label': 'Couper le son',
    onclick: () => { volTouched = Date.now(); invoke('volume', { action: 'mute' }); },
  });
  const range = slider('Volume', (v) => {
    volTouched = Date.now();
    setText(value, `${v} %`);
    invoke('volume', { action: 'set', value: v / 100 });
  });
  const row = h('div', { class: 'vol-row' }, mute, range, value);
  row._parts = { mute, range, value };
  masterRows.add(row);
  updateMasterRow(row);
  return row;
}

function updateMasterRow(row) {
  const v = S.volume;
  const { mute, range, value } = row._parts;
  setIcon(mute, I.speaker(v));
  range.disabled = !v;
  if (!v) return;
  setSlider(range, v.level, v.muted);
  if (Date.now() - (range._touched || 0) > 700) setText(value, v.muted ? 'muet' : `${v.level} %`);
}

function updateMasterRows() {
  for (const row of masterRows) {
    if (row.isConnected) updateMasterRow(row);
    else masterRows.delete(row);
  }
}

// ------------------------------------------------------------------- popup son
const mixer = { data: null, key: '', timer: 0 };
const sendAppVolume = throttle((key, level) => invoke('app_volume', { key, level }), 60);

function mixerKey(d) {
  return d ? JSON.stringify([d.devices.map((x) => [x.id, x.default]), d.apps.map((a) => a.key)]) : '';
}

async function refreshMixer() {
  let data;
  try { data = await invoke('audio_mixer'); } catch { return; }
  mixer.data = data;
  if (pop.name !== 'sound') return;
  const key = mixerKey(data);
  if (key === mixer.key) return updateAppRows();
  mixer.key = key;
  renderPop();
  setShape('pop', rectOf($('panel')));
}

function startMixer() {
  clearInterval(mixer.timer);
  refreshMixer();
  mixer.timer = setInterval(refreshMixer, 1500);
}

function stopMixer() {
  clearInterval(mixer.timer);
}

function selectOutput(dev) {
  if (dev.default) return;
  for (const d of mixer.data.devices) d.default = d === dev;
  mixer.key = mixerKey(mixer.data);
  document.querySelectorAll('#panel .dev').forEach((b, i) => b.classList.toggle('on', mixer.data.devices[i] === dev));
  invoke('audio_output', { id: dev.id }).then(refreshMixer);
}

function appRow(a, i) {
  const value = h('span', { class: 'p-value' });
  const ico = h('button', {
    class: 'app-ico', title: 'Couper / rétablir',
    onclick: () => invoke('app_volume', { key: a.key, mute: true }).then(refreshMixer),
  }, a.icon ? h('img', { src: a.icon, alt: '' }) : icon(I.speaker({ level: 60 })));
  const range = slider(`Volume de ${a.name}`, (v) => {
    setText(value, `${v} %`);
    sendAppVolume(a.key, v / 100);
  });
  const row = h('div', { class: 'mix-row', 'data-key': a.key, style: `--i:${i}` },
    ico, h('div', { class: 'mix-main' }, h('span', { class: 'mix-name' }, a.name), range), value);
  row._parts = { range, value };
  updateAppRow(row, a);
  return row;
}

function updateAppRow(row, a) {
  const { range, value } = row._parts;
  row.classList.toggle('muted', a.muted);
  setSlider(range, a.level, a.muted);
  if (Date.now() - (range._touched || 0) > 700) setText(value, a.muted ? 'muet' : `${a.level} %`);
}

function updateAppRows() {
  for (const a of mixer.data?.apps || []) {
    const row = document.querySelector(`.mix-row[data-key="${CSS.escape(a.key)}"]`);
    if (row) updateAppRow(row, a);
  }
}

function soundPanel() {
  const d = mixer.data;
  const link = (label, name) => h('button', { class: 'p-sub link', onclick: () => { closeAll(); invoke('shortcut', { name }); } }, label);
  return [
    h('div', { class: 'p-head' }, h('span', { class: 'p-title' }, 'Son'), link('Paramètres ›', 'sound')),
    masterRow(),
    d && d.devices.length > 0 && [
      h('div', { class: 'p-section' }, 'Sortie'),
      h('div', { class: 'dev-list' }, d.devices.map((dev, i) => h('button', {
        class: `dev${dev.default ? ' on' : ''}`, style: `--i:${i}`, onclick: () => selectOutput(dev),
      }, h('i', { class: 'radio' }), h('span', { class: 'dev-name' }, dev.name)))),
    ],
    d && d.apps.length > 0 && [
      h('div', { class: 'p-section' }, 'Applications'),
      h('div', { class: 'mix-list' }, d.apps.map(appRow)),
    ],
    !d && h('div', { class: 'p-empty' }, 'Lecture des périphériques audio…'),
    h('button', { class: 'p-foot', onclick: () => { closeAll(); invoke('shortcut', { name: 'mixer' }); } }, 'Mélangeur de volume Windows ›'),
  ];
}

// ===================================================================== clavier
function renderKeyboard() {
  const k = S.keyboard;
  const el = $('lang');
  const cur = k?.layouts.find((l) => l.id === k.current);
  el.hidden = !cur;
  if (!cur) return;
  const code = $('lang-code');
  if (code.textContent !== cur.code) {
    code.textContent = cur.code;
    code.classList.remove('bump');
    void code.offsetWidth; // relance l'animation
    code.classList.add('bump');
  }
  el.title = `${cur.name}${cur.detail ? ` — clavier ${cur.detail}` : ''}`;
}

function langPanel() {
  const k = S.keyboard;
  if (!k) return [];
  return [
    h('div', { class: 'p-head' }, h('span', { class: 'p-title' }, 'Clavier'), h('span', { class: 'p-sub' }, 'Win + Espace')),
    h('div', { class: 'kb-list' }, k.layouts.map((l, i) => h('button', {
      class: `kb${l.id === k.current ? ' on' : ''}`, style: `--i:${i}`,
      onclick: () => { invoke('keyboard_layout', { id: l.id }); closePop(); },
    }, h('span', { class: 'code' }, l.code), h('span', { class: 'txt' }, h('b', {}, l.name || l.code), l.detail && h('span', {}, l.detail))))),
    h('button', { class: 'p-foot', onclick: () => { closeAll(); invoke('shortcut', { name: 'keyboard' }); } }, 'Langues et claviers ›'),
  ];
}

// ======================================================================= météo
function weatherNow() {
  const d = S.weather?.data;
  return d?.current ? d : null;
}

function renderWeather() {
  const d = weatherNow();
  const el = $('weather');
  el.hidden = !d || S.config?.show_weather === false;
  if (!d) return;
  const c = d.current;
  setIcon($('w-icon'), weatherSvg(c.weather_code, c.is_day !== 0));
  setText('w-temp', `${Math.round(c.temperature_2m)}°`);
  setText('w-label', S.weather.city || wmo(c.weather_code)[0]);
  el.title = `${S.weather.city ? `${S.weather.city} — ` : ''}${wmo(c.weather_code)[0]}, ${Math.round(c.temperature_2m)}° (ressenti ${Math.round(c.apparent_temperature)}°)`;
}

function weatherCard() {
  const d = weatherNow();
  if (!d) return [h('div', { class: 'p-empty' }, 'Météo indisponible pour le moment.')];
  const c = d.current;
  const hourFmt = (iso) => `${Number(iso.slice(11, 13))} h`;
  const H = d.hourly || {};
  const hours = [];
  for (let i = 0; i < 13 && i < (H.time || []).length; i += 2) {
    hours.push(h('div', { class: 'w-h' },
      h('span', { class: 'k' }, i === 0 ? 'Maint.' : hourFmt(H.time[i])),
      h('span', { class: 'wi' }, icon(weatherSvg(H.weather_code[i], H.is_day?.[i] !== 0))),
      h('span', { class: 'v' }, `${Math.round(H.temperature_2m[i])}°`),
      h('span', { class: 'pp' }, H.precipitation_probability?.[i] >= 20 ? `${H.precipitation_probability[i]} %` : '')));
  }
  const D = d.daily || {};
  const n = D.temperature_2m_min && D.temperature_2m_max ? Math.min(6, (D.time || []).length) : 0;
  const lo = n ? Math.min(...D.temperature_2m_min.slice(0, n)) : 0;
  const hi = n ? Math.max(...D.temperature_2m_max.slice(0, n)) : 0;
  const span = Math.max(1, hi - lo);
  const dayFmt = new Intl.DateTimeFormat('fr-FR', { weekday: 'long' });
  const days = [];
  for (let i = 0; i < n; i++) {
    const a = D.temperature_2m_min[i];
    const b = D.temperature_2m_max[i];
    const name = i === 0 ? "Aujourd'hui" : dayFmt.format(new Date(`${D.time[i]}T12:00`));
    days.push(h('div', { class: 'w-d' },
      h('span', { class: 'k' }, name.charAt(0).toUpperCase() + name.slice(1)),
      h('span', { class: 'wi' }, icon(weatherSvg(D.weather_code[i]))),
      h('span', { class: 'lo' }, `${Math.round(a)}°`),
      h('div', { class: 'range' }, h('b', { style: `left:${((a - lo) / span) * 100}%;right:${100 - ((b - lo) / span) * 100}%` })),
      h('span', { class: 'hi' }, `${Math.round(b)}°`)));
  }
  return [
    h('div', { class: 'p-head' },
      h('span', { class: 'p-title' }, S.weather.city || 'Météo'),
      h('span', { class: 'p-sub' }, D.sunrise ? `☀ ${D.sunrise[0].slice(11)} · ☾ ${D.sunset[0].slice(11)}` : '')),
    h('div', { class: 'w-now' },
      h('span', { class: 'wi' }, icon(weatherSvg(c.weather_code, c.is_day !== 0))),
      h('span', { class: 'w-temp' }, `${Math.round(c.temperature_2m)}°`),
      h('div', { class: 'w-desc' },
        h('b', {}, wmo(c.weather_code)[0]),
        h('span', {}, `Ressenti ${Math.round(c.apparent_temperature)}° · Humidité ${c.relative_humidity_2m} %`),
        h('span', {}, `Vent ${Math.round(c.wind_speed_10m)} km/h`))),
    h('div', { class: 'w-hours' }, hours),
    h('div', { class: 'w-days' }, days),
  ];
}

// ======================================================== zone de notification
const trayEls = new Map();
const trayGrid = h('div', { class: 'tray-grid' });

function trayButton(t) {
  let b = trayEls.get(t.id);
  if (!b) {
    b = h('button', { class: 'tray-ico' }, h('img', { alt: '', draggable: 'false' }));
    const send = (button) => invoke('tray_click', { id: b._id, button });
    b.addEventListener('click', (e) => { if (e.detail <= 1) send('left'); });
    b.addEventListener('dblclick', () => send('double'));
    b.addEventListener('auxclick', (e) => { if (e.button === 1) send('middle'); });
    b.addEventListener('contextmenu', (e) => { e.preventDefault(); send('right'); });
    trayEls.set(t.id, b);
  }
  b._id = t.id;
  const img = b.firstChild;
  if (img.getAttribute('src') !== t.icon) img.setAttribute('src', t.icon);
  b.title = t.tip || t.app;
  b.setAttribute('aria-label', b.title);
  return b;
}

function trayPanel() {
  const list = S.tray || [];
  if (!list.length) {
    return [h('div', { class: 'tray-empty' }, S.config?.tray === false
      ? 'Zone de notification désactivée (option "tray" de la config).'
      : 'Aucune icône pour le moment.')];
  }
  const nodes = list.map(trayButton);
  const keep = new Set(nodes);
  for (const [id, b] of trayEls) if (!keep.has(b)) trayEls.delete(id);
  trayGrid.style.setProperty('--cols', String(clamp(Math.ceil(Math.sqrt(list.length * 1.4)), 3, 7)));
  nodes.forEach((n, i) => n.style.setProperty('--i', String(i)));
  trayGrid.replaceChildren(...nodes);
  return [trayGrid];
}

// ====================================================================== popups
const pop = { name: null, anchor: null, timer: 0 };
const POPS = { claude: claudePanel, sys: sysPanel, tray: trayPanel, sound: soundPanel, lang: langPanel };
// Sujets qui redessinent chaque popup (le popup son se met à jour sur place, voir refreshMixer).
const POP_DEPS = { claude: ['claude'], sys: ['stats'], tray: ['tray', 'config'], sound: [], lang: ['keyboard'] };

function renderPop() {
  const el = $('panel');
  el.replaceChildren(...POPS[pop.name]().flat().filter(Boolean));
  placePop();
}

function placePop() {
  const el = $('panel');
  const a = pop.anchor.getBoundingClientRect();
  const g = geom();
  const w = el.offsetWidth;
  const left = Math.round(clamp(a.left + a.width / 2 - w / 2, 8, innerWidth - w - 8));
  const top = g.bottom ? g.bar_top - 8 - el.offsetHeight : g.bar_top + g.bar_height + 8;
  el.style.left = `${left}px`;
  el.style.top = `${top}px`;
  el.style.transformOrigin = `${Math.round(a.left + a.width / 2 - left)}px ${g.bottom ? '100%' : '0'}`;
}

async function openPop(name, anchor) {
  if (pop.name === name) return closePop();
  stopMixer();
  closeDrawer();
  collapseIsland();
  const el = $('panel');
  if (pop.anchor) pop.anchor.classList.remove('open');
  clearTimeout(pop.timer);
  pop.name = name;
  pop.anchor = anchor;
  anchor.classList.add('open');
  el.classList.remove('open');
  el.className = `pop pop-${name}`;
  if (name === 'sound') mixer.key = mixerKey(mixer.data);
  renderPop();
  await setShape('pop', rectOf(el));
  if (pop.name === name) requestAnimationFrame(() => el.classList.add('open'));
  if (name === 'sound') startMixer();
}

function closePop() {
  if (!pop.name) return;
  stopMixer();
  const el = $('panel');
  pop.anchor.classList.remove('open');
  pop.name = null;
  pop.anchor = null;
  el.classList.remove('open');
  clearTimeout(pop.timer);
  pop.timer = setTimeout(() => {
    if (pop.name) return;
    el.replaceChildren();
    setShape('pop', null);
  }, 230);
}

async function refreshPop(topic) {
  if (!pop.name || !POP_DEPS[pop.name].includes(topic)) return;
  renderPop();
  await setShape('pop', rectOf($('panel')));
}

// ======================================================================= volet
const drawer = { open: false, timer: 0, month: null, tick: 0 };
const fmtLong = new Intl.DateTimeFormat('fr-FR', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
const fmtMonth = new Intl.DateTimeFormat('fr-FR', { month: 'long', year: 'numeric' });

function isoWeek(d) {
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const day = t.getUTCDay() || 7;
  t.setUTCDate(t.getUTCDate() + 4 - day);
  const y0 = new Date(Date.UTC(t.getUTCFullYear(), 0, 1));
  return Math.ceil(((t - y0) / 86400000 + 1) / 7);
}

function section(id, i, cls) {
  return h('section', { id, class: `d-sec ${cls || 'd-card'}`, style: `--i:${i}` });
}

function headSection() {
  const now = new Date();
  const hm = now.toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
  const ss = String(now.getSeconds()).padStart(2, '0');
  const date = fmtLong.format(now);
  return [
    h('div', { class: 'd-time' }, h('b', {}, hm), h('span', {}, ss)),
    h('div', { class: 'd-date' }, h('b', {}, date.charAt(0).toUpperCase() + date.slice(1)), ` · semaine ${isoWeek(now)}`),
  ];
}

function calendarCard() {
  const m = drawer.month;
  const first = new Date(m.getFullYear(), m.getMonth(), 1);
  const offset = (first.getDay() + 6) % 7;
  const days = new Date(m.getFullYear(), m.getMonth() + 1, 0).getDate();
  const weeks = Math.ceil((offset + days) / 7);
  const today = new Date();
  const cells = ['lun', 'mar', 'mer', 'jeu', 'ven', 'sam', 'dim'].map((d) => h('span', { class: 'dn' }, d));
  for (let i = 0; i < weeks * 7; i++) {
    const d = new Date(m.getFullYear(), m.getMonth(), 1 - offset + i);
    const cls = ['d'];
    if (d.getMonth() !== m.getMonth()) cls.push('out');
    if (i % 7 >= 5) cls.push('we');
    if (d.toDateString() === today.toDateString()) cls.push('today');
    cells.push(h('span', { class: cls.join(' ') }, String(d.getDate())));
  }
  const nav = (delta) => () => {
    drawer.month = delta ? new Date(m.getFullYear(), m.getMonth() + delta, 1) : new Date(today.getFullYear(), today.getMonth(), 1);
    $('d-cal').replaceChildren(...calendarCard());
  };
  const title = fmtMonth.format(m);
  return [
    h('div', { class: 'p-head' },
      h('button', { class: 'p-title', onclick: nav(0), title: "Revenir à aujourd'hui" }, title.charAt(0).toUpperCase() + title.slice(1)),
      h('div', { class: 'cal-nav' },
        h('button', { class: 'icon-btn', 'aria-label': 'Mois précédent', onclick: nav(-1) }, icon(I.chevL)),
        h('button', { class: 'icon-btn', 'aria-label': 'Mois suivant', onclick: nav(1) }, icon(I.chevR)))),
    h('div', { class: 'cal' }, cells),
  ];
}

function soundCard() {
  return [
    h('div', { class: 'p-head' },
      h('span', { class: 'p-title' }, 'Son'),
      h('button', { class: 'p-sub link', onclick: () => openPop('sound', $('vol-btn')) }, 'Sortie et applications ›')),
    masterRow(),
  ];
}

function actionsSection() {
  const act = (label, ico, name) => h('button', { class: 'act', onclick: () => { closeAll(); invoke('shortcut', { name }); } }, icon(ico), label);
  return [
    act('Notifications', I.bell, 'notifications'),
    act('Réglages rapides', I.toggles, 'quick'),
    act('Paramètres', I.gear, 'settings'),
    act('Tâches', I.pulse, 'taskmgr'),
  ];
}

function claudeCard() {
  const k = claudeCalc();
  if (!k) return [h('div', { class: 'p-empty' }, 'Aucune donnée Claude pour le moment.')];
  return [
    h('div', { class: 'p-head' }, h('span', { class: 'p-title' }, 'Claude'),
      h('span', { class: 'p-sub' }, `${(k.c.sessions || []).length} session(s) active(s)`)),
    claudeBody(k, true),
  ];
}

function renderDrawer() {
  const el = $('drawer');
  const secs = [
    [section('d-head', 0, 'd-head'), headSection],
    [section('d-weather', 1), weatherCard],
    [section('d-cal', 2), calendarCard],
    [section('d-claude', 3), claudeCard],
    [section('d-sound', 4), soundCard],
    [section('d-actions', 5, 'actions'), actionsSection],
  ];
  el.replaceChildren(...secs.map(([node, fn]) => { node.replaceChildren(...fn().flat().filter(Boolean)); return node; }));
}

function tickDrawer() {
  clearTimeout(drawer.tick);
  if (!drawer.open) return;
  const head = document.getElementById('d-head');
  if (head) head.replaceChildren(...headSection());
  drawer.tick = setTimeout(tickDrawer, 1000 - (Date.now() % 1000) + 10);
}

async function openDrawer() {
  if (drawer.open) return closeDrawer();
  closePop();
  collapseIsland();
  clearTimeout(drawer.timer);
  drawer.open = true;
  const now = new Date();
  drawer.month = new Date(now.getFullYear(), now.getMonth(), 1);
  const el = $('drawer');
  el.classList.remove('open');
  renderDrawer();
  $('clock').classList.add('open');
  await setShape('drawer', rectOf(el));
  if (drawer.open) requestAnimationFrame(() => el.classList.add('open'));
  tickDrawer();
}

function closeDrawer() {
  if (!drawer.open) return;
  drawer.open = false;
  clearTimeout(drawer.tick);
  $('clock').classList.remove('open');
  const el = $('drawer');
  el.classList.remove('open');
  clearTimeout(drawer.timer);
  drawer.timer = setTimeout(() => {
    if (drawer.open) return;
    el.replaceChildren();
    setShape('drawer', null);
  }, 340);
}

function refreshDrawer(topic) {
  if (!drawer.open) return;
  const map = { weather: ['d-weather', weatherCard], claude: ['d-claude', claudeCard] };
  const m = map[topic];
  const node = m && document.getElementById(m[0]);
  if (!node) return;
  node.replaceChildren(...m[1]().flat().filter(Boolean));
  setShape('drawer', rectOf($('drawer')));
}

function closeAll() {
  closePop();
  closeDrawer();
  collapseIsland();
}

// ===================================================================== horloge
const fmtTime = () => new Intl.DateTimeFormat('fr-FR', S.config?.clock_seconds
  ? { hour: '2-digit', minute: '2-digit', second: '2-digit' } : { hour: '2-digit', minute: '2-digit' });
const fmtDate = new Intl.DateTimeFormat('fr-FR', { weekday: 'short', day: 'numeric', month: 'short' });
let clockTimer = 0;
function tickClock() {
  const now = new Date();
  setText('time', fmtTime().format(now));
  setText('date', fmtDate.format(now));
  clearTimeout(clockTimer);
  const step = S.config?.clock_seconds ? 1000 : 60000;
  clockTimer = setTimeout(tickClock, step - (now.getTime() % step) + 20);
}

// ================================================================== événements
function applyConfig() {
  const c = S.config;
  if (!c) return;
  document.documentElement.style.setProperty('--alpha', String(c.opacity ?? 0.72));
  applyGeometry();
  tickClock();
}

const RENDER = {
  config: () => { applyConfig(); renderDock(); renderMedia(); renderClaude(); renderStats(); renderWeather(); },
  geometry: applyGeometry,
  pins: renderDock,
  tasks: renderDock,
  apps: renderDock,
  media: renderMedia,
  art: renderArt,
  stats: renderStats,
  claude: renderClaude,
  weather: renderWeather,
  volume: renderVolume,
  keyboard: renderKeyboard,
  tray: () => {},
};

function apply(topic, payload) {
  S[topic] = payload ?? (topic in EMPTY ? EMPTY[topic] : null);
  RENDER[topic]();
  refreshPop(topic);
  refreshDrawer(topic);
}

function wire() {
  $('start').addEventListener('click', () => { closeAll(); invoke('shortcut', { name: 'start' }); });
  $('start').addEventListener('contextmenu', (e) => { e.preventDefault(); closeAll(); invoke('shortcut', { name: 'power' }); });
  $('search').addEventListener('click', () => { closeAll(); invoke('shortcut', { name: 'search' }); });
  $('peek').addEventListener('click', () => { closeAll(); invoke('shortcut', { name: 'desktop' }); });
  $('tray-btn').addEventListener('click', () => openPop('tray', $('tray-btn')));
  $('claude').addEventListener('click', () => openPop('claude', $('claude')));
  $('sys').addEventListener('click', () => openPop('sys', $('sys')));
  $('weather').addEventListener('click', openDrawer);
  $('clock').addEventListener('click', openDrawer);
  $('clock').addEventListener('contextmenu', (e) => { e.preventDefault(); closeAll(); invoke('shortcut', { name: 'notifications' }); });
  $('net-btn').addEventListener('click', () => { closeAll(); invoke('shortcut', { name: 'quick' }); });
  $('vol-btn').addEventListener('click', () => openPop('sound', $('vol-btn')));
  $('vol-btn').addEventListener('contextmenu', (e) => { e.preventDefault(); closeAll(); invoke('shortcut', { name: 'mixer' }); });
  $('lang').addEventListener('click', () => openPop('lang', $('lang')));
  $('status').addEventListener('wheel', (e) => {
    e.preventDefault();
    invoke('volume', { action: 'step', value: e.deltaY < 0 ? 0.02 : -0.02 });
  }, { passive: false });
  for (const id of ['claude', 'sys', 'weather', 'clock', 'lang']) {
    $(id).addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); $(id).click(); } });
  }

  const isl = $('island');
  isl.addEventListener('click', (e) => {
    if (e.target.closest('[data-media]')) return;
    if (island.mode === 'compact') expandIsland();
    else if (island.mode === 'full') collapseIsland();
  });
  isl.addEventListener('mouseenter', () => { if (island.peek) { island.peek = false; clearTimeout(island.peekTimer); } });
  isl.addEventListener('wheel', (e) => {
    e.preventDefault();
    invoke('volume', { action: 'step', value: e.deltaY < 0 ? 0.02 : -0.02 });
  }, { passive: false });
  isl.querySelectorAll('[data-media]').forEach((b) => b.addEventListener('click', () => invoke('media_control', { action: b.dataset.media })));

  document.addEventListener('mousedown', (e) => {
    const t = e.target;
    if (pop.name && !$('panel').contains(t) && !pop.anchor.contains(t)) closePop();
    if (drawer.open && !$('drawer').contains(t) && !$('clock').contains(t) && !$('weather').contains(t)) closeDrawer();
    if (island.expanded && !isl.contains(t)) collapseIsland();
  });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape') closeAll(); });
  document.addEventListener('contextmenu', (e) => e.preventDefault());
  window.addEventListener('blur', closeAll);
  window.addEventListener('resize', () => { layoutIsland(); if (pop.name) placePop(); });
  // Progression du morceau : uniquement quand l'îlot est déployé et qu'une lecture est en cours.
  setInterval(() => { if (island.mode === 'full' && S.media?.playing) tickMedia(); }, 1000);
}

async function init() {
  wire();
  const received = new Set();
  await Promise.all(TOPICS.map((t) => listen(t, (e) => { received.add(t); apply(t, e.payload); })));
  const snap = await invoke('snapshot');
  for (const t of TOPICS) if (t in snap && !received.has(t)) apply(t, snap[t]);
  applyGeometry();
  tickClock();
  requestAnimationFrame(() => document.body.classList.add('ready'));
}

init();
