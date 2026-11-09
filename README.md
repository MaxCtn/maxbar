# maxbar

Une barre des tâches de remplacement pour Windows 11, écrite en Rust avec [Tauri 2](https://tauri.app).
L'interface est en HTML, CSS et JavaScript, sans framework.

maxbar se place en haut (ou en bas) de l'écran, réserve sa place comme la barre Windows, et peut cacher complètement cette dernière.
Elle tient en un exécutable d'environ 5 Mo et consomme à peu près 80 Mo de RAM, WebView2 compris.

## Fonctionnalités

**Applications**
- Les épingles de la barre Windows sont importées au premier lancement.
- Les fenêtres ouvertes sont regroupées par application.
- Gestes sur une icône :
  - clic : activer ou réduire ;
  - clic molette : nouvelle fenêtre ;
  - clic droit : liste des fenêtres, épingler, fermer.

**Îlot central**
- Il affiche le morceau en cours (Spotify, navigateurs… via les contrôles média de Windows).
- Il s'ouvre au clic : pochette, progression, contrôles.
- Il sert aussi de notification : volume, changement de morceau, alertes de quota Claude.

**Zone de notification**
- maxbar héberge elle-même les icônes des applications (Discord, Steam, OneDrive…).
- Clic gauche, clic droit, double-clic et clic molette sont transmis à l'application.

**Son**
- Un clic sur le haut-parleur ouvre un panneau :
  - volume général ;
  - choix de la sortie audio ;
  - volume par application.
- La molette sur l'icône règle le volume.

**Clavier** : l'indicateur de langue (FRA, ENU…) suit la fenêtre active. Un clic permet de changer de disposition.

**Horloge** : un clic ouvre un volet latéral avec :
- l'heure ;
- la météo détaillée ;
- un calendrier avec numéros de semaine ;
- le quota Claude ;
- le volume ;
- des raccourcis.

**Météo** : [Open-Meteo](https://open-meteo.com), sans clé d'API. La position vient de la ville configurée, à défaut de l'adresse IP.

**Système** : CPU, RAM, GPU, débit réseau, batterie, avec un historique de 3 minutes.

**Quota Claude Code** :
- fenêtres de 5 h et de 7 jours ;
- consommation sur les 15 dernières minutes et la dernière heure ;
- rythme horaire et projection jusqu'à la réinitialisation.

maxbar se met en pause dès qu'une application passe en plein écran (jeu, vidéo) : la barre disparaît et ses mesures s'arrêtent.

## Installation

Prérequis : Windows 11, [Rust](https://rustup.rs) stable et les outils de build C++ de Visual Studio. WebView2 est fourni avec Windows 11.

```powershell
git clone https://github.com/MaxCtn/maxbar
cd maxbar\src-tauri
cargo build --release
.\target\release\maxbar.exe
```

Le CLI de Tauri n'est pas nécessaire. L'interface (`ui/`) est embarquée dans l'exécutable à la compilation.

L'icône maxbar de la zone de notification donne accès aux options suivantes :
- position haut ou bas ;
- masquage de la barre Windows ;
- lancement au démarrage ;
- réimport des épingles ;
- édition de la configuration.

Si maxbar plante en ayant caché la barre Windows, relancer maxbar puis la quitter proprement la rétablit. Redémarrer l'Explorateur fonctionne aussi.

## Configuration

Le fichier est `%APPDATA%\maxbar\config.json`. Après l'avoir modifié, utilise « Recharger la configuration » dans le menu.

| Clé | Défaut | Rôle |
|---|---|---|
| `edge` | `"top"` | `"top"` ou `"bottom"` |
| `height` | `42` | Hauteur de la barre, en pixels logiques |
| `pinned` | épingles Windows | Chemins d'exécutables, de raccourcis `.lnk` ou URL |
| `opacity` | `0.72` | Opacité du fond (0 à 1) |
| `weather_city` | `""` | Ville pour la météo ; vide = position approximative par IP |
| `show_music`, `show_claude`, `show_system`, `show_weather` | `true` | Modules affichés |
| `clock_seconds` | `false` | Secondes dans l'horloge |
| `tray` | `true` | Héberger la zone de notification |
| `hide_windows_taskbar` | `true` | Cacher complètement la barre Windows |

### Quota Claude Code

maxbar lit `%USERPROFILE%\.claude\monitor\quota.json`. Ce fichier doit être écrit par une [statusline](https://docs.claude.com/en/docs/claude-code/statusline) de Claude Code. Il reprend le champ `rate_limits` que Claude Code transmet à la statusline, avec un horodatage :

```json
{
  "rate_limits": {
    "five_hour": { "used_percentage": 12.5, "resets_at": 1762712400 },
    "seven_day": { "used_percentage": 40, "resets_at": 1763000000 }
  },
  "_ts": 1762700000
}
```

Les sessions actives sont détectées dans `%USERPROFILE%\.claude\projects`. Sans ce fichier, le module affiche simplement « aucune donnée ».

## Comment ça marche

```
src-tauri/src/
  main.rs      commandes appelées par l'interface, menu de l'icône, configuration
  appbar.rs    réservation de l'écran (SHAppBarMessage), région de fenêtre, plein écran,
               fenêtres décalées sous la barre, masquage de la barre Windows
  tray.rs      hôte de la zone de notification
  tasks.rs     fenêtres ouvertes et épingles (raccourcis résolus via IShellLinkW)
  audio.rs     volume, sorties et volume par application (Core Audio)
  keyboard.rs  dispositions du clavier
  media.rs     lecture en cours (GlobalSystemMediaTransportControls)
  stats.rs     CPU, RAM, GPU (PDH), réseau, batterie
  weather.rs   météo Open-Meteo
  claude.rs    quota et sessions Claude Code
  config.rs    lecture et écriture de la configuration
  icons.rs     extraction des icônes d'applications en PNG
  sys.rs       petits utilitaires Win32
ui/            index.html, style.css, app.js
```

Quelques choix qui ne sautent pas aux yeux :

- **Fenêtre et clics.** La fenêtre couvre tout l'écran. Une région Win32 (`SetWindowRgn`) ne garde visibles et cliquables que la barre et les panneaux ouverts. Partout ailleurs, les clics atteignent les fenêtres en dessous.
- **Zone de notification.** maxbar crée une fenêtre de classe `Shell_TrayWnd`, la même méthode que [Zebar](https://github.com/glzr-io/zebar). Les applications lui envoient leurs icônes comme à l'Explorateur, et chaque message est relayé à la vraie barre.
- **Météo.** Les requêtes passent par `curl.exe`, livré avec Windows : le client HTTP WinRT échoue à la négociation TLS avec l'API d'Open-Meteo.
- **Sortie audio.** Elle se change via `IPolicyConfig`, une interface non documentée mais stable, utilisée par le panneau Son de Windows lui-même.

## Développement

Pour inspecter l'interface pendant que maxbar tourne, lance-la avec :

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
.\target\release\maxbar.exe
```

Ouvre ensuite `edge://inspect`.

L'exécutable en cours d'utilisation est verrouillé par Windows. Pour recompiler sans quitter maxbar, renomme-le d'abord (`maxbar.exe` → `maxbar.exe.old`).
