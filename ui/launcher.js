// Lanceur maxbar : remplace le menu Démarrer et la recherche Windows.
// Script classique chargé après app.js, dont il réutilise les utilitaires (h, icon, invoke, S, POPS...).
(() => {
  'use strict';

  const DAY = 86400000;
  const GRID_MAX = 18;
  const LIMIT = { app: 8, win: 5, set: 6, rec: 5 };
  const ANIM_MAX = 12;
  const DEFAULT_SEARCH = 'https://www.google.com/search?q=';

  // ==================================================================== icônes
  const svg = (body) => `<svg viewBox="0 0 20 20" class="lx-svg">${body}</svg>`;
  const G = {
    search: svg('<circle cx="8.6" cy="8.6" r="5.2"/><path d="m12.6 12.6 4 4"/>'),
    close: svg('<path d="m6 6 8 8m0-8-8 8"/>'),
    chevR: svg('<path d="m8 5 5 5-5 5"/>'),
    chevL: svg('<path d="m12 5-5 5 5 5"/>'),
    gear: svg('<circle cx="10" cy="10" r="2.6"/><path d="M10 2.6v2M10 15.4v2M17.4 10h-2M4.6 10h-2M15.2 4.8l-1.4 1.4M6.2 13.8l-1.4 1.4M15.2 15.2l-1.4-1.4M6.2 6.2 4.8 4.8"/>'),
    folder: svg('<path d="M2.8 5.8a1.6 1.6 0 0 1 1.6-1.6h3.2l1.9 1.9h6.1a1.6 1.6 0 0 1 1.6 1.6v6.5a1.6 1.6 0 0 1-1.6 1.6H4.4a1.6 1.6 0 0 1-1.6-1.6z"/>'),
    download: svg('<path d="M10 3.4v9.2M6.2 9l3.8 3.8L13.8 9M4 16.4h12"/>'),
    doc: svg('<path d="M5.4 2.8h5.9l3.3 3.3v11.1H5.4z"/><path d="M11.1 2.8v3.5h3.5M7.9 10.4h4.4M7.9 13.3h4.4"/>'),
    power: svg('<path d="M10 2.9v6.5"/><path d="M6.1 5.4a6.1 6.1 0 1 0 7.8 0"/>'),
    lock: svg('<rect x="4.4" y="8.8" width="11.2" height="8" rx="2"/><path d="M6.9 8.8V6.5a3.1 3.1 0 0 1 6.2 0v2.3"/>'),
    moon: svg('<path d="M16.3 12.3A6.7 6.7 0 0 1 7.7 3.7a6.7 6.7 0 1 0 8.6 8.6z"/>'),
    logout: svg('<path d="M8.4 16.5H5.2a1.6 1.6 0 0 1-1.6-1.6V5.1a1.6 1.6 0 0 1 1.6-1.6h3.2M12.6 13.6 16.2 10l-3.6-3.6M16.2 10H7.8"/>'),
    restart: svg('<path d="M15.8 10.4a5.8 5.8 0 1 1-1.9-4.6"/><path d="M14.8 2.9v3.3h-3.3"/>'),
    window: svg('<rect x="3" y="4" width="14" height="12" rx="2"/><path d="M3 7.4h14"/>'),
    calc: svg('<rect x="4.2" y="2.6" width="11.6" height="14.8" rx="2.2"/><path d="M7 6h6M7.2 10h.01M10 10h.01M12.8 10h.01M7.2 13.4h.01M10 13.4h.01M12.8 13.4h.01"/>'),
    check: svg('<path d="m4.6 10.4 3.4 3.4 7.4-7.6"/>'),
    globe: svg('<circle cx="10" cy="10" r="7"/><path d="M3 10h14M10 3c1.9 2.1 2.8 4.4 2.8 7s-.9 4.9-2.8 7c-1.9-2.1-2.8-4.4-2.8-7S8.1 5.1 10 3z"/>'),
    fileSearch: svg('<path d="M9 17H5.4A1.4 1.4 0 0 1 4 15.6V4.4A1.4 1.4 0 0 1 5.4 3h5.4l3.6 3.6v2.2"/><circle cx="13" cy="13" r="2.6"/><path d="m15 15 2 2"/>'),
    tool: svg('<path d="M12.7 3.3a3.8 3.8 0 0 0-4.6 5L3.7 12.7a1.7 1.7 0 0 0 2.4 2.4l4.4-4.4a3.8 3.8 0 0 0 5-4.6l-2.2 2.2-2-.4-.4-2z"/>'),
    sliders: svg('<path d="M4 6h7M15 6h1M4 14h1M9 14h7"/><circle cx="13" cy="6" r="2"/><circle cx="7" cy="14" r="2"/>'),
    terminal: svg('<rect x="2.8" y="3.8" width="14.4" height="12.4" rx="2"/><path d="m6.2 8 2.4 2-2.4 2M10.4 12.4h3.4"/>'),
    shield: svg('<path d="M10 2.8 4.2 5v4.6c0 3.4 2.4 6.2 5.8 7.6 3.4-1.4 5.8-4.2 5.8-7.6V5z"/>'),
    admin: svg('<path d="M10 2.8 4.2 5v4.6c0 3.4 2.4 6.2 5.8 7.6 3.4-1.4 5.8-4.2 5.8-7.6V5z"/><path d="m7.6 10 1.7 1.7 3.2-3.3"/>'),
    open: svg('<path d="M11.4 3.6h5v5M16.4 3.6l-7 7M14.4 11.8v3a1.6 1.6 0 0 1-1.6 1.6H5.2a1.6 1.6 0 0 1-1.6-1.6V7.2a1.6 1.6 0 0 1 1.6-1.6h3"/>'),
    pin: svg('<path d="m11.6 3.2 5.2 5.2-1.9.6-2.9 2.9.2 3.2-1.3 1.3-6.5-6.5 1.3-1.3 3.2.2 2.9-2.9z"/><path d="m6.6 13.4-3.4 3.4"/>'),
    unpin: svg('<path d="m11.6 3.2 5.2 5.2-1.9.6-2.9 2.9.2 3.2-1.3 1.3-6.5-6.5 1.3-1.3 3.2.2 2.9-2.9z"/><path d="m6.6 13.4-3.4 3.4M3 3l14 14"/>'),
  };

  // ============================================================ paramètres
  // [nom, cible, mots-clés FR/EN, type] — s : Paramètres, p : panneau de configuration,
  // t : outil d'administration, c : console, x : sécurité.
  const SETTINGS = [
    ['Paramètres', 'ms-settings:', 'settings reglages configuration options', 's'],
    ['Système', 'ms-settings:system', 'system', 's'],
    ['Affichage', 'ms-settings:display', 'display ecran resolution luminosite brightness echelle scale hdr moniteur monitor', 's'],
    ['Éclairage nocturne', 'ms-settings:nightlight', 'night light lumiere bleue blue light', 's'],
    ['Graphiques', 'ms-settings:display-advancedgraphics', 'graphics gpu carte graphique performances', 's'],
    ['Son', 'ms-settings:sound', 'sound audio volume haut-parleurs speakers casque headphones micro microphone sortie entree', 's'],
    ['Mélangeur de volume', 'ms-settings:apps-volume', 'volume mixer melangeur applications son', 's'],
    ['Notifications', 'ms-settings:notifications', 'notifications alertes', 's'],
    ['Ne pas déranger', 'ms-settings:quiethours', 'do not disturb focus concentration', 's'],
    ['Alimentation et batterie', 'ms-settings:powersleep', 'power battery batterie alimentation veille sleep energie', 's'],
    ['Stockage', 'ms-settings:storagesense', 'storage stockage disque espace nettoyage storage sense', 's'],
    ['Multitâche', 'ms-settings:multitasking', 'multitasking snap ancrage fenetres alt tab bureaux virtuels virtual desktops', 's'],
    ['Presse-papiers', 'ms-settings:clipboard', 'clipboard historique history copier coller', 's'],
    ['Bureau à distance', 'ms-settings:remotedesktop', 'remote desktop rdp distance', 's'],
    ['Informations système', 'ms-settings:about', 'about a propos specifications pc nom renommer rename version windows processeur ram', 's'],
    ['Bluetooth et appareils', 'ms-settings:bluetooth', 'bluetooth appareils devices peripheriques casque', 's'],
    ['Imprimantes et scanners', 'ms-settings:printers', 'printers imprimante scanner', 's'],
    ['Souris', 'ms-settings:mousetouchpad', 'mouse souris pointeur vitesse molette scroll', 's'],
    ['Pavé tactile', 'ms-settings:devices-touchpad', 'touchpad pave tactile gestes gestures', 's'],
    ['Exécution automatique', 'ms-settings:autoplay', 'autoplay execution automatique usb', 's'],
    ['Réseau et Internet', 'ms-settings:network', 'network reseau internet connexion', 's'],
    ['Wi-Fi', 'ms-settings:network-wifi', 'wifi sans fil wireless wlan', 's'],
    ['Ethernet', 'ms-settings:network-ethernet', 'ethernet cable lan ip dns', 's'],
    ['VPN', 'ms-settings:network-vpn', 'vpn reseau prive', 's'],
    ['Proxy', 'ms-settings:network-proxy', 'proxy', 's'],
    ['Mode avion', 'ms-settings:network-airplanemode', 'airplane mode avion', 's'],
    ["Point d'accès mobile", 'ms-settings:network-mobilehotspot', 'hotspot partage de connexion tethering', 's'],
    ['Arrière-plan', 'ms-settings:personalization-background', 'wallpaper fond d ecran background image', 's'],
    ['Couleurs', 'ms-settings:personalization-colors', 'colors couleurs mode sombre dark mode clair light accent transparence', 's'],
    ['Thèmes', 'ms-settings:themes', 'themes personnalisation personalization', 's'],
    ['Écran de verrouillage', 'ms-settings:lockscreen', 'lock screen ecran verrouillage', 's'],
    ['Polices', 'ms-settings:fonts', 'fonts polices typographie', 's'],
    ['Barre des tâches', 'ms-settings:taskbar', 'taskbar barre des taches', 's'],
    ['Applications installées', 'ms-settings:appsfeatures', 'apps installed programs programmes desinstaller uninstall logiciels', 's'],
    ['Applications par défaut', 'ms-settings:defaultapps', 'default apps navigateur par defaut browser associations', 's'],
    ['Applications au démarrage', 'ms-settings:startupapps', 'startup demarrage lancement automatique', 's'],
    ['Fonctionnalités facultatives', 'ms-settings:optionalfeatures', 'optional features fonctionnalites', 's'],
    ['Vos informations', 'ms-settings:yourinfo', 'account compte profil photo', 's'],
    ['Options de connexion', 'ms-settings:signinoptions', 'sign in password mot de passe pin hello empreinte', 's'],
    ['Date et heure', 'ms-settings:dateandtime', 'date heure time fuseau horaire timezone horloge clock', 's'],
    ['Langue et région', 'ms-settings:regionlanguage', 'language langue region format', 's'],
    ['Saisie', 'ms-settings:typing', 'typing saisie correction orthographe autocorrect', 's'],
    ['Clavier', 'ms-settings:keyboard', 'keyboard clavier disposition layout langue de saisie', 's'],
    ['Mode jeu', 'ms-settings:gaming-gamemode', 'game mode jeu gaming', 's'],
    ['Xbox Game Bar', 'ms-settings:gaming-gamebar', 'game bar xbox capture', 's'],
    ['Accessibilité', 'ms-settings:easeofaccess', 'accessibility', 's'],
    ['Taille du texte', 'ms-settings:easeofaccess-display', 'text size taille texte police agrandir', 's'],
    ['Loupe', 'ms-settings:easeofaccess-magnifier', 'magnifier loupe zoom', 's'],
    ['Confidentialité et sécurité', 'ms-settings:privacy', 'privacy confidentialite permissions autorisations', 's'],
    ['Localisation', 'ms-settings:privacy-location', 'location localisation gps position', 's'],
    ['Caméra', 'ms-settings:privacy-webcam', 'camera webcam', 's'],
    ['Microphone', 'ms-settings:privacy-microphone', 'microphone micro', 's'],
    ['Windows Update', 'ms-settings:windowsupdate', 'update mise a jour mises a jour', 's'],
    ['Récupération', 'ms-settings:recovery', 'recovery reinitialiser reset restaurer demarrage avance', 's'],
    ['Activation', 'ms-settings:activation', 'activation licence cle de produit product key', 's'],
    ['Espace développeurs', 'ms-settings:developers', 'developer mode developpeur sudo', 's'],
    ['Sécurité Windows', 'windowsdefender:', 'antivirus defender securite virus pare-feu firewall protection', 'x'],
    ['Panneau de configuration', 'control', 'control panel panneau configuration', 'p'],
    ['Connexions réseau', 'ncpa.cpl', 'network connections adaptateur adapter carte reseau ip', 'p'],
    ['Programmes et fonctionnalités', 'appwiz.cpl', 'programs features desinstaller uninstall programmes', 'p'],
    ["Options d'alimentation", 'powercfg.cpl', 'power options mode de gestion plan alimentation', 'p'],
    ['Son (panneau classique)', 'mmsys.cpl', 'sound playback recording peripheriques lecture enregistrement audio', 'p'],
    ['Propriétés système', 'sysdm.cpl', 'system properties variables d environnement environment variables nom ordinateur', 'p'],
    ['Pare-feu Windows Defender', 'firewall.cpl', 'firewall pare-feu', 'p'],
    ['Options Internet', 'inetcpl.cpl', 'internet options proxy', 'p'],
    ["Comptes d'utilisateurs", 'netplwiz', 'user accounts utilisateurs comptes', 'p'],
    ['Gestionnaire de périphériques', 'devmgmt.msc', 'device manager peripheriques pilotes drivers materiel hardware', 't'],
    ['Services', 'services.msc', 'services', 't'],
    ['Gestion des disques', 'diskmgmt.msc', 'disk management partitions volumes disques', 't'],
    ["Gestion de l'ordinateur", 'compmgmt.msc', 'computer management ordinateur', 't'],
    ["Observateur d'événements", 'eventvwr.msc', 'event viewer evenements journaux logs', 't'],
    ['Planificateur de tâches', 'taskschd.msc', 'task scheduler planificateur', 't'],
    ['Éditeur du Registre', 'regedit', 'registry editor regedit base de registre', 't'],
    ['Gestionnaire des tâches', 'taskmgr', 'task manager processus processes', 't'],
    ['Moniteur de ressources', 'resmon', 'resource monitor ressources', 't'],
    ['Analyseur de performances', 'perfmon', 'performance monitor', 't'],
    ['Informations système (msinfo32)', 'msinfo32', 'system information materiel', 't'],
    ['Configuration du système', 'msconfig', 'system configuration demarrage boot', 't'],
    ['Nettoyage de disque', 'cleanmgr', 'disk cleanup nettoyage', 't'],
    ['Défragmenter et optimiser les lecteurs', 'dfrgui', 'defragment optimize trim ssd', 't'],
    ['Connexion Bureau à distance', 'mstsc', 'remote desktop connection rdp mstsc', 't'],
    ['Terminal', 'wt', 'terminal windows terminal console shell', 'c'],
    ['Invite de commandes', 'cmd', 'command prompt cmd dos console', 'c'],
    ['Windows PowerShell', 'powershell', 'powershell ps console', 'c'],
  ];
  const KINDS = {
    s: ['Paramètres', G.gear, 'var(--violet)'],
    p: ['Panneau de configuration', G.sliders, 'var(--blue)'],
    t: ["Outil d'administration", G.tool, 'var(--amber)'],
    c: ['Console', G.terminal, 'var(--green)'],
    x: ['Sécurité', G.shield, 'var(--green)'],
  };
  let catalog = null;

  const POWER = [
    ['lock', 'Verrouiller', G.lock, false],
    ['sleep', 'Mettre en veille', G.moon, false],
    ['logoff', 'Se déconnecter', G.logout, true],
    ['restart', 'Redémarrer', G.restart, true],
    ['shutdown', 'Arrêter', G.power, true],
  ];
  const LETTERS = ['#', ...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'];

  // ================================================================= état
  const L = {
    apps: null, // null : pas encore chargées ; [] : index en cours de construction
    sorted: [],
    byId: new Map(),
    version: 0,
    stale: false,
    loading: null,
    again: false, // index reconstruit pendant un chargement : à relire ensuite
    state: { usage: {}, pins: [] },
    stateReady: false,
    recent: [],
    recentReady: false,
    recentSeq: 0,
    view: 'home',
    query: '',
    q: '',
    nav: [],
    sel: -1,
    selEl: null,
    selKey: null,
    copiedAt: 0,
    copiedFor: '',
    copyTimer: 0,
    armTimer: 0,
    // Fermé par Échap, la touche Win ou Démarrer : le clavier doit revenir à la fenêtre d'avant.
    giveBack: false,
  };
  let ui = null;
  const tilePool = new Map();
  const filePool = new Map();
  const rowPool = new Map();
  const allRows = new Map();
  const allHeads = new Map();
  const secHeads = new Map();

  // ======================================================= correspondance
  const norm = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
  const isWordChar = (c) => /[\p{L}\p{N}]/u.test(c);

  /** Nom prénormalisé : texte sans accents, mots (CamelCase découpé) et initiales. */
  function prep(text) {
    const n = norm(text);
    const words = norm(text.replace(/(\p{Ll})(\p{Lu})/gu, '$1 $2')).split(/[^\p{L}\p{N}]+/u).filter(Boolean);
    return { n, words, initials: words.map((w) => w[0]).join(''), same: n.length === text.length };
  }

  function boundary(n, q) {
    for (let i = n.indexOf(q, 1); i > 0; i = n.indexOf(q, i + 1)) if (!isWordChar(n[i - 1])) return i;
    return -1;
  }

  /** Écart total entre les lettres de `q` retrouvées dans l'ordre dans `n`, ou -1. */
  function subsequence(n, q) {
    let j = 0;
    let last = -1;
    let gaps = 0;
    for (let i = 0; i < n.length && j < q.length; i++) {
      if (n[i] !== q[j]) continue;
      if (last >= 0) gaps += i - last - 1;
      last = i;
      j++;
    }
    return j === q.length ? gaps : -1;
  }

  /** Préfixe > début de mot > initiales > sous-chaîne > sous-séquence ; `at` situe le surlignage. */
  function match(q, p) {
    const { n } = p;
    if (n.startsWith(q)) return { s: (n.length === q.length ? 1000 : 900) - Math.min(n.length - q.length, 60), at: 0 };
    const b = boundary(n, q);
    if (b > 0) return { s: 780 - Math.min(b, 40), at: b };
    if (q.length < 2) return null;
    if (p.words.some((w) => w.startsWith(q))) return { s: 740 };
    if (p.initials.startsWith(q)) return { s: 700 - Math.min(p.initials.length - q.length, 20) * 2 };
    const i = n.indexOf(q);
    if (i >= 0) return { s: 520 - Math.min(i, 40), at: i };
    const tokens = q.split(' ');
    if (tokens.length > 1 && tokens.every((t) => p.words.some((w) => w.startsWith(t)))) return { s: 640 };
    if (q.length > 2) {
      const gaps = subsequence(n, q);
      if (gaps >= 0) return { s: Math.max(120, 320 - gaps * 6), fuzzy: true };
    }
    return null;
  }

  function keywordMatch(q, s) {
    if (q.length < 2) return null;
    if (s.kw.some((w) => w.startsWith(q))) return { s: 600 };
    if (s.kwText.includes(q)) return { s: 560 };
    const tokens = q.split(' ');
    if (tokens.length > 1 && tokens.every((t) => s.kw.some((w) => w.startsWith(t)) || s.p.words.some((w) => w.startsWith(t)))) {
      return { s: 560 };
    }
    return null;
  }

  /** Fréquence (log) et récence des lancements : départage sans renverser un bien meilleur score. */
  function bonus(id) {
    const u = L.state.usage[id];
    if (!u || !u.count) return 0;
    const age = Math.max(0, Date.now() - (u.last || 0));
    return Math.min(90, 16 * Math.log2(1 + u.count) + 40 * Math.exp(-age / (3 * DAY)));
  }

  const highlight = (m, p) => (m && m.at != null && p.same ? [m.at, L.q.length] : null);

  // ================================================================ calcul
  const FUNCS = new Map([
    ['sqrt', Math.sqrt], ['racine', Math.sqrt], ['sin', Math.sin], ['cos', Math.cos], ['tan', Math.tan],
    ['log', Math.log10], ['ln', Math.log], ['abs', Math.abs], ['exp', Math.exp],
  ]);
  const CONSTS = new Map([['pi', Math.PI], ['π', Math.PI], ['e', Math.E]]);
  const OPS = { '×': '*', '·': '*', '÷': '/', '−': '-', '**': '^' };

  function tokenize(src) {
    const re = /\s*(?:(\d+(?:[.,]\d+)?(?:e[+-]?\d+)?|[.,]\d+)|([a-zπ]+)|(\*\*|[-+*/^%()×÷·−√°]))/iy;
    const out = [];
    while (re.lastIndex < src.length) {
      const from = re.lastIndex;
      const m = re.exec(src);
      if (!m) {
        if (/^\s*$/.test(src.slice(from))) break;
        return null;
      }
      if (m[1]) out.push({ n: parseFloat(m[1].replace(',', '.')) });
      else if (m[2]) out.push({ id: m[2].toLowerCase() });
      else out.push({ op: OPS[m[3]] || m[3] });
    }
    return out;
  }

  /** Analyseur à descente récursive (sans eval) : + − × ÷ ^ %, parenthèses, fonctions, constantes. */
  function evaluate(src) {
    const t = tokenize(src);
    if (!t || !t.length) return null;
    let i = 0;
    let ops = 0;
    let consts = 0;
    const is = (op) => t[i]?.op === op;
    const fail = () => { throw new Error('calc'); };

    const expr = () => {
      let v = term();
      while (is('+') || is('-')) {
        const op = t[i++].op;
        ops++;
        const r = term();
        v = op === '+' ? v + r : v - r;
      }
      return v;
    };
    const term = () => {
      let v = unary();
      for (;;) {
        if (is('*') || is('/')) {
          const op = t[i++].op;
          ops++;
          const r = unary();
          v = op === '*' ? v * r : v / r;
        } else if (t[i] && (t[i].id || is('(') || is('√'))) {
          // Multiplication implicite : 2pi, 3(4+1), 2 sqrt 9.
          ops++;
          v *= unary();
        } else {
          return v;
        }
      }
    };
    const unary = () => {
      if (is('-')) { i++; return -unary(); }
      if (is('+')) { i++; return unary(); }
      return power();
    };
    const power = () => {
      const base = postfix();
      if (!is('^')) return base;
      i++;
      ops++;
      return base ** unary();
    };
    const postfix = () => {
      let v = primary();
      for (;;) {
        if (is('%')) { i++; ops++; v /= 100; }
        else if (is('°')) { i++; v = (v * Math.PI) / 180; }
        else return v;
      }
    };
    const primary = () => {
      const tk = t[i++];
      if (!tk) fail();
      if (tk.n != null) return tk.n;
      if (tk.op === '(') {
        const v = expr();
        // Parenthèse fermante facultative en fin de saisie (« sqrt(2 » pendant la frappe).
        if (is(')')) i++;
        else if (i < t.length) fail();
        return v;
      }
      if (tk.op === '√') { ops++; return Math.sqrt(postfix()); }
      if (tk.id && FUNCS.has(tk.id)) { ops++; return FUNCS.get(tk.id)(is('(') ? primary() : postfix()); }
      if (tk.id && CONSTS.has(tk.id)) { consts++; return CONSTS.get(tk.id); }
      return fail();
    };

    try {
      const value = expr();
      return i === t.length && Number.isFinite(value) ? { value, ops, consts } : null;
    } catch {
      return null;
    }
  }

  const fmtNumber = new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 10 });
  function calcText(v, display) {
    const x = Number(v.toPrecision(12));
    const a = Math.abs(x);
    if (a !== 0 && (a >= 1e15 || a < 1e-9)) return x.toExponential(8).replace(/\.?0+e/, 'e').replace('.', ',');
    return display ? fmtNumber.format(x) : String(x).replace('.', ',');
  }

  // ========================================================== dates relatives
  const fmtHM = new Intl.DateTimeFormat('fr-FR', { hour: '2-digit', minute: '2-digit' });
  const fmtWeekday = new Intl.DateTimeFormat('fr-FR', { weekday: 'long' });
  const fmtDM = new Intl.DateTimeFormat('fr-FR', { day: 'numeric', month: 'short' });
  const fmtDMY = new Intl.DateTimeFormat('fr-FR', { day: 'numeric', month: 'short', year: 'numeric' });

  function when(ms) {
    const d = new Date(ms);
    const now = new Date();
    const diff = now - d;
    if (diff < 60000) return "À l'instant";
    if (diff < 3600000) return `Il y a ${Math.floor(diff / 60000)} min`;
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
    if (ms >= today) return `Aujourd'hui, ${fmtHM.format(d)}`;
    if (ms >= today - DAY) return `Hier, ${fmtHM.format(d)}`;
    if (ms >= today - 6 * DAY) {
      const day = fmtWeekday.format(d);
      return day.charAt(0).toUpperCase() + day.slice(1);
    }
    return (d.getFullYear() === now.getFullYear() ? fmtDM : fmtDMY).format(d);
  }

  // =============================================================== actions
  function open(path) {
    closeAll();
    invoke('launch', { path });
  }

  function launchApp(app, admin = false) {
    const u = L.state.usage[app.id] || (L.state.usage[app.id] = { count: 0, last: 0 });
    u.count++;
    u.last = Date.now();
    closeAll();
    invoke('launcher_launch', { id: app.id, admin }).catch(() => {});
  }

  function reveal(id) {
    closeAll();
    invoke('launcher_reveal', { id }).catch(() => {});
  }

  function activateWindow(w) {
    closeAll();
    // `activate` réduit la fenêtre déjà active (comportement du dock) : ici on veut seulement y revenir.
    if (w.hwnd !== S.tasks?.active || w.minimized) invoke('activate', { hwnd: w.hwnd });
    else invoke('restore_focus').catch(() => {});
  }

  function applyState(s) {
    if (!s) return;
    L.state = { usage: s.usage || {}, pins: s.pins || [] };
    L.stateReady = true;
  }

  function pin(id, on) {
    if (!on) L.state.pins = L.state.pins.filter((p) => p !== id);
    else if (!L.state.pins.includes(id)) L.state.pins = [...L.state.pins, id];
    refresh();
    invoke('launcher_pin', { id, pinned: on }).then((s) => { applyState(s); refresh(); }, () => {});
  }

  function copyCalc(v) {
    const raw = L.query;
    invoke('clipboard_set', { text: calcText(v, false) }).then(() => {
      L.copiedAt = Date.now();
      L.copiedFor = raw;
      refresh();
      clearTimeout(L.copyTimer);
      L.copyTimer = setTimeout(refresh, 1500);
    }, () => {});
  }

  function appMenu(app) {
    const pinned = L.state.pins.includes(app.id);
    // Les apps empaquetées (AUMID « famille!app ») ne s'élèvent pas.
    const packaged = !app.path && app.id.includes('!');
    return [
      ['Ouvrir', G.open, () => launchApp(app)],
      ["Exécuter en tant qu'administrateur", G.admin, () => launchApp(app, true), packaged],
      ["Ouvrir l'emplacement du fichier", G.folder, () => reveal(app.id), !app.path],
      pinned
        ? ['Désépingler du lanceur', G.unpin, () => pin(app.id, false)]
        : ['Épingler au lanceur', G.pin, () => pin(app.id, true)],
    ];
  }

  // ================================================================ éléments
  function appItem(app, m) {
    return {
      key: `app:${app.id}`, title: app.name, sub: 'Application', img: app.icon, letter: app.name,
      hl: highlight(m, app._p), hint: 'Ouvrir',
      run: (admin) => launchApp(app, admin), menu: () => appMenu(app),
    };
  }

  function winItem(w, m, p) {
    const info = S.apps?.[w.key] || {};
    return {
      key: `win:${w.hwnd}`, title: w.title, sub: info.name ? `Fenêtre ouverte · ${info.name}` : 'Fenêtre ouverte',
      img: info.icon, glyph: G.window, tint: 'var(--blue)', hl: highlight(m, p), hint: 'Basculer',
      run: () => activateWindow(w),
    };
  }

  function setItem(s, m) {
    const [label, glyph, tint] = KINDS[s.type];
    return { key: `set:${s.target}`, title: s.name, sub: label, glyph, tint, hl: highlight(m, s.p), hint: 'Ouvrir', run: () => open(s.target) };
  }

  function recItem(r, m) {
    return {
      key: `rec:${r.path}`, title: r.name, sub: `${r.folder} · ${when(r.modified_ms)}`, img: r.icon,
      glyph: G.doc, tint: 'var(--muted)', hl: highlight(m, r._p), hint: 'Ouvrir',
      run: () => open(r.path),
      menu: () => [
        ['Ouvrir', G.open, () => open(r.path)],
        ["Ouvrir l'emplacement du fichier", G.folder, () => reveal(r.path)],
      ],
    };
  }

  function calcItem(raw) {
    const r = evaluate(raw);
    if (!r || !(r.ops > 0 || (r.consts > 0 && !/^\s*e\s*$/i.test(raw)))) return null;
    const copied = L.copiedFor === raw && Date.now() - L.copiedAt < 1400;
    return {
      strong: r.ops > 0,
      it: {
        key: 'calc', title: `= ${calcText(r.value, true)}`,
        sub: copied ? 'Copié dans le presse-papiers' : `Calcul · ${raw.trim()}`,
        glyph: copied ? G.check : G.calc, tint: 'var(--green)', copied, hint: copied ? 'Copié' : 'Copier',
        run: () => copyCalc(r.value),
      },
    };
  }

  function webItem(raw) {
    const q = raw.trim();
    const base = S.config?.search_url || DEFAULT_SEARCH;
    let host = '';
    try { host = new URL(base).hostname.replace(/^www\./, ''); } catch { host = ''; }
    const pre = 'Rechercher « ';
    return {
      key: 'web', title: `${pre}${q} » sur le web`, hl: [pre.length, q.length],
      sub: host ? `Recherche web · ${host}` : 'Recherche web', glyph: G.globe, tint: 'var(--blue)', hint: 'Rechercher',
      run: () => open(base + encodeURIComponent(q)),
    };
  }

  function filesItem(raw) {
    const q = raw.trim();
    const pre = 'Rechercher « ';
    return {
      key: 'files', title: `${pre}${q} » dans les fichiers`, hl: [pre.length, q.length],
      sub: "Explorateur · dossier personnel", glyph: G.fileSearch, tint: 'var(--amber)', hint: 'Rechercher',
      run: () => { closeAll(); invoke('launcher_search_files', { query: q }).catch(() => {}); },
    };
  }

  function settings() {
    catalog ??= SETTINGS.map(([name, target, kw, type]) => ({ name, target, type, p: prep(name), kw: norm(kw).split(' '), kwText: norm(kw) }));
    return catalog;
  }

  function search(raw) {
    const q = norm(raw.trim()).replace(/\s+/g, ' ');
    L.q = q;
    const byScore = (a, b) => b.s - a.s;
    const pins = new Set(L.state.pins);

    const apps = [];
    const appNames = new Set();
    for (const a of L.apps || []) {
      const m = match(q, a._p);
      if (!m) continue;
      apps.push({ it: appItem(a, m), s: m.s + bonus(a.id) + (pins.has(a.id) ? 15 : 0), fuzzy: m.fuzzy });
      appNames.add(a._p.n);
    }
    const wins = [];
    for (const w of S.tasks?.windows || []) {
      const p = prep(w.title || '');
      let m = match(q, p);
      const appName = S.apps?.[w.key]?.name;
      if (!m && appName) {
        const byApp = match(q, prep(appName));
        if (byApp) m = { s: byApp.s - 40, fuzzy: byApp.fuzzy };
      }
      if (m) wins.push({ it: winItem(w, m, p), s: m.s - 50, fuzzy: m.fuzzy });
    }
    const sets = [];
    for (const s of settings()) {
      // Terminal, Gestionnaire des tâches... figurent déjà parmi les applications.
      if (appNames.has(s.p.n)) continue;
      let m = match(q, s.p);
      if (!m || m.fuzzy) m = keywordMatch(q, s) || m;
      if (m) sets.push({ it: setItem(s, m), s: m.s - 20, fuzzy: m.fuzzy });
    }
    const recs = [];
    for (const r of L.recent) {
      r._p ??= prep(r.name);
      const m = match(q, r._p);
      if (m) recs.push({ it: recItem(r, m), s: m.s - 60, fuzzy: m.fuzzy });
    }
    // Dès qu'une correspondance franche existe, les lettres éparpillées (« paint » dans « Pare-feu ») sont du bruit.
    const sharp = [apps, wins, sets, recs].some((list) => list.some((x) => !x.fuzzy));
    for (const list of [apps, wins, sets, recs]) {
      if (sharp) list.splice(0, list.length, ...list.filter((x) => !x.fuzzy));
      list.sort(byScore);
    }

    const calc = calcItem(raw);
    const web = webItem(raw);
    const top = [apps[0], wins[0], sets[0], recs[0]].filter(Boolean).sort(byScore)[0];
    const best = calc?.strong ? calc.it : top?.it || calc?.it || web;
    const pick = (list, n) => list.map((x) => x.it).filter((it) => it !== best).slice(0, n);
    return {
      best,
      groups: [
        ['Applications', pick(apps, LIMIT.app)],
        ['Fenêtres ouvertes', pick(wins, LIMIT.win)],
        ['Paramètres', pick(sets, LIMIT.set)],
        ['Fichiers récents', pick(recs, LIMIT.rec)],
        ['Calcul', calc && calc.it !== best ? [calc.it] : []],
        ['Rechercher', [web, filesItem(raw)].filter((it) => it !== best)],
      ],
    };
  }

  // =================================================================== rendu
  function setIc(span, it) {
    const sig = it.img || (it.glyph ? `${it.glyph}${it.tint}` : `l:${it.letter}`);
    if (span._sig === sig) return;
    span._sig = sig;
    span.className = 'lx-ic';
    if (it.img) {
      span.replaceChildren(h('img', { alt: '', decoding: 'async', loading: 'lazy', draggable: 'false', src: it.img }));
    } else if (it.glyph) {
      span.classList.add('glyph');
      span.style.setProperty('--tint', it.tint || 'var(--violet)');
      span.replaceChildren(icon(it.glyph));
    } else {
      const name = (it.letter || '?').trim() || '?';
      span.classList.add('letter');
      span.style.setProperty('--tint', `hsl(${hue(name)} 45% 42%)`);
      span.replaceChildren(name.charAt(0).toUpperCase());
    }
  }

  function setName(el, title, hl) {
    const sig = hl ? `${title}\u0001${hl[0]},${hl[1]}` : title;
    if (el._sig === sig) return;
    el._sig = sig;
    if (!hl) {
      el.textContent = title;
      return;
    }
    const [a, n] = hl;
    el.replaceChildren(title.slice(0, a), h('mark', {}, title.slice(a, a + n)), title.slice(a + n));
  }

  function wireItem(el) {
    el.addEventListener('click', (e) => el._it?.run(e.ctrlKey && e.shiftKey));
    el.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      if (el._it) openMenu(el._it, e.clientX, e.clientY);
    });
  }

  function rowEl(pool, key, cls) {
    let el = pool.get(key);
    if (!el) {
      el = h('button', { class: cls, tabindex: '-1', role: 'option' },
        h('span', { class: 'lx-ic' }),
        h('span', { class: 'lx-txt' }, h('span', { class: 'lx-name' }), h('span', { class: 'lx-sub' })),
        h('span', { class: 'lx-hint' }));
      wireItem(el);
      pool.set(key, el);
    }
    return el;
  }

  function fillRow(el, it) {
    el._it = it;
    const [ic, txt, hint] = el.children;
    setIc(ic, it);
    setName(txt.firstChild, it.title, it.hl);
    setText(txt.lastChild, it.sub || '');
    txt.lastChild.hidden = !it.sub;
    setText(hint, it.hint || '');
    el.classList.toggle('copied', !!it.copied);
    return el;
  }

  function tile(app) {
    let el = tilePool.get(app.id);
    if (!el) {
      el = h('button', { class: 'lx-tile', tabindex: '-1', role: 'option' }, h('span', { class: 'lx-ic' }), h('span', { class: 'lx-tname' }));
      wireItem(el);
      tilePool.set(app.id, el);
    }
    const it = appItem(app);
    el._it = it;
    setIc(el.firstChild, it);
    setText(el.lastChild, app.name);
    el.title = app.name;
    return el;
  }

  /** Remet les enfants de `parent` dans l'ordre voulu en déplaçant les nœuds existants. */
  function sync(parent, nodes) {
    nodes.forEach((n, i) => { if (parent.children[i] !== n) parent.insertBefore(n, parent.children[i] || null); });
    while (parent.children.length > nodes.length) parent.lastChild.remove();
  }

  function prune(pool, nodes) {
    const keep = new Set(nodes);
    for (const [k, el] of pool) if (!keep.has(el)) pool.delete(k);
  }

  /** Échelonne l'entrée des premiers éléments visibles (animation jouée seulement à l'ouverture). */
  function stagger(nodes) {
    let i = 1;
    for (const n of nodes) {
      const on = i <= ANIM_MAX;
      n.classList.toggle('lx-anim', on);
      if (on) n.style.setProperty('--i', String(i));
      i++;
    }
  }

  // Tant que l'historique est court : les apps du dock, puis les classiques de Windows, plutôt que le début
  // de l'alphabet (« 7-Zip », « Accès vocal »...).
  const STAPLES = [
    'explorateurdefichiers', 'fileexplorer', 'parametres', 'settings', 'calculatrice', 'calculator', 'blocnotes',
    'notepad', 'terminal', 'paint', 'photos', 'outilcapturedecran', 'snippingtool', 'microsoftstore',
    'gestionnairedestaches', 'taskmanager', 'horloge', 'clock',
  ];
  const bare = (s) => norm(s).replace(/[^a-z0-9]/g, '');

  function mostUsed() {
    const used = L.sorted.filter((a) => L.state.usage[a.id]?.count > 0).sort((a, b) => bonus(b.id) - bonus(a.id));
    const out = used.slice(0, GRID_MAX);
    if (out.length >= GRID_MAX) return out;
    // Par nom, pas par chemin : bien des raccourcis de dossier pointent sur explorer.exe.
    const docked = new Set((S.pins || []).map((p) => bare(S.apps?.[p.key]?.name || '')).filter(Boolean));
    const rank = (a) => {
      const name = bare(a.name);
      if (docked.has(name)) return 0;
      const i = STAPLES.indexOf(name);
      return i < 0 ? STAPLES.length + 1 : i + 1;
    };
    const taken = new Set(out);
    const rest = L.sorted.filter((a) => !taken.has(a)).map((a) => [rank(a), a]);
    rest.sort((x, y) => x[0] - y[0]);
    for (const [, a] of rest.slice(0, GRID_MAX - out.length)) out.push(a);
    return out;
  }

  function renderHome() {
    const ready = Array.isArray(L.apps) && L.apps.length > 0;
    const pins = ready ? L.state.pins.map((id) => L.byId.get(id)).filter(Boolean) : [];
    const used = Object.values(L.state.usage).some((u) => u.count > 0);
    setText(ui.homeTitle, pins.length || !ready ? 'Épinglées' : used ? 'Les plus utilisées' : 'Suggestions');
    let tiles = [];
    if (ready) {
      tiles = (pins.length ? pins : mostUsed()).map(tile);
      prune(tilePool, tiles);
      sync(ui.grid, tiles);
    } else {
      ui.skeleton.classList.toggle('indexing', Array.isArray(L.apps));
      sync(ui.grid, [ui.skeleton]);
    }
    const files = L.recent.map((r) => fillRow(rowEl(filePool, r.path, 'lx-file'), recItem(r)));
    prune(filePool, files);
    const empty = L.recentReady ? [ui.noRecent] : [];
    sync(ui.recent, files.length ? files : empty);
    stagger([ui.homeHead, ...tiles, ui.recentHead, ...files]);
    if (currentView() === 'home') setNav([...tiles, ...files], -1);
  }

  const letterOf = (n) => {
    const c = n.charAt(0).toUpperCase();
    return c >= 'A' && c <= 'Z' ? c : '#';
  };

  function renderAll() {
    if (ui.allVersion === L.version) return;
    ui.allVersion = L.version;
    const nodes = [];
    const rows = [];
    const used = new Set();
    let current = null;
    for (const a of L.sorted) {
      const c = letterOf(a._p.n);
      if (c !== current) {
        current = c;
        used.add(c);
        let head = allHeads.get(c);
        if (!head) {
          head = h('button', { class: 'lx-letter', tabindex: '-1', title: 'Aller à une lettre', onclick: () => showLetters(true) }, c);
          allHeads.set(c, head);
        }
        nodes.push(head);
      }
      let el = allRows.get(a.id);
      if (!el) {
        el = h('button', { class: 'lx-arow', tabindex: '-1', role: 'option' }, h('span', { class: 'lx-ic' }), h('span', { class: 'lx-aname' }));
        wireItem(el);
        allRows.set(a.id, el);
      }
      const it = appItem(a);
      el._it = it;
      setIc(el.firstChild, it);
      setText(el.lastChild, a.name);
      nodes.push(el);
      rows.push(el);
    }
    prune(allRows, rows);
    for (const [c, b] of ui.letterBtns) b.disabled = !used.has(c);
    sync(ui.allList, nodes.length ? nodes : [ui.noApps]);
    ui.allNav = rows;
  }

  function secHead(title) {
    let el = secHeads.get(title);
    if (!el) secHeads.set(title, (el = h('div', { class: 'lx-sec' }, title)));
    return el;
  }

  function renderResults() {
    const r = search(L.query);
    const nodes = [];
    const nav = [];
    const add = (it, best) => {
      const el = fillRow(rowEl(rowPool, `${best ? 'b' : 'r'}:${it.key}`, best ? 'lx-row lx-best' : 'lx-row'), it);
      nodes.push(el);
      nav.push(el);
    };
    nodes.push(secHead('Meilleur résultat'));
    add(r.best, true);
    for (const [title, items] of r.groups) {
      if (!items.length) continue;
      nodes.push(secHead(title));
      for (const it of items) add(it, false);
    }
    prune(rowPool, nav);
    sync(ui.results, nodes);
    setNav(nav, 0);
  }

  const currentView = () => (L.query.trim() ? 'results' : L.view);

  /** Met à jour la vue affichée (accueil, toutes les applications ou résultats). */
  function refresh() {
    if (!ui || pop.name !== 'launcher') return;
    const v = currentView();
    if (ui.root.dataset.view !== v) {
      ui.root.dataset.view = v;
      L.selKey = null;
    }
    if (v === 'results') renderResults();
    else if (v === 'all') {
      renderAll();
      setNav(ui.allNav || [], -1);
    } else renderHome();
  }

  // =============================================================== sélection
  function setNav(list, fallback) {
    L.nav = list;
    const i = L.selKey ? list.findIndex((el) => el._it?.key === L.selKey) : -1;
    select(i >= 0 ? i : fallback, false);
  }

  function select(i, scroll) {
    const el = L.nav[i] || null;
    if (L.selEl && L.selEl !== el) {
      L.selEl.classList.remove('sel');
      L.selEl.removeAttribute('aria-selected');
    }
    L.sel = el ? i : -1;
    L.selEl = el;
    L.selKey = el?._it?.key ?? null;
    if (!el) return;
    el.classList.add('sel');
    el.setAttribute('aria-selected', 'true');
    if (scroll) revealEl(el);
  }

  function move(d) {
    const n = L.nav.length;
    if (!n) return;
    select(L.sel < 0 ? (d > 0 ? 0 : n - 1) : (L.sel + d + n) % n, true);
  }

  /** Haut/Bas dans les grilles de l'accueil : élément de la rangée voisine le plus proche en colonne. */
  function moveRow(d) {
    const cur = L.selEl;
    if (!cur?.closest('.lx-grid, .lx-recent')) return move(d);
    const r = cur.getBoundingClientRect();
    const x = r.left + r.width / 2;
    let best = -1;
    let bestGap = Infinity;
    let bestDx = Infinity;
    L.nav.forEach((el, i) => {
      const b = el.getBoundingClientRect();
      const gap = Math.round(d > 0 ? b.top - r.bottom : r.top - b.bottom);
      if (gap < 0 || !b.height) return; // même rangée, ou de l'autre côté
      const dx = Math.abs(b.left + b.width / 2 - x);
      if (gap < bestGap || (gap === bestGap && dx < bestDx)) {
        best = i;
        bestGap = gap;
        bestDx = dx;
      }
    });
    // Au bord, on boucle comme dans les listes.
    select(best >= 0 ? best : d > 0 ? 0 : L.nav.length - 1, true);
  }

  function revealEl(el) {
    const box = el.closest('.lx-scroll');
    if (!box) return;
    const r = el.getBoundingClientRect();
    const b = box.getBoundingClientRect();
    const pad = 10;
    if (r.top < b.top + pad) box.scrollTop -= b.top + pad - r.top;
    else if (r.bottom > b.bottom - pad) box.scrollTop += r.bottom - b.bottom + pad;
  }

  // ========================================================= menus internes
  function openMenu(it, x, y) {
    const entries = it.menu?.();
    if (!entries?.length) return;
    closePower();
    const m = ui.menu;
    m.replaceChildren(...entries.map(([label, glyph, fn, disabled]) => h('button', {
      class: 'lx-mi', tabindex: '-1', role: 'menuitem', disabled: !!disabled,
      onclick: () => { closeMenu(); fn(); },
    }, icon(glyph), h('span', {}, label))));
    m.hidden = false;
    const box = ui.root.getBoundingClientRect();
    m.style.left = `${clamp(x - box.left, 8, box.width - m.offsetWidth - 8)}px`;
    m.style.top = `${clamp(y - box.top, 8, box.height - m.offsetHeight - 8)}px`;
    m.classList.remove('in');
    void m.offsetWidth; // relance l'animation
    m.classList.add('in');
  }

  function closeMenu() {
    if (ui && !ui.menu.hidden) ui.menu.hidden = true;
  }

  function menuForSelection() {
    const el = L.selEl;
    if (!el?._it) return;
    const r = el.getBoundingClientRect();
    openMenu(el._it, r.left + 28, r.bottom - 6);
    ui.menu.querySelector('.lx-mi:not(:disabled)')?.classList.add('sel');
  }

  /** Clavier dans le menu contextuel ouvert. Vrai si la touche lui revenait. */
  function menuKey(k) {
    const items = [...ui.menu.querySelectorAll('.lx-mi:not(:disabled)')];
    const i = items.findIndex((b) => b.classList.contains('sel'));
    switch (k) {
      case 'ArrowDown':
      case 'ArrowUp': {
        const n = items.length;
        if (!n) return true;
        const d = k === 'ArrowDown' ? 1 : -1;
        items[i]?.classList.remove('sel');
        items[i < 0 ? (d > 0 ? 0 : n - 1) : (i + d + n) % n].classList.add('sel');
        return true;
      }
      case 'Enter':
        // Sans entrée choisie, Entrée referme le menu sans rien lancer.
        if (i >= 0) items[i].click();
        else closeMenu();
        return true;
      case 'Escape':
        closeMenu();
        return true;
    }
    return false;
  }

  function disarm() {
    clearTimeout(L.armTimer);
    ui?.power.querySelectorAll('.armed').forEach((b) => b.classList.remove('armed'));
  }

  function togglePower() {
    closeMenu();
    disarm();
    ui.root.classList.toggle('power-open');
  }

  function closePower() {
    if (!ui) return;
    disarm();
    ui.root.classList.remove('power-open');
  }

  function powerItem([action, label, glyph, guarded]) {
    const b = h('button', { class: 'lx-pi', tabindex: '-1', role: 'menuitem' },
      icon(glyph),
      h('span', { class: 'lx-plabel' }, h('span', {}, label), guarded && h('span', { class: 'lx-psub' }, 'Cliquer à nouveau pour confirmer')));
    b.addEventListener('click', () => {
      // Déconnexion, redémarrage et arrêt : un second clic dans les 3 s confirme.
      if (guarded && !b.classList.contains('armed')) {
        disarm();
        b.classList.add('armed');
        L.armTimer = setTimeout(disarm, 3000);
        return;
      }
      closeAll();
      invoke('power_action', { action }).catch(() => {});
    });
    return b;
  }

  function showLetters(on) {
    if (!ui) return;
    ui.all.classList.toggle('letters', on);
  }

  function jumpTo(c) {
    showLetters(false);
    const head = allHeads.get(c);
    if (head?.isConnected) ui.allList.scrollTop = head.offsetTop - 6;
  }

  // ============================================================ construction
  function build() {
    const input = h('input', {
      class: 'lx-input', type: 'text', spellcheck: 'false', autocomplete: 'off', 'aria-label': 'Rechercher',
      placeholder: 'Rechercher des applications, réglages, fichiers…',
    });
    const clear = h('button', { class: 'lx-clear', tabindex: '-1', 'aria-label': 'Effacer la recherche', hidden: true }, icon(G.close));
    const field = h('label', { class: 'lx-field lx-anim', style: '--i:0' }, h('span', { class: 'lx-glass' }, icon(G.search)), input, clear);

    const homeTitle = h('span', { class: 'lx-title' }, 'Épinglées');
    const homeHead = h('div', { class: 'lx-head' }, homeTitle,
      h('button', { class: 'lx-pill', tabindex: '-1', onclick: () => setView('all') }, 'Toutes les applications', icon(G.chevR)));
    const grid = h('div', { class: 'lx-grid', role: 'listbox', 'aria-label': 'Applications épinglées' });
    const skeleton = h('div', { class: 'lx-skel' },
      h('div', { class: 'lx-skel-grid' }, Array.from({ length: 12 }, (_, i) => h('span', { class: 'lx-sk', style: `--d:${i * 70}ms` }, h('i'), h('b')))),
      h('div', { class: 'lx-skel-cap' }, 'Indexation des applications…'));
    const recentHead = h('div', { class: 'lx-head' }, h('span', { class: 'lx-title' }, 'Récents'));
    const recent = h('div', { class: 'lx-recent', role: 'listbox', 'aria-label': 'Fichiers récents' });
    const home = h('section', { class: 'lx-view lx-home lx-scroll' }, homeHead, grid, recentHead, recent);

    const letterBtns = new Map(LETTERS.map((c) => [c, h('button', { class: 'lx-lt', tabindex: '-1', onclick: () => jumpTo(c) }, c)]));
    const letters = h('div', { class: 'lx-letters' }, [...letterBtns.values()]);
    letters.addEventListener('click', (e) => { if (e.target === letters) showLetters(false); });
    const allList = h('div', { class: 'lx-alist lx-scroll', role: 'listbox', 'aria-label': 'Toutes les applications' });
    const all = h('section', { class: 'lx-view lx-all' },
      h('div', { class: 'lx-head' }, h('span', { class: 'lx-title' }, 'Toutes les applications'),
        h('button', { class: 'lx-pill back', tabindex: '-1', onclick: () => setView('home') }, icon(G.chevL), 'Retour')),
      allList, letters);

    const results = h('section', { class: 'lx-view lx-results lx-scroll', role: 'listbox', 'aria-label': 'Résultats' });

    const fbtn = (label, glyph, path, square) => h('button', {
      class: `lx-fbtn${square ? ' sq' : ''}`, tabindex: '-1', title: label, 'aria-label': label, onclick: () => open(path),
    }, icon(glyph), !square && h('span', {}, label));
    const pwrBtn = h('button', { class: 'lx-fbtn sq lx-pwr', tabindex: '-1', title: 'Alimentation', 'aria-label': 'Alimentation', onclick: togglePower }, icon(G.power));
    const power = h('div', { class: 'lx-power', role: 'menu' }, POWER.map(powerItem));
    const foot = h('div', { class: 'lx-foot' },
      fbtn('Paramètres', G.gear, 'ms-settings:'),
      fbtn('Explorateur', G.folder, 'explorer.exe'),
      fbtn('Téléchargements', G.download, 'shell:Downloads', true),
      fbtn('Documents', G.doc, 'shell:Personal', true),
      h('span', { class: 'lx-spacer' }),
      pwrBtn, power);

    const menu = h('div', { class: 'lx-menu', role: 'menu', hidden: true });
    const root = h('div', { class: 'lx', 'data-view': 'home' },
      h('div', { class: 'lx-top' }, field),
      h('div', { class: 'lx-body' }, home, all, results),
      foot, menu);

    ui = {
      root, input, clear, home, homeHead, homeTitle, grid, skeleton, recentHead, recent, all, allList, letters, letterBtns,
      results, power, pwrBtn, menu, allVersion: -1, allNav: [],
      noRecent: h('div', { class: 'lx-empty' }, 'Aucun fichier récent.'),
      noApps: h('div', { class: 'lx-empty' }, 'Indexation des applications…'),
    };

    input.addEventListener('input', onInput);
    clear.addEventListener('click', () => { setQuery(''); focusInput(); });
    // Les clics gardent le focus dans le champ de recherche (et donc le clavier).
    root.addEventListener('mousedown', (e) => {
      const t = e.target;
      if (!menu.hidden && !menu.contains(t)) closeMenu();
      if (root.classList.contains('power-open') && !power.contains(t) && !pwrBtn.contains(t)) closePower();
      if (t !== input && t.closest('button')) e.preventDefault();
    });
    root.addEventListener('scroll', closeMenu, true);
  }

  // ================================================================ saisie
  function focusInput(selectAll) {
    if (!ui) return;
    ui.input.focus({ preventScroll: true });
    if (selectAll) ui.input.select();
  }

  function onInput() {
    L.query = ui.input.value;
    L.selKey = null;
    ui.clear.hidden = !L.query;
    ui.results.scrollTop = 0;
    closeMenu();
    refresh();
  }

  function setQuery(text) {
    ui.input.value = text;
    onInput();
  }

  function setView(v) {
    L.view = v;
    L.selKey = null;
    showLetters(false);
    if (v === 'all') ui.allList.scrollTop = 0;
    refresh();
  }

  function onKey(e) {
    if (pop.name !== 'launcher' || !ui || e.isComposing) return;
    const k = e.key;
    // La touche Win elle-même et la frappe factice que winkey glisse avant son relâchement.
    if (k === 'Meta' || k === 'OS' || k === 'Unidentified') return;
    const stop = () => { e.preventDefault(); e.stopPropagation(); };
    if (!ui.menu.hidden) {
      if (menuKey(k)) return stop();
      closeMenu();
    }
    if (k === 'Escape' && ui.root.classList.contains('power-open')) {
      closePower();
      return stop();
    }
    if (k === 'Escape' && ui.all.classList.contains('letters')) {
      showLetters(false);
      return stop();
    }
    switch (k) {
      case 'ArrowDown':
      case 'ArrowUp':
        stop();
        return moveRow(k === 'ArrowDown' ? 1 : -1);
      case 'ArrowLeft':
      case 'ArrowRight':
        if (!L.query) {
          stop();
          move(k === 'ArrowRight' ? 1 : -1);
        }
        return;
      case 'Enter':
        stop();
        return L.selEl?._it?.run(e.ctrlKey && e.shiftKey);
      case 'Escape':
        // Requête vide : on laisse passer, app.js ferme tout.
        if (L.query) {
          stop();
          setQuery('');
        } else {
          L.giveBack = true;
        }
        return;
      case 'Tab':
        stop();
        if (!L.query.trim()) return setView(L.view === 'all' ? 'home' : 'all');
        return move(e.shiftKey ? -1 : 1);
      case 'ContextMenu':
        stop();
        return menuForSelection();
      case 'F10':
        if (e.shiftKey) {
          stop();
          menuForSelection();
        }
        return;
    }
    // Une frappe ailleurs dans le panneau revient dans le champ de recherche.
    if (document.activeElement === ui.input || e.ctrlKey || e.altKey || e.metaKey) return;
    if (k.length === 1 || k === 'Backspace') {
      stop();
      focusInput();
      const v = ui.input.value;
      setQuery(k === 'Backspace' ? v.slice(0, -1) : v + k);
    }
  }

  // ============================================================== chargement
  function setApps(list) {
    if (!Array.isArray(list)) return;
    for (const a of list) a._p = prep(a.name);
    const group = (a) => (letterOf(a._p.n) === '#' ? 0 : 1);
    L.apps = list;
    L.sorted = list.slice().sort((a, b) => group(a) - group(b) || a._p.n.localeCompare(b._p.n, 'fr'));
    L.byId = new Map(list.map((a) => [a.id, a]));
    L.version++;
    refresh();
  }

  function loadApps() {
    if (!L.loading) {
      L.stale = false;
      L.loading = invoke('launcher_apps').then(setApps, () => {}).finally(() => {
        L.loading = null;
        // Index reconstruit pendant la requête : la réponse reçue est peut-être déjà périmée.
        if (L.again) {
          L.again = false;
          if (pop.name === 'launcher') loadApps();
          else L.stale = true;
        }
      });
    }
    return L.loading;
  }

  function loadState() {
    invoke('launcher_state').then((s) => { applyState(s); refresh(); }, () => {});
  }

  async function loadRecent() {
    const seq = ++L.recentSeq;
    let list;
    try { list = await invoke('launcher_recent'); } catch { return; }
    if (seq !== L.recentSeq || !Array.isArray(list)) return;
    L.recent = list;
    L.recentReady = true;
    refresh();
  }

  /** Préchargement au survol de #start / #search : le lanceur s'ouvre ensuite complet. */
  function prefetch() {
    if (L.apps === null || L.stale) loadApps();
    if (!L.stateReady) loadState();
  }

  function reset() {
    L.query = '';
    L.view = 'home';
    L.selKey = null;
    ui.input.value = '';
    ui.clear.hidden = true;
    ui.root.dataset.view = 'home';
    ui.home.scrollTop = 0;
    ui.results.scrollTop = 0;
    closeMenu();
    closePower();
    showLetters(false);
  }

  // ============================================================ enregistrement
  POPS.launcher = () => {
    if (!ui) build();
    // Relancé sur `tasks` / `apps` : seuls les résultats (fenêtres ouvertes) en dépendent.
    if (L.query.trim()) renderResults();
    return [ui.root];
  };
  POP_DEPS.launcher = ['tasks', 'apps'];
  POP_HOOKS.launcher = {
    prepare() {
      if (!ui) build();
      L.giveBack = false;
      reset();
      if (L.apps === null || L.stale) loadApps();
      if (!L.stateReady) loadState();
      loadRecent();
      refresh();
    },
    open() {
      focusInput();
      invoke('launcher_refresh').catch(() => {});
    },
    close() {
      closeMenu();
      closePower();
      showLetters(false);
      // Côté Rust, rien ne se passe si la barre n'est déjà plus au premier plan.
      if (L.giveBack) invoke('restore_focus').catch(() => {});
      L.giveBack = false;
    },
  };

  const start = $('start');
  const toggle = () => {
    if (pop.name === 'launcher') L.giveBack = true;
    openPop('launcher', start);
  };
  const openSearch = () => (pop.name === 'launcher' ? focusInput(true) : openPop('launcher', start));

  start.addEventListener('click', toggle);
  start.addEventListener('pointerenter', prefetch);
  $('search').addEventListener('pointerenter', prefetch);
  // Lanceur déjà ouvert : le clic sur la loupe ne doit ni le fermer (mousedown global) ni voler le focus.
  $('search').addEventListener('mousedown', (e) => {
    if (pop.name !== 'launcher') return;
    e.preventDefault();
    e.stopPropagation();
  });
  $('search').addEventListener('click', openSearch);
  document.addEventListener('keydown', onKey, true);
  window.addEventListener('focus', () => { if (pop.name === 'launcher') focusInput(); });
  listen('launcher', (e) => (e.payload === 'search' ? openSearch() : toggle()));
  listen('launcher-index', () => {
    if (L.loading) L.again = true;
    else if (pop.name === 'launcher') loadApps();
    else L.stale = true;
  });
})();
