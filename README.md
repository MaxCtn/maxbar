# maxbar

Barre de bureau légère pour Windows 11, pensée pour remplacer à terme la barre des tâches.
Elle est écrite en Tauri 2 (Rust), avec WebView2 et une UI en HTML/CSS/JS vanilla, sans framework.

## Ce qu'elle fait

- **Dock** : les apps épinglées (importées une fois depuis la barre Windows) et les fenêtres ouvertes, regroupées par appli.
  - Clic : activer ou réduire.
  - Clic droit : menu.
  - Animations au survol et au lancement.
- **Îlot dynamique** au centre : la musique en cours (SMTC : Spotify, navigateurs…).
  - Il s'agrandit au clic (pochette, contrôles).
  - Il affiche des toasts : volume, alertes de quota Claude à 80 % et 95 %.
- **Zone de notification** : maxbar héberge les icônes de la zone de notification (comme Explorer).
  - Clic gauche, droit, milieu et double-clic sont relayés à l'appli.
- **Quota Claude Code** :
  - fenêtre de 5 h et semaine ;
  - delta sur 15 min et sur 1 h ;
  - rythme en %/h ;
  - projection jusqu'à la réinitialisation, avec sparkline.
  - Source : `~/.claude/monitor/quota.json` (écrit par la statusline).
- **Météo** : Open-Meteo, gratuit et sans clé. Position par ville configurée, sinon par IP. Prévisions à l'heure et sur 6 jours.
- **Stats système** : CPU, RAM, GPU, réseau ↓/↑, type de connexion, volume, batterie.
- **Panneau latéral** (clic sur l'heure) :
  - horloge ;
  - météo détaillée ;
  - calendrier avec numéro de semaine ;
  - Claude ;
  - curseur de volume ;
  - raccourcis.
- **Barre translucide** : l'opacité est réglable.
- **Économe** :
  - les threads se mettent en pause pendant un jeu ou une vidéo en plein écran ;
  - environ 80 Mo de RAM ;
  - moins de 0,3 % d'un cœur au repos.

## Raccourcis

| Action | Effet |
|---|---|
| Clic sur l'heure | Panneau latéral |
| Clic droit sur l'heure | Centre de notifications (Win+N) |
| Clic sur réseau/son | Paramètres rapides (Win+A) |
| Molette sur réseau/son | Volume ±2 % |
| Clic droit sur le bouton Démarrer | Menu Win+X |
| Échap / clic ailleurs | Ferme les panneaux |

Le menu de l'icône maxbar (zone de notification) permet notamment de :

- placer la barre en haut ou en bas ;
- cacher complètement la barre des tâches Windows ;
- réimporter les épingles de Windows ;
- lancer maxbar au démarrage de Windows ;
- quitter.

## Configuration

Le fichier est `%APPDATA%\maxbar\config.json`. Pour l'éditer puis l'appliquer sans redémarrer, utilise les entrées « Modifier la configuration… » puis « Recharger la configuration » du menu.

| Clé | Défaut | Rôle |
|---|---|---|
| `edge` | `"top"` | `"top"` ou `"bottom"` |
| `height` | `42` | Hauteur réservée (px logiques) |
| `pinned` | importées | Chemins `.exe`, `.lnk` ou URL |
| `opacity` | `0.72` | Opacité du fond (0 à 1) |
| `weather_city` | `""` | Ville ; vide = géolocalisation IP |
| `show_music` / `show_claude` / `show_system` / `show_weather` | `true` | Modules affichés |
| `clock_seconds` | `false` | Secondes dans l'horloge |
| `tray` | `true` | Héberger la zone de notification |
| `hide_windows_taskbar` | `false` | Cacher complètement la barre Windows |

L'historique du quota Claude est conservé 26 h dans `%APPDATA%\maxbar\claude-history.csv`.

## Build

Prérequis : Rust stable, WebView2 (fourni avec Windows 11).

```powershell
cd src-tauri
cargo build --release
.\target\release\maxbar.exe
```

L'UI (`ui/`) est embarquée à la compilation. Toute modification du HTML, du CSS ou du JS demande donc un rebuild.
Pour recompiler pendant que maxbar tourne, renomme d'abord l'exe en cours (`maxbar.exe` → `maxbar.exe.old`), sinon Windows le verrouille.

Pour déboguer l'UI, lance maxbar avec
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`, puis ouvre `edge://inspect`.

## Architecture

```
src-tauri/src/
  main.rs     commandes Tauri, menu de la zone de notification, application de la config
  appbar.rs   réservation d'écran (SHAppBarMessage), région de fenêtre, masquage de la barre Windows, plein écran
  tray.rs     hôte de zone de notification : fenêtre "Shell_TrayWnd", WM_COPYDATA, relais vers Explorer
  tasks.rs    fenêtres ouvertes et épingles (.lnk résolus via IShellLinkW)
  media.rs    musique via Windows.Media.Control (SMTC)
  audio.rs    volume via Core Audio (IAudioEndpointVolume)
  stats.rs    CPU / RAM / GPU (PDH), réseau (GetIfTable2), batterie
  weather.rs  Open-Meteo, via curl.exe (le client HTTP WinRT échoue en TLS sur cette API)
  claude.rs   quota et sessions Claude Code, historique
  config.rs   %APPDATA%\maxbar\config.json
  icons.rs    extraction des icônes en PNG
ui/           index.html, style.css, app.js
```

La fenêtre couvre tout l'écran. Une région Win32 (`SetWindowRgn`) ne garde que la barre et les panneaux ouverts : le reste est réellement transparent aux clics.
