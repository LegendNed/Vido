//! Which game is being played, for sorting clips into per-game folders.
//!
//! A program counts as a game when it runs from a folder that a game library
//! installed: Steam, Epic, GOG, EA app or Ubisoft Connect. Their local library
//! files and registry entries give the install folders and the real names; nothing
//! is fetched from the internet. The foreground window is checked when a clip is
//! saved, and every few seconds in the background so a clip saved from the tray
//! still gets the game that was played last.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// An installed game: its folder (lowercase, ending in `\`) and its name.
#[derive(Clone, Debug, PartialEq)]
struct Game {
    dir: String,
    name: String,
}

struct Detector {
    games: Vec<Game>,
    scanned: Option<Instant>,
    /// Last game seen in the foreground, and when.
    last: Option<(String, Instant)>,
}

static DETECTOR: Mutex<Detector> = Mutex::new(Detector {
    games: Vec::new(),
    scanned: None,
    last: None,
});

/// Libraries are read again at most this often (a new install shows up then).
const RESCAN: Duration = Duration::from_secs(60);
/// How often the background check looks at the foreground window.
const SAMPLE: Duration = Duration::from_secs(3);

/// The game for a clip saved now: the one in the foreground, else the last one
/// seen in the foreground within `within`.
pub fn current(within: Duration) -> Option<String> {
    if let Some(g) = sample() {
        return Some(g);
    }
    let d = DETECTOR.lock();
    d.last
        .as_ref()
        .filter(|(_, at)| at.elapsed() <= within)
        .map(|(g, _)| g.clone())
}

/// Checks the foreground window every few seconds (cheap: a few system calls).
pub fn start_sampler() {
    let _ = std::thread::Builder::new()
        .name("vido-games".into())
        .spawn(|| loop {
            sample();
            std::thread::sleep(SAMPLE);
        });
}

/// The game in the foreground right now (remembered as the last one seen).
fn sample() -> Option<String> {
    let exe = foreground_exe()?;
    let mut d = DETECTOR.lock();
    let stale = d.scanned.is_none_or(|t| t.elapsed() >= RESCAN);
    let mut game = find(&d.games, &exe);
    if game.is_none() && stale {
        d.games = scan();
        d.scanned = Some(Instant::now());
        tracing::debug!("game libraries: {} games", d.games.len());
        game = find(&d.games, &exe);
    }
    let game = game?;
    d.last = Some((game.clone(), Instant::now()));
    Some(game)
}

fn norm_dir(p: &Path) -> String {
    let mut s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    if !s.ends_with('\\') {
        s.push('\\');
    }
    s
}

/// The game whose folder holds `exe` (the deepest one if folders nest).
fn find(games: &[Game], exe: &Path) -> Option<String> {
    let exe = exe.to_string_lossy().replace('/', "\\").to_lowercase();
    games
        .iter()
        .filter(|g| exe.starts_with(&g.dir))
        .max_by_key(|g| g.dir.len())
        .map(|g| g.name.clone())
}

fn scan() -> Vec<Game> {
    let mut out = Vec::new();
    let mut add = |dir: &Path, name: &str| {
        let name = name.trim();
        // A drive root or an empty name would match far too much.
        if name.is_empty() || dir.parent().is_none() || dir.as_os_str().len() < 4 {
            return;
        }
        let dir = norm_dir(dir);
        // Steam lists its own folder twice (differently spelled).
        if !out.iter().any(|g: &Game| g.dir == dir) {
            out.push(Game {
                dir,
                name: name.to_owned(),
            });
        }
    };
    for (dir, name) in steam_games() {
        add(&dir, &name);
    }
    for (dir, name) in epic_games() {
        add(&dir, &name);
    }
    for (dir, name) in registry_games() {
        add(&dir, &name);
    }
    out
}

/// The two quoted strings of a Valve KeyValues line (`"key"  "value"`).
fn vdf_pair(line: &str) -> Option<(String, String)> {
    let mut parts = line.split('"').skip(1).step_by(2);
    let key = parts.next()?;
    let value = parts.next()?;
    Some((key.to_lowercase(), value.replace("\\\\", "\\")))
}

/// Library folders listed in Steam's `libraryfolders.vdf` (plus Steam's own).
fn steam_libraries(steam: &Path, vdf: &str) -> Vec<PathBuf> {
    let mut libs = vec![steam.to_path_buf()];
    for (k, v) in vdf.lines().filter_map(vdf_pair) {
        let p = PathBuf::from(&v);
        if k == "path" && !libs.contains(&p) {
            libs.push(p);
        }
    }
    libs
}

