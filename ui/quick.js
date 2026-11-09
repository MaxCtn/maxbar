// Paramètres rapides maison : connexion, Wi-Fi, tuiles, curseurs, alimentation.
// Remplace le volet Win+A : rien d'ici ne doit renvoyer vers l'ancienne interface de Windows.
(() => {
  'use strict';

  const RATE_POINTS = 60; // 2 min d'historique à 2 s
  const NET_EVERY = 3000;
  const WIFI_EVERY = 5000;

  const ICON = {
    bluetooth: '<svg viewBox="0 0 20 20"><path d="m6 6.6 7.6 6.8L10 16.6V3.4l3.6 3.2L6 13.4"/></svg>',
    gauge: '<svg viewBox="0 0 20 20"><path d="M4.1 14.6a7 7 0 1 1 11.8 0"/><path d="m10 11.4 3.2-3.6"/><circle cx="10" cy="11.6" r="1"/></svg>',
    moon: '<svg viewBox="0 0 20 20"><path d="M16 12.4A6.6 6.6 0 0 1 7.6 4a6.6 6.6 0 1 0 8.4 8.4z"/></svg>',
    project: '<svg viewBox="0 0 20 20"><rect x="2.5" y="3.5" width="10" height="7.2" rx="1.4"/><path d="M7.5 13.2v1.4c0 .8.6 1.4 1.4 1.4h7.2c.8 0 1.4-.6 1.4-1.4V9.6c0-.8-.6-1.4-1.4-1.4H15"/></svg>',
    cast: '<svg viewBox="0 0 20 20"><path d="M2.8 7.2V5.4c0-.9.7-1.6 1.6-1.6h11.2c.9 0 1.6.7 1.6 1.6v9.2c0 .9-.7 1.6-1.6 1.6h-4.2"/><path d="M2.8 10.2a6 6 0 0 1 6 6M2.8 13.2a3 3 0 0 1 3 3"/><circle cx="3.1" cy="16" r=".5"/></svg>',
    snip: '<svg viewBox="0 0 20 20"><circle cx="5.8" cy="14.2" r="2.4"/><circle cx="14.2" cy="14.2" r="2.4"/><path d="m7.6 12.6 7-9M12.4 12.6l-7-9"/></svg>',
    sun: '<svg viewBox="0 0 20 20"><circle cx="10" cy="10" r="3.2"/><path d="M10 2.6v1.6M10 15.8v1.6M2.6 10h1.6M15.8 10h1.6M4.8 4.8l1.1 1.1M14.1 14.1l1.1 1.1M4.8 15.2l1.1-1.1M14.1 5.9l1.1-1.1"/></svg>',
    lock: '<svg viewBox="0 0 20 20"><rect x="4.6" y="8.8" width="10.8" height="8" rx="1.8"/><path d="M7.2 8.8V6.6a2.8 2.8 0 0 1 5.6 0v2.2"/></svg>',
    power: '<svg viewBox="0 0 20 20"><path d="M10 2.8v6.6"/><path d="M6.1 5.3a6.4 6.4 0 1 0 7.8 0"/></svg>',
    logoff: '<svg viewBox="0 0 20 20"><path d="M8.2 3.6H5.4c-.9 0-1.6.7-1.6 1.6v9.6c0 .9.7 1.6 1.6 1.6h2.8"/><path d="m12.6 6.6 3.4 3.4-3.4 3.4M16 10H8"/></svg>',
    restart: '<svg viewBox="0 0 20 20"><path d="M16.2 10a6.2 6.2 0 1 1-1.9-4.5"/><path d="M16.4 3.4v3.8h-3.8"/></svg>',
    copy: '<svg viewBox="0 0 20 20"><rect x="7" y="7" width="9.6" height="9.6" rx="1.8"/><path d="M13 4.6c-.1-.6-.6-1-1.2-1H5c-.8 0-1.4.6-1.4 1.4v6.8c0 .6.4 1.1 1 1.2"/></svg>',
    check: '<svg viewBox="0 0 20 20"><path d="m4.6 10.4 3.4 3.4 7.4-7.6"/></svg>',
    eye: '<svg viewBox="0 0 20 20"><path d="M2.4 10S5.2 4.8 10 4.8s7.6 5.2 7.6 5.2-2.8 5.2-7.6 5.2S2.4 10 2.4 10z"/><circle cx="10" cy="10" r="2.3"/></svg>',
    chevD: '<svg viewBox="0 0 20 20"><path d="m5.5 8 4.5 4.5L14.5 8"/></svg>',
  };

  const bars = (signal) => (signal == null || signal >= 70 ? 3 : signal >= 45 ? 2 : signal >= 20 ? 1 : 0);

  /** Barres Wi-Fi : les arcs au-delà du niveau de signal sont estompés. */
  function wifiIcon(signal) {
    const lv = bars(signal);
    const arc = (d, n) => `<path d="${d}"${lv < n ? ' class="dim"' : ''}/>`;
    return `<svg viewBox="0 0 20 20">${arc('M2.6 7.7a10.6 10.6 0 0 1 14.8 0', 3)}${arc('M5.2 10.4a6.8 6.8 0 0 1 9.6 0', 2)}${
      arc('M7.9 13.1a3 3 0 0 1 4.2 0', 1)}<circle cx="10" cy="15.7" r="0.6"/></svg>`;
  }

  function batteryIcon(percent) {
    const w = ((Math.round(clamp(percent, 0, 100) / 10) * 9.9) / 10).toFixed(1);
    return `<svg viewBox="0 0 20 20"><rect x="2.5" y="6" width="13.5" height="8" rx="2"/><path d="M18 8.8v2.4"/>${
      w > 0 ? `<rect class="lvl" x="4.3" y="7.8" width="${w}" height="4.4" rx=".8"/>` : ''}</svg>`;
  }

  const Q = {
    built: false,
    open: false,
    // Change à chaque ouverture et fermeture : les réponses d'une session précédente sont ignorées.
    gen: 0,
    timers: [],
    nodes: [],
    net: null,
    wifi: null,
    radios: [],
    plans: [],
    monitors: [],
    netBusy: false,
    wifiBusy: false,
    wifiEpoch: 0,
    scanning: false,
    radioBusy: {},
    expanded: false,
    detailsKey: '',
    speedEls: [],
    join: null,
    armed: null,
    armTimer: 0,
    rates: { down: [], up: [] },
  };
  const N = {};

  const resized = () => { if (Q.open) popResized(); };

  function openSettings(uri) {
    closeAll();
    invoke('launch', { path: uri }).catch(() => {});
  }

  /** Remplace les enfants de `parent` par `nodes` sans recréer ceux qui y sont déjà. */
  function reorder(parent, nodes) {
    nodes.forEach((n, i) => { if (parent.children[i] !== n) parent.insertBefore(n, parent.children[i] || null); });
    while (parent.children.length > nodes.length) parent.lastChild.remove();
  }

  // ================================================================ connexion
  function speedText(bps) {
    if (!bps) return '';
    const [v, unit] = bps >= 1e9 ? [bps / 1e9, 'Gbit/s'] : bps >= 1e6 ? [bps / 1e6, 'Mbit/s'] : [bps / 1e3, 'kbit/s'];
    return `${String(Math.round(v * 10) / 10).replace('.', ',')} ${unit}`;
  }

  const SUBTITLE = {
    internet: 'Connecté · Accès à Internet',
    constrained: 'Connecté · Accès limité',
    local: "Connecté · Pas d'accès à Internet",
    none: 'Pas d’accès au réseau',
    unknown: 'Connecté',
  };

  function renderConn() {
    const n = Q.net;
    const ad = n?.adapters?.find((a) => a.primary) || n?.adapters?.[0] || null;
    let kind;
    let title;
    let sub;
    if (!n) {
      // Avant la première réponse : le type de lien publié par les stats évite une carte vide.
      kind = S.stats?.net || 'none';
      title = kind === 'ethernet' ? 'Ethernet' : kind === 'wifi' ? 'Wi-Fi' : 'Pas de connexion';
      sub = 'Lecture de l’état du réseau…';
    } else if (!ad) {
      kind = 'none';
      title = 'Pas de connexion';
      sub = 'Aucune carte réseau active';
    } else {
      kind = ad.kind;
      title = ad.kind === 'wifi' ? n.ssid || 'Wi-Fi' : ad.kind === 'ethernet' ? 'Ethernet' : ad.name;
      sub = SUBTITLE[n.connectivity] || SUBTITLE.unknown;
    }
    const state = kind === 'none' ? 'offline' : !n || n.connectivity === 'internet' || n.connectivity === 'unknown' ? 'ok' : 'limited';
    setIcon(N.badge, kind === 'wifi' ? wifiIcon(n?.signal) : kind === 'none' ? I.offline : I.ethernet);
    setText(N.title, title);
    setText(N.sub, sub);
    N.conn.dataset.state = state;
    N.head.title = ad ? [ad.description, speedText(ad.speed)].filter(Boolean).join(' · ') : '';
    if (Q.expanded) renderDetails();
  }

  function renderRates(s) {
    setText(N.down, s ? `↓ ${bytes(s.net_down)}` : '↓ –');
    setText(N.up, s ? `↑ ${bytes(s.net_up)}` : '↑ –');
    const { down, up } = Q.rates;
    if (down.length < 2) {
      for (const el of [N.area, N.lineDown, N.lineUp]) el.setAttribute('points', '');
      return;
    }
    // Plancher de 64 Ko/s : au repos, le bruit de fond ne doit pas remplir le graphe.
    const top = Math.max(65536, ...down, ...up) * 1.15;
    const off = RATE_POINTS - down.length;
    const pts = (arr) => arr.map((v, i) => `${off + i},${(40 - (v / top) * 38).toFixed(1)}`).join(' ');
    const d = pts(down);
    N.area.setAttribute('points', `${off},40 ${d} ${RATE_POINTS - 1},40`);
    N.lineDown.setAttribute('points', d);
    N.lineUp.setAttribute('points', pts(up));
  }

  function copyButton(text) {
    const b = h('button', { class: 'q-copy', title: `Copier ${text}`, 'aria-label': `Copier ${text}` });
    setIcon(b, ICON.copy);
    b.addEventListener('click', async () => {
      try { await invoke('clipboard_set', { text }); } catch { return; }
      setIcon(b, ICON.check);
      b.classList.add('done');
      setTimeout(() => { setIcon(b, ICON.copy); b.classList.remove('done'); }, 1400);
    });
    return b;
  }

  function adapterBlock(a, i, all) {
    const row = (k, values, extra) => h('div', { class: 'q-kv' },
      h('span', { class: 'k' }, k),
      h('span', { class: 'v' }, values.map((v) => h('span', {}, v))),
      extra || null);
    const speed = h('span', {}, speedText(a.speed));
    Q.speedEls.push([a.name, speed]);
    const v4 = a.ipv4[0]?.split('/')[0];
    return h('div', { class: 'q-ad', style: `--i:${i}` },
      h('div', { class: 'q-ad-head' },
        h('b', {}, a.name),
        a.primary && all.length > 1 && h('span', { class: 'q-tag' }, 'Principale')),
      a.description && h('div', { class: 'q-ad-desc' }, a.description),
      a.ipv4.length > 0 && row('IPv4', a.ipv4, v4 && copyButton(v4)),
      a.ipv6.length > 0 && row('IPv6', a.ipv6),
      a.gateways.length > 0 && row(a.gateways.length > 1 ? 'Passerelles' : 'Passerelle', a.gateways),
      a.dns.length > 0 && row('DNS', a.dns),
      a.mac && row('Adresse MAC', [a.mac]),
      a.speed && h('div', { class: 'q-kv' }, h('span', { class: 'k' }, 'Vitesse du lien'), h('span', { class: 'v' }, speed)));
  }

  function renderDetails() {
    const ads = Q.net?.adapters || [];
    // Le débit d'un lien Wi-Fi bouge sans cesse : mis à jour sur place, il ne reconstruit pas les détails.
    const key = Q.net ? JSON.stringify(ads.map((a) => ({ ...a, speed: !!a.speed }))) : 'attente';
    if (key === Q.detailsKey) {
      for (const [name, el] of Q.speedEls) setText(el, speedText(ads.find((a) => a.name === name)?.speed));
      return;
    }
    Q.detailsKey = key;
    Q.speedEls = [];
    if (!Q.net) N.details.replaceChildren(h('div', { class: 'p-empty' }, 'Lecture des cartes réseau…'));
    else if (!ads.length) N.details.replaceChildren(h('div', { class: 'p-empty' }, 'Aucune carte réseau active.'));
    else N.details.replaceChildren(...ads.map((a, i) => adapterBlock(a, i, ads)));
  }

  /** Relance l'animation d'apparition d'un bloc qui vient d'être déplié. */
  function reveal(el) {
    el.classList.remove('reveal');
    void el.offsetWidth;
    el.classList.add('reveal');
    clearTimeout(el._reveal);
    el._reveal = setTimeout(() => el.classList.remove('reveal'), 700);
  }

  function setExpanded(on) {
    Q.expanded = on;
    N.conn.classList.toggle('expanded', on);
    N.head.setAttribute('aria-expanded', String(on));
    N.details.hidden = !on;
    if (on) {
      renderDetails();
      reveal(N.details);
    }
  }

  // ==================================================================== Wi-Fi
  const netRows = new Map();

  function netRow(n) {
    let row = netRows.get(n.ssid);
    if (!row) {
      const ico = h('span', { class: 'q-nico' });
      const name = h('span', { class: 'q-nname' });
      const tag = h('span', { class: 'q-ok' }, 'Connecté');
      const sub = h('span', { class: 'q-nsub' });
      const lock = h('span', { class: 'q-lock', title: 'Réseau sécurisé' });
      setIcon(lock, ICON.lock);
      const main = h('button', { class: 'q-nmain' }, ico, h('span', { class: 'q-ntxt' }, h('span', { class: 'q-nline1' }, name, tag), sub), lock);
      const act = h('button', { class: 'q-btn ghost q-nact', onclick: disconnectWifi }, 'Déconnecter');
      const join = h('div', { class: 'q-join', hidden: true });
      row = h('div', { class: 'q-net' }, h('div', { class: 'q-nrow' }, main, act), join);
      row._p = { ico, name, tag, sub, lock, main, act, join };
      main.addEventListener('click', () => onNetClick(row._net));
      // Retirée après coup : une ligne déplacée par le tri rejouerait sinon son apparition.
      row.addEventListener('animationend', (e) => { if (e.target === row) row.classList.remove('enter'); });
      if (Q.open) row.classList.add('enter');
      netRows.set(n.ssid, row);
    }
    row._net = n;
    const p = row._p;
    const busy = Q.join?.ssid === n.ssid && Q.join.busy;
    setIcon(p.ico, wifiIcon(n.signal));
    setText(p.name, n.ssid);
    p.tag.hidden = !n.connected;
    p.lock.hidden = !n.secure;
    p.act.hidden = !n.connected;
    setText(p.sub, busy ? 'Connexion…' : [n.secure ? 'Sécurisé' : 'Ouvert', n.saved && !n.connected && 'enregistré'].filter(Boolean).join(' · '));
    row.classList.toggle('connected', n.connected);
    row.classList.toggle('busy', busy);
    p.main.title = `${n.ssid} — signal ${n.signal} %`;
    return row;
  }

  function renderWifi() {
    const w = Q.wifi;
    N.wifi.hidden = !w?.interface;
    if (!w?.interface) return;
    N.wifiSwitch.setAttribute('aria-checked', String(!!w.radio));
    N.wifiSwitch.disabled = !!Q.radioBusy.wifi;
    // Tri par nombre de barres puis par position actuelle : un signal qui fluctue ne fait pas sauter la liste.
    const was = new Map([...N.wifiList.children].map((r, i) => [r._net?.ssid, i]));
    const list = (w.radio ? [...w.networks] : []).sort((a, b) => (b.connected - a.connected)
      || (bars(b.signal) - bars(a.signal)) || ((was.get(a.ssid) ?? 1e4) - (was.get(b.ssid) ?? 1e4)));
    const nodes = list.map(netRow);
    // Un champ de mot de passe ouvert perdrait le focus si sa ligne était déplacée : on fige l'ordre.
    if (!Q.join) {
      const keep = new Set(nodes);
      for (const [k, row] of netRows) if (!keep.has(row)) netRows.delete(k);
      reorder(N.wifiList, nodes);
    }
    N.wifiList.hidden = !list.length;
    N.wifiStatus.hidden = list.length > 0 && !N.wifiStatus.classList.contains('err');
    if (!N.wifiStatus.classList.contains('err')) {
      setText(N.wifiStatus, !w.radio ? 'Wi-Fi désactivé.' : Q.scanning ? 'Recherche des réseaux…' : 'Aucun réseau à portée.');
    }
  }

  function wifiNotice(text) {
    N.wifiStatus.classList.add('err');
    N.wifiStatus.hidden = false;
    setText(N.wifiStatus, text);
    clearTimeout(N.wifiStatus._t);
    N.wifiStatus._t = setTimeout(() => { N.wifiStatus.classList.remove('err'); renderWifi(); resized(); }, 4000);
    resized();
  }

  function onNetClick(n) {
    if (!n || n.connected || Q.join?.busy) return;
    if (Q.join?.ssid === n.ssid) return closeJoin(true);
    if (n.password && !n.saved) return openJoin(n, null);
    connectWifi(n, null);
  }

  /** Zone sous un réseau : mot de passe (si le réseau en demande un) et dernière erreur. */
  function openJoin(n, error) {
    closeJoin(false);
    const row = netRows.get(n.ssid);
    if (!row) return;
    const err = h('div', { class: 'q-err', hidden: !error }, error || '');
    let form = null;
    const parts = [];
    if (n.password) {
      const input = h('input', {
        type: 'password', class: 'q-input', placeholder: 'Mot de passe du réseau',
        autocomplete: 'off', spellcheck: 'false', 'aria-label': `Mot de passe de ${n.ssid}`,
      });
      const eye = h('button', { class: 'q-eye', title: 'Afficher le mot de passe', 'aria-label': 'Afficher le mot de passe' });
      setIcon(eye, ICON.eye);
      eye.addEventListener('click', () => {
        input.type = input.type === 'password' ? 'text' : 'password';
        eye.classList.toggle('on', input.type === 'text');
        input.focus();
      });
      const go = h('button', { class: 'q-btn' }, 'Se connecter');
      const submit = () => { if (input.value) connectWifi(n, input.value); else input.focus(); };
      go.addEventListener('click', submit);
      input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') { e.preventDefault(); submit(); }
        // Échap referme le champ seulement, pas tout le volet.
        if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeJoin(true); }
      });
      const cancel = h('button', { class: 'q-btn ghost', onclick: () => closeJoin(true) }, 'Annuler');
      form = { input, go, cancel, eye };
      parts.push(h('div', { class: 'q-pass' }, input, eye));
      parts.push(h('div', { class: 'q-join-act' }, err, cancel, go));
    } else {
      parts.push(h('div', { class: 'q-join-act' }, err, h('button', { class: 'q-btn ghost', onclick: () => closeJoin(true) }, 'Fermer'),
        h('button', { class: 'q-btn', onclick: () => connectWifi(n, null) }, 'Réessayer')));
    }
    row._p.join.replaceChildren(...parts);
    row._p.join.hidden = false;
    row.classList.add('open');
    reveal(row._p.join);
    Q.join = { ssid: n.ssid, busy: false, form };
    resized();
    requestAnimationFrame(() => {
      // La liste défile : le champ ouvert en bas ne doit pas rester caché.
      row.scrollIntoView({ block: 'nearest' });
      if (form) form.input.focus({ preventScroll: true });
    });
  }

  function closeJoin(refresh) {
    if (!Q.join) return;
    const row = netRows.get(Q.join.ssid);
    if (row) {
      row._p.join.hidden = true;
      row._p.join.replaceChildren();
      row.classList.remove('open', 'busy');
    }
    Q.join = null;
    if (refresh) {
      renderWifi();
      resized();
    }
  }

  async function connectWifi(n, password) {
    if (Q.join?.busy) return;
    if (Q.join?.ssid !== n.ssid) closeJoin(false);
    Q.join = Q.join || { ssid: n.ssid, busy: false, form: null };
    Q.join.busy = true;
    const f = Q.join.form;
    if (f) {
      for (const el of [f.input, f.go, f.cancel, f.eye]) el.disabled = true;
      setText(f.go, 'Connexion…');
    }
    renderWifi();
    const gen = Q.gen;
    let error = null;
    try {
      await invoke('wifi_connect', { ssid: n.ssid, password: password || null });
    } catch (e) {
      error = typeof e === 'string' && e ? e : 'Connexion impossible.';
    }
    if (gen !== Q.gen) return;
    const mine = Q.join?.ssid === n.ssid;
    if (!error) {
      if (mine) closeJoin(false);
      // Connexion confirmée par le service WLAN : la liste peut l'afficher sans attendre le prochain relevé.
      Q.wifiEpoch++;
      for (const x of Q.wifi?.networks || []) x.connected = x.ssid === n.ssid;
      renderWifi();
      renderTiles();
      resized();
      refreshNet();
      setTimeout(() => { if (gen === Q.gen) refreshWifi(false); }, 800);
      return;
    }
    if (!mine) return wifiNotice(error);
    Q.join.busy = false;
    // Un réseau enregistré dont la clé a changé redemande le mot de passe.
    openJoin(n.password ? { ...n, saved: false } : n, error);
    renderWifi();
  }

  async function disconnectWifi(e) {
    e.stopPropagation();
    const b = e.currentTarget;
    b.disabled = true;
    setText(b, 'Déconnexion…');
    try { await invoke('wifi_disconnect'); } catch (err) { wifiNotice(String(err)); }
    Q.wifiEpoch++;
    b.disabled = false;
    setText(b, 'Déconnecter');
    refreshWifi(false);
    refreshNet();
  }

  const wifiSsid = () => Q.wifi?.networks?.find((n) => n.connected)?.ssid || Q.net?.ssid || '';

  // ==================================================================== tuiles
  const TILES = [
    { key: 'wifi', label: 'Wi-Fi', icon: () => wifiIcon(null), radio: 'wifi', settings: 'ms-settings:network-wifi' },
    { key: 'bluetooth', label: 'Bluetooth', icon: ICON.bluetooth, radio: 'bluetooth', settings: 'ms-settings:bluetooth' },
    { key: 'power', label: 'Mode d’alimentation', icon: ICON.gauge, more: true, settings: 'ms-settings:powersleep' },
    { key: 'night', label: 'Éclairage nocturne', icon: ICON.moon, sub: 'Réglages', open: 'ms-settings:nightlight' },
    { key: 'project', label: 'Projeter', icon: ICON.project, sub: 'Win + P', shortcut: 'project', settings: 'ms-settings:display' },
    { key: 'cast', label: 'Caster', icon: ICON.cast, sub: 'Win + K', shortcut: 'cast', settings: 'ms-settings:connecteddevices' },
    { key: 'snip', label: 'Capture d’écran', icon: ICON.snip, sub: 'Win + Maj + S', shortcut: 'snip' },
    { key: 'taskmgr', label: 'Gestionnaire des tâches', icon: () => I.pulse, sub: 'Ctrl + Maj + Échap', taskmgr: true },
    { key: 'settings', label: 'Paramètres', icon: () => I.gear, sub: 'Windows', open: 'ms-settings:' },
  ];
  const tileEls = new Map();
  const radioOf = (kind) => Q.radios.find((r) => r.kind === kind);

  function tileEl(d) {
    let t = tileEls.get(d.key);
    if (t) return t;
    const ico = h('span', { class: 'q-tico' });
    const sub = h('span', { class: 'q-tsub' });
    t = h('button', { class: `q-tile${d.radio ? ' toggle' : ''}`, 'data-key': d.key },
      ico, h('b', { class: 'q-tlabel' }, d.label), sub,
      d.more && h('span', { class: 'q-tmore' }, icon(ICON.chevD)));
    t._p = { ico, sub };
    t.addEventListener('click', () => onTile(d));
    t.addEventListener('contextmenu', (e) => { e.preventDefault(); if (d.settings) openSettings(d.settings); });
    if (d.more) t.setAttribute('aria-expanded', 'false');
    tileEls.set(d.key, t);
    return t;
  }

  function renderTiles() {
    const visible = TILES.filter((d) => !d.radio || radioOf(d.radio));
    const nodes = visible.map((d, i) => {
      const t = tileEl(d);
      t.style.setProperty('--i', String(i));
      const r = d.radio && radioOf(d.radio);
      let sub = d.sub || '';
      if (r) {
        t.classList.toggle('on', r.on);
        t.setAttribute('aria-pressed', String(r.on));
        sub = !r.on ? 'Désactivé' : (d.radio === 'wifi' && wifiSsid()) || 'Activé';
      }
      if (d.key === 'power') sub = Q.plans.find((p) => p.active)?.name || '—';
      t.classList.toggle('err', !!t._notice);
      setText(t._p.sub, t._notice || sub);
      setIcon(t._p.ico, typeof d.icon === 'function' ? d.icon() : d.icon);
      t.title = t._notice || (d.settings ? `${d.label} — clic droit : paramètres` : d.label);
      return t;
    });
    reorder(N.tiles, nodes);
  }

  /** Message bref à la place du sous-libellé d'une tuile (échec d'une bascule...). */
  function tileNotice(key, text) {
    const t = tileEls.get(key);
    if (!t || !t.isConnected) return key === 'wifi' && wifiNotice(text);
    t._notice = text;
    renderTiles();
    clearTimeout(t._noticeTimer);
    t._noticeTimer = setTimeout(() => { t._notice = null; renderTiles(); }, 3500);
  }

  function onTile(d) {
    if (d.radio) {
      const r = radioOf(d.radio);
      if (r) setRadio(d.radio, !r.on);
    } else if (d.more) {
      togglePlans();
    } else if (d.open) {
      openSettings(d.open);
    } else if (d.shortcut) {
      closeAll();
      invoke('quick_shortcut', { name: d.shortcut }).catch(() => {});
    } else if (d.taskmgr) {
      closeAll();
      invoke('shortcut', { name: 'taskmgr' }).catch(() => {});
    }
  }

  async function setRadio(kind, on) {
    if (Q.radioBusy[kind]) return;
    Q.radioBusy[kind] = true;
    const r = radioOf(kind);
    if (r) r.on = on;
    if (kind === 'wifi' && Q.wifi) Q.wifi.radio = on;
    renderTiles();
    renderWifi();
    resized(); // la liste Wi-Fi vient peut-être de disparaître
    const gen = Q.gen;
    let error = null;
    try { await invoke('quick_set_radio', { kind, on }); } catch (e) { error = String(e); }
    Q.radioBusy[kind] = false;
    if (kind === 'wifi') Q.wifiEpoch++;
    if (gen !== Q.gen) return;
    try { Q.radios = (await invoke('quick_radios')) || []; } catch { /* état précédent conservé */ }
    if (gen !== Q.gen) return;
    renderTiles();
    if (error) tileNotice(kind, error);
    if (kind === 'wifi') {
      if (on) Q.scanning = true;
      await refreshWifi(on);
      setTimeout(() => { Q.scanning = false; if (gen === Q.gen) refreshWifi(false); }, 3500);
    }
    resized();
  }

  // ======================================================= modes d'alimentation
  function togglePlans(force) {
    const open = force ?? N.plans.hidden;
    N.plans.hidden = !open;
    const t = tileEls.get('power');
    t?.classList.toggle('open', open);
    t?.setAttribute('aria-expanded', String(open));
    if (open) renderPlans();
    resized();
    if (open) {
      reveal(N.plans);
      requestAnimationFrame(() => N.plans.scrollIntoView({ block: 'nearest', behavior: 'smooth' }));
    }
  }

  function renderPlans() {
    if (N.plans.hidden) return;
    // Même liste qu'avant : seul l'état change, pour que le bouton radio s'anime au lieu d'être recréé.
    const key = Q.plans.map((p) => `${p.guid}:${p.name}`).join('|');
    if (key === N.plans._key) {
      for (const b of N.plans.querySelectorAll('.dev')) {
        const on = !!Q.plans.find((p) => p.guid === b._guid)?.active;
        b.classList.toggle('on', on);
        b.setAttribute('aria-checked', String(on));
      }
      return;
    }
    N.plans._key = key;
    const items = Q.plans.length
      ? Q.plans.map((p, i) => {
        const b = h('button', {
          class: `dev${p.active ? ' on' : ''}`, style: `--i:${i}`, role: 'radio', 'aria-checked': String(p.active),
          onclick: () => setPlan(p.guid),
        }, h('i', { class: 'radio' }), h('span', { class: 'dev-name' }, p.name));
        b._guid = p.guid;
        return b;
      })
      : [h('div', { class: 'p-empty' }, 'Aucun mode d’alimentation trouvé.')];
    N.plans.replaceChildren(...items,
      h('button', { class: 'p-sub link q-more', onclick: () => openSettings('ms-settings:powersleep') }, 'Alimentation et mise en veille ›'));
  }

  async function setPlan(guid) {
    const p = Q.plans.find((x) => x.guid === guid);
    if (!p || p.active) return;
    for (const x of Q.plans) x.active = x === p;
    renderPlans();
    renderTiles();
    const gen = Q.gen;
    try { await invoke('quick_set_power_plan', { guid }); } catch (e) { tileNotice('power', String(e)); }
    try { Q.plans = (await invoke('quick_power_plans')) || []; } catch { return; }
    if (gen !== Q.gen) return;
    renderPlans();
    renderTiles();
  }

  // ================================================================== curseurs
  const brightRows = new Map();

  function renderBrightness() {
    const list = Q.monitors.filter((m) => m.supported);
    N.bright.hidden = !list.length;
    // Plusieurs écrans branchés : chaque curseur dit lequel il règle, même s'il est le seul à répondre.
    const multi = Q.monitors.length > 1;
    const nodes = list.map((m) => {
      let row = brightRows.get(m.id);
      if (!row) {
        const send = throttle((value) => invoke('quick_set_brightness', { id: m.id, value }).catch(() => {}), 90);
        const value = h('span', { class: 'p-value' });
        const range = slider(`Luminosité de ${m.name}`, (v) => {
          setText(value, `${v} %`);
          send(v);
          // Le volet rouvert affiche la dernière valeur choisie, pas celle lue au chargement.
          const cur = Q.monitors.find((x) => x.id === m.id);
          if (cur) cur.brightness = v;
        });
        const sun = h('span', { class: 'q-sun', 'aria-hidden': 'true' });
        setIcon(sun, ICON.sun);
        const name = h('div', { class: 'q-bname' });
        row = h('div', { class: 'q-brow' }, name, h('div', { class: 'vol-row' }, sun, range, value));
        row._p = { range, value, name };
        brightRows.set(m.id, row);
      }
      const { range, value, name } = row._p;
      name.hidden = !multi;
      setText(name, m.name);
      row.title = `Luminosité — ${m.name}`;
      setSlider(range, m.brightness);
      if (Date.now() - (range._touched || 0) > 700) setText(value, `${m.brightness} %`);
      return row;
    });
    for (const [id] of brightRows) if (!list.some((m) => m.id === id)) brightRows.delete(id);
    reorder(N.bright, nodes);
  }

  // ===================================================================== pied
  const POWER = [
    { action: 'lock', label: 'Verrouiller', icon: ICON.lock },
    { action: 'sleep', label: 'Mettre en veille', icon: ICON.moon },
    { action: 'logoff', label: 'Se déconnecter', icon: ICON.logoff, confirm: 'Cliquer encore pour se déconnecter' },
    { action: 'restart', label: 'Redémarrer', icon: ICON.restart, confirm: 'Cliquer encore pour redémarrer' },
    { action: 'shutdown', label: 'Arrêter', icon: ICON.power, confirm: 'Cliquer encore pour arrêter' },
  ];

  function renderBattery(s) {
    const b = s?.battery;
    N.bat.hidden = !b;
    if (!b) return;
    setIcon(N.batIco, batteryIcon(b.percent));
    setText(N.batTxt, `${b.percent} %${b.charging ? ' ⚡' : ''}`);
    N.bat.title = b.charging ? 'Batterie en charge' : 'Sur batterie';
    N.bat.classList.toggle('low', b.percent <= 15 && !b.charging);
  }

  function disarm() {
    clearTimeout(Q.armTimer);
    Q.armed = null;
    for (const b of N.powerMenu?.children || []) {
      b.classList.remove('armed');
      setText(b._label, b._item.label);
    }
  }

  function onPower(b) {
    const it = b._item;
    if (it.confirm && Q.armed !== it.action) {
      disarm();
      Q.armed = it.action;
      b.classList.add('armed');
      setText(b._label, it.confirm);
      Q.armTimer = setTimeout(disarm, 4000);
      return;
    }
    disarm();
    closeAll();
    invoke('power_action', { action: it.action }).catch(() => {});
  }

  function togglePowerMenu() {
    const open = N.powerMenu.hidden;
    disarm();
    N.powerMenu.hidden = !open;
    N.powerBtn.classList.toggle('open', open);
    N.powerBtn.setAttribute('aria-expanded', String(open));
    resized();
    if (open) {
      reveal(N.powerMenu);
      // Volet à sa hauteur maximale : le menu s'ouvre sous le pli.
      requestAnimationFrame(() => N.powerMenu.scrollIntoView({ block: 'nearest', behavior: 'smooth' }));
    }
  }

  // ===================================================================== DOM
  function build() {
    N.badge = h('span', { class: 'q-badge' });
    N.title = h('span', { class: 'q-title' });
    N.sub = h('span', { class: 'q-subtitle' });
    N.down = h('span', { class: 'dn' });
    N.up = h('span', { class: 'up' });
    const chev = h('span', { class: 'q-chev' });
    setIcon(chev, ICON.chevD);
    N.spark = svg('svg', { class: 'q-spark', viewBox: `0 0 ${RATE_POINTS - 1} 40`, preserveAspectRatio: 'none', 'aria-hidden': 'true' });
    N.area = svg('polygon', { class: 'area' });
    N.lineDown = svg('polyline', { class: 'down' });
    N.lineUp = svg('polyline', { class: 'up' });
    N.spark.append(N.area, N.lineDown, N.lineUp);
    N.head = h('button', { class: 'q-conn-head', 'aria-expanded': 'false', onclick: () => { setExpanded(!Q.expanded); resized(); } },
      h('span', { class: 'q-conn-top' },
        N.badge,
        h('span', { class: 'q-conn-txt' }, N.title, N.sub),
        // Les débits changent toutes les 2 s : pas d'annonce au lecteur d'écran à chaque fois.
        h('span', { class: 'q-rates', 'aria-live': 'off' }, N.down, N.up),
        chev),
      N.spark);
    N.details = h('div', { class: 'q-details', hidden: true });
    N.conn = h('section', { class: 'q-sec q-conn', style: '--i:0' }, N.head, N.details);

    N.wifiSwitch = h('button', {
      class: 'q-switch', role: 'switch', 'aria-checked': 'false', 'aria-label': 'Wi-Fi',
      onclick: () => setRadio('wifi', !Q.wifi?.radio),
    }, h('i'));
    N.wifiList = h('div', { class: 'q-wlist' });
    N.wifiStatus = h('div', { class: 'p-empty q-wstatus' });
    N.wifi = h('section', { class: 'q-sec q-wifi', style: '--i:1', hidden: true },
      h('div', { class: 'q-shead' },
        h('span', { class: 'q-label' }, 'Wi-Fi'),
        h('button', { class: 'p-sub link', onclick: () => openSettings('ms-settings:network-wifi') }, 'Paramètres Wi-Fi ›'),
        N.wifiSwitch),
      N.wifiList, N.wifiStatus);

    N.tiles = h('section', { class: 'q-sec q-tiles', style: '--i:2' });
    N.plans = h('div', { class: 'q-plans', hidden: true });

    N.volSlot = h('div', { class: 'q-vol' });
    N.bright = h('div', { class: 'q-bright', hidden: true });
    N.sliders = h('section', { class: 'q-sec q-sliders', style: '--i:3' }, N.volSlot, N.bright);

    N.batIco = h('span', { class: 'q-bat-ico' });
    N.batTxt = h('span', {});
    N.bat = h('span', { class: 'q-bat', hidden: true }, N.batIco, N.batTxt);
    N.powerBtn = h('button', {
      class: 'icon-btn q-power', 'aria-label': 'Alimentation', 'aria-expanded': 'false',
      title: 'Verrouiller, mettre en veille, redémarrer…', onclick: togglePowerMenu,
    });
    setIcon(N.powerBtn, ICON.power);
    N.foot = h('section', { class: 'q-sec q-foot', style: '--i:4' },
      N.bat,
      h('button', { class: 'p-sub link q-netlink', onclick: () => openSettings('ms-settings:network') }, 'Paramètres réseau et Internet ›'),
      N.powerBtn);
    N.powerMenu = h('div', { class: 'q-powmenu', hidden: true }, POWER.map((it, i) => {
      const ico = h('span', { class: 'q-pw-ico' });
      setIcon(ico, it.icon);
      const label = h('span', {}, it.label);
      const b = h('button', { class: `q-pw${it.confirm ? ' danger' : ''}`, style: `--i:${i}` }, ico, label);
      b._item = it;
      b._label = label;
      b.addEventListener('click', () => onPower(b));
      return b;
    }));

    Q.nodes = [N.conn, N.wifi, N.tiles, N.plans, N.sliders, N.foot, N.powerMenu];
    Q.built = true;
  }

  // ================================================================ données
  // Une réponse arrivée après la fermeture reste valable : elle sert à la prochaine ouverture.
  async function refreshNet() {
    if (Q.netBusy) return;
    Q.netBusy = true;
    let data = null;
    try { data = await invoke('network_info'); } catch { /* état précédent conservé */ }
    Q.netBusy = false;
    if (!data) return;
    Q.net = data;
    if (!Q.open) return;
    renderConn();
    renderTiles();
    resized();
  }

  async function refreshWifi(scan) {
    if (Q.wifiBusy) return;
    Q.wifiBusy = true;
    const epoch = Q.wifiEpoch;
    let data = null;
    try { data = await invoke('wifi_networks', { scan }); } catch { /* état précédent conservé */ }
    Q.wifiBusy = false;
    // Relevé lancé avant une connexion ou une déconnexion : il décrit l'état d'avant, on le refait.
    if (epoch !== Q.wifiEpoch) return refreshWifi(false);
    if (!data) return;
    Q.wifi = data;
    if (!Q.open) return;
    renderWifi();
    renderTiles();
    resized();
  }

  function loadOnce() {
    const gen = Q.gen;
    const load = (cmd, apply) => invoke(cmd)
      .then((v) => { if (gen === Q.gen) { apply(v || []); resized(); } })
      .catch(() => {});
    load('quick_radios', (v) => { Q.radios = v; renderTiles(); });
    load('quick_power_plans', (v) => { Q.plans = v; renderTiles(); renderPlans(); });
    load('quick_monitors', (v) => { Q.monitors = v; renderBrightness(); });
  }

  // ============================================================ enregistrement
  POPS.quick = () => {
    if (!Q.built) build();
    return Q.nodes;
  };
  // Mises à jour sur place (écouteur de stats ci-dessous et rafraîchissements du volet).
  POP_DEPS.quick = [];
  POP_HOOKS.quick = {
    prepare() {
      if (!Q.built) build();
      Q.gen++;
      Q.open = true;
      closeJoin(false);
      disarm();
      setExpanded(false);
      N.plans.hidden = true;
      tileEls.get('power')?.classList.remove('open');
      N.powerMenu.hidden = true;
      N.powerBtn.classList.remove('open');
      // Une ligne neuve à chaque ouverture : app.js oublie celles qui ont quitté le DOM.
      N.volSlot.replaceChildren(masterRow());
      renderConn();
      renderRates(S.stats);
      renderWifi();
      renderTiles();
      renderBrightness();
      renderBattery(S.stats);
    },
    open() {
      refreshNet();
      Q.scanning = true;
      refreshWifi(true);
      loadOnce();
      const gen = Q.gen;
      // Le balayage Wi-Fi demandé à l'ouverture donne ses résultats quelques secondes plus tard.
      Q.timers.push(
        setInterval(refreshNet, NET_EVERY),
        setInterval(() => refreshWifi(false), WIFI_EVERY),
        setTimeout(() => { Q.scanning = false; if (gen === Q.gen) refreshWifi(false); }, 2500),
      );
    },
    close() {
      Q.open = false;
      Q.gen++;
      // clearTimeout annule aussi bien les intervalles que les délais.
      for (const t of Q.timers) clearTimeout(t);
      Q.timers = [];
      Q.scanning = false;
      closeJoin(false);
      disarm();
    },
  };

  listen('stats', (e) => {
    const s = e.payload;
    if (!s) return;
    Q.rates.down.push(s.net_down || 0);
    Q.rates.up.push(s.net_up || 0);
    if (Q.rates.down.length > RATE_POINTS) {
      Q.rates.down.shift();
      Q.rates.up.shift();
    }
    if (Q.open) {
      renderRates(s);
      renderBattery(s);
    }
  });

  const btn = $('net-btn');
  btn.addEventListener('click', () => openPop('quick', btn));
  btn.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    openSettings('ms-settings:network');
  });
})();
