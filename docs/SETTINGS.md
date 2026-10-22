# Settings & files

## Where things live

| What                                  | Installed                      | Portable               |
| ------------------------------------- | ------------------------------ | ---------------------- |
| Settings                              | `%APPDATA%\Vido\settings.json` | `.\data\settings.json` |
| Logs (5 × 5 MB, rotated)              | `%LOCALAPPDATA%\Vido\logs`     | `.\data\logs`          |
| Buffer segments, thumbnails, previews | `%LOCALAPPDATA%\Vido\cache`    | `.\data\cache`         |
| Clips waiting to be re-encoded        | `...\cache\pending`            | `.\data\cache\pending` |
| Clips                                 | `Videos\Vido` (configurable)   | same                   |

The cache folder can be moved in Settings → Advanced.

All defaults: [`config/default-settings.json`](../config/default-settings.json) (generated from the
Rust defaults; a test keeps it in sync). Notable defaults: Vido starts with Windows and minimized
to the tray, the buffer starts on launch, saved clips are copied to the clipboard, the "Clip
saved" popup is on, the always-visible overlay is off, and the save sound is Two-Tone at 80 %.

Invalid or partially broken settings files are repaired section by section on startup; a corrupt
file is kept as `settings.json.corrupt-<timestamp>`. **Reset settings and run setup** (Settings →
General) starts over with the defaults.

## Hotkeys

Defaults: Save clip **Ctrl+Shift+F6**, manual recording **Ctrl+Shift+F9**, pause **Ctrl+Shift+F10**,
open last clip **Ctrl+Shift+F11**, open editor **Ctrl+Shift+F12** (see
[`config/hotkeys.example.json`](../config/hotkeys.example.json)).

The defaults avoid NVIDIA ShadowPlay (Alt+F9/F10/Z), Xbox Game Bar (Win+Alt+…) and AMD Adrenalin
(Ctrl+Shift+letters). Shortcuts are validated in Rust: a modifier is required except for F-keys,
Pause, ScrollLock and PrintScreen; Alt+F4 etc. are rejected; duplicates are refused; registration
failures (another app owns the shortcut) are shown per action.

The clip length is the buffer length. Settings from earlier versions, which had "save last
15/30/60 s" and "save custom length" hotkeys, are migrated: Save clip gets the 30 s key (else the
60 s, 15 s or custom-length one; none if all were unbound).