/// Name and folder from an `appmanifest_<id>.acf`.
fn steam_manifest(library: &Path, acf: &str) -> Option<(PathBuf, String)> {
    let (mut name, mut dir) = (None, None);
    for (k, v) in acf.lines().filter_map(vdf_pair) {
        match k.as_str() {
            "name" if name.is_none() => name = Some(v),
            "installdir" if dir.is_none() => dir = Some(v),
            _ => {}
        }
    }
    let dir = library.join("steamapps").join("common").join(dir?);
    Some((dir, name?))
}

fn steam_games() -> Vec<(PathBuf, String)> {
    let Some(steam) =
        reg::string(reg::HKCU, r"Software\Valve\Steam", "SteamPath").map(PathBuf::from)
    else {
        return Vec::new();
    };
    let vdf = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf"))
        .unwrap_or_default();
    let mut out = Vec::new();
    for lib in steam_libraries(&steam, &vdf) {
        let Ok(entries) = std::fs::read_dir(lib.join("steamapps")) else {
            continue;
        };
        for e in entries.flatten() {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if n.starts_with("appmanifest_") && n.ends_with(".acf") {
                if let Ok(acf) = std::fs::read_to_string(e.path()) {
                    out.extend(steam_manifest(&lib, &acf));
                }
            }
        }
    }
    out
}

fn epic_games() -> Vec<(PathBuf, String)> {
    let root = std::env::var_os("ProgramData")
        .map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
    let Ok(entries) = std::fs::read_dir(root.join(r"Epic\EpicGamesLauncher\Data\Manifests")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("item"))
        })
        .filter_map(|e| {
            let v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            let name = v.get("DisplayName")?.as_str()?.to_owned();
            let dir = v.get("InstallLocation")?.as_str()?;
            Some((PathBuf::from(dir), name))
        })
        .collect()
}

/// GOG's game list, plus EA app and Ubisoft Connect games from Windows'
/// installed-programs list (their own launchers are left out).
fn registry_games() -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let gog = r"SOFTWARE\WOW6432Node\GOG.com\Games";
    for id in reg::subkeys(reg::HKLM, gog) {
        let key = format!(r"{gog}\{id}");
        if let (Some(dir), Some(name)) = (
            reg::string(reg::HKLM, &key, "path"),
            reg::string(reg::HKLM, &key, "gameName"),
        ) {
            out.push((PathBuf::from(dir), name));
        }
    }
    for (root, base) in [
        (
            reg::HKLM,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            reg::HKLM,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            reg::HKCU,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ] {
        for id in reg::subkeys(root, base) {
            let key = format!(r"{base}\{id}");
            let get = |v: &str| reg::string(root, &key, v).unwrap_or_default();
            let (name, dir, publisher) =
                (get("DisplayName"), get("InstallLocation"), get("Publisher"));
            if is_store_game(&id, &name, &publisher) && !dir.is_empty() {
                out.push((PathBuf::from(dir), name));
            }
        }
    }
    out
}

/// An installed-programs entry that is an EA app or Ubisoft Connect game.
fn is_store_game(key: &str, name: &str, publisher: &str) -> bool {
    let launcher = ["ea app", "origin", "ubisoft connect", "uplay", "ea desktop"]
        .iter()
        .any(|l| name.eq_ignore_ascii_case(l));
    if launcher || name.is_empty() {
        return false;
    }
    key.starts_with("Uplay Install ") || publisher.to_lowercase().contains("electronic arts")
}

/// The executable of the foreground window's process.
#[cfg(windows)]
fn foreground_exe() -> Option<PathBuf> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    // SAFETY: plain Win32 queries; the process handle is closed before returning.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(h);
        ok.ok()?;
        Some(PathBuf::from(String::from_utf16_lossy(
            &buf[..len as usize],
        )))
    }
}

#[cfg(not(windows))]
fn foreground_exe() -> Option<PathBuf> {
    None
}

/// Minimal registry reading (string values and subkey names).
mod reg {
    #[cfg(windows)]
    pub use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER as HKCU, HKEY_LOCAL_MACHINE as HKLM,
    };

    #[cfg(windows)]
    pub fn string(root: HKEY, key: &str, value: &str) -> Option<String> {
        use windows::core::HSTRING;
        use windows::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};
        let mut buf = vec![0u16; 1024];
        let mut bytes = (buf.len() * 2) as u32;
        // SAFETY: the buffer and its size in bytes match.
        let r = unsafe {
            RegGetValueW(
                root,
                &HSTRING::from(key),
                &HSTRING::from(value),
                RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if r.is_err() {
            return None;
        }
        let len = (bytes as usize / 2).saturating_sub(1).min(buf.len());
        buf.truncate(len);
        let s = String::from_utf16_lossy(&buf);
        let s = s.trim_end_matches('\0').trim().to_owned();
        (!s.is_empty()).then_some(s)
    }

    #[cfg(windows)]
    pub fn subkeys(root: HKEY, key: &str) -> Vec<String> {
        use windows::core::{HSTRING, PWSTR};
        use windows::Win32::System::Registry::{
            RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, KEY_READ,
        };
        let mut h = HKEY::default();
        // SAFETY: the key is opened read-only and closed below.
        unsafe {
            if RegOpenKeyExW(root, &HSTRING::from(key), Some(0), KEY_READ, &mut h).is_err() {
                return Vec::new();
            }
            let mut out = Vec::new();
            for i in 0.. {
                let mut name = [0u16; 256];
                let mut len = name.len() as u32;
                let r = RegEnumKeyExW(
                    h,
                    i,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                );
                if r.is_err() {
                    break;
                }
                out.push(String::from_utf16_lossy(&name[..len as usize]));
            }
            let _ = RegCloseKey(h);
            out
        }
    }

    #[cfg(not(windows))]
    pub type HKEY = ();
    #[cfg(not(windows))]
    pub const HKCU: HKEY = ();
    #[cfg(not(windows))]
    pub const HKLM: HKEY = ();
    #[cfg(not(windows))]
    pub fn string(_: HKEY, _: &str, _: &str) -> Option<String> {
        None
    }
    #[cfg(not(windows))]
    pub fn subkeys(_: HKEY, _: &str) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_files_parse() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps" { "730" "123" }
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}"#;
        let steam = Path::new(r"C:\Program Files (x86)\Steam");
        let libs = steam_libraries(steam, vdf);
        assert_eq!(
            libs,
            [steam.to_path_buf(), PathBuf::from(r"D:\SteamLibrary")]
        );

        let acf = r#"
"AppState"
{
	"appid"		"730"
	"name"		"Counter-Strike 2"
	"installdir"		"Counter-Strike Global Offensive"
}"#;
        let (dir, name) = steam_manifest(Path::new(r"D:\SteamLibrary"), acf).unwrap();
        assert_eq!(name, "Counter-Strike 2");
        assert_eq!(
            dir,
            PathBuf::from(r"D:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive")
        );
    }

    #[test]
    fn exe_matches_the_deepest_game_folder() {
        let games = vec![
            Game {
                dir: norm_dir(Path::new(r"D:\Games")),
                name: "Too broad".into(),
            },
            Game {
                dir: norm_dir(Path::new(r"D:\Games\Valorant")),
                name: "VALORANT".into(),
            },
        ];
        let exe = Path::new(
            r"d:\games\valorant\live\ShooterGame\Binaries\Win64\VALORANT-Win64-Shipping.exe",
        );
        assert_eq!(find(&games, exe).as_deref(), Some("VALORANT"));
        // Prefix of a folder name is not a match.
        assert_eq!(
            find(&games[1..], Path::new(r"D:\Games\ValorantTools\x.exe")),
            None
        );
        assert_eq!(find(&games, Path::new(r"C:\Windows\explorer.exe")), None);
    }

    #[test]
    fn store_entries_skip_the_launchers() {
        assert!(is_store_game(
            "Uplay Install 635",
            "Rainbow Six Siege",
            "Ubisoft"
        ));
        assert!(is_store_game("{GUID}", "Apex Legends", "Electronic Arts"));
        assert!(!is_store_game("{GUID}", "EA app", "Electronic Arts"));
        assert!(!is_store_game("Uplay", "Ubisoft Connect", "Ubisoft"));
        assert!(!is_store_game("{GUID}", "7-Zip", "Igor Pavlov"));
    }

    /// Lists the games found in this PC's libraries (read-only).
    #[test]
    #[ignore = "reads this machine's game libraries; run by hand"]
    fn list_installed_games() {
        for g in scan() {
            println!("{:<40} {}", g.name, g.dir);
        }
    }
}
