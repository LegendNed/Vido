//! Which game is being played, for sorting clips into per-game folders.
//!
//! **Is it a game?** The foreground program counts as one when Windows' Game Bar
//! recognised it (`HKCU\System\GameConfigStore`, the list behind Win+G) or when it
//! runs from a folder a game library installed (Steam, Epic, GOG, EA app, Ubisoft
//! Connect). Window titles and exe names never decide this: every program has them.
//!
//! **What is it called?** The first of: the library's official name, the name
//! remembered for that exe (`games.json`, so a game keeps one folder even when its
//! window title changes), a few built-in names for odd cases, the cleaned-up
//! window title, the exe's product name, its folder, the exe name.
//!
//! Everything is read locally; nothing is fetched from the internet. The
//! foreground window is checked when a clip is saved, and every few seconds in the
//! background so a clip saved from the tray still gets the game played last.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// An installed game: its folder (lowercase, ending in `\`) and its name.
#[derive(Clone, Debug, PartialEq)]
struct Game {
    dir: String,
    name: String,
}

struct Detector {
    /// Games from the launcher libraries.
    games: Vec<Game>,
    /// Executables Game Bar recognised as games (lowercase full paths).
    gamebar: Vec<String>,
    scanned: Option<Instant>,
    /// Names given to executables (lowercase path → name), kept in `games.json`.
    names: Option<HashMap<String, String>>,
    /// Last game seen in the foreground, and when.
    last: Option<(String, Instant)>,
}

static DETECTOR: Mutex<Detector> = Mutex::new(Detector {
    games: Vec::new(),
    gamebar: Vec::new(),
    scanned: None,
    names: None,
    last: None,
});

/// Where remembered names are stored (set once at startup).
static NAMES_FILE: OnceLock<PathBuf> = OnceLock::new();

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
/// `data_dir` holds `games.json` with the remembered names.
pub fn start_sampler(data_dir: &Path) {
    let _ = NAMES_FILE.set(data_dir.join("games.json"));
    let _ = std::thread::Builder::new()
        .name("vido-games".into())
        .spawn(|| loop {
            sample();
            std::thread::sleep(SAMPLE);
        });
}

/// The game in the foreground right now (remembered as the last one seen).
fn sample() -> Option<String> {
    let (exe, title) = foreground()?;
    let key = norm_path(&exe);
    let mut d = DETECTOR.lock();
    let mut library = find(&d.games, &key);
    let mut known = library.is_some() || d.gamebar.contains(&key);
    if !known && d.scanned.is_none_or(|t| t.elapsed() >= RESCAN) {
        d.games = scan();
        d.gamebar = gamebar_games();
        d.scanned = Some(Instant::now());
        tracing::debug!(
            "games: {} from libraries, {} from Game Bar",
            d.games.len(),
            d.gamebar.len()
        );
        library = find(&d.games, &key);
        known = library.is_some() || d.gamebar.contains(&key);
    }
    if !known {
        return None;
    }
    let name = match library {
        Some(n) => n,
        None => remembered_name(&mut d, &key, &exe, &title),
    };
    d.last = Some((name.clone(), Instant::now()));
    Some(name)
}

/// The remembered name for `exe`, or a new one (then remembered).
fn remembered_name(d: &mut Detector, key: &str, exe: &Path, title: &str) -> String {
    let names = d.names.get_or_insert_with(load_names);
    if let Some(n) = names.get(key) {
        return n.clone();
    }
    let name = name_for(exe, title);
    names.insert(key.to_owned(), name.clone());
    if let Some(file) = NAMES_FILE.get() {
        if let Ok(json) = serde_json::to_string_pretty(&*names) {
            let _ = std::fs::write(file, json);
        }
    }
    tracing::info!("new game: {name} ({})", exe.display());
    name
}

fn load_names() -> HashMap<String, String> {
    NAMES_FILE
        .get()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// A name for a game no library knows: built-in names for odd cases, the window
/// title, the exe's product name, its folder, the exe name.
fn name_for(exe: &Path, title: &str) -> String {
    let path = norm_path(exe);
    let file = exe
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    // Games whose exe, title and product name all say something else.
    let fixed = [
        ("javaw.exe", "minecraft", "Minecraft"),
        ("javaw.exe", "microsoft.4297127d64ec6", "Minecraft"),
        ("robloxplayerbeta.exe", "", "Roblox"),
    ];
    if let Some((_, _, n)) = fixed
        .iter()
        .find(|(f, p, _)| file == *f && path.contains(p))
    {
        return (*n).to_owned();
    }
    clean_title(title)
        .or_else(|| product_name(exe))
        .or_else(|| folder_name(exe))
        .unwrap_or_else(|| {
            exe.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Game".into())
        })
}

/// "Minecraft 1.21.1 - Multiplayer" → "Minecraft"; generic titles give nothing.
fn clean_title(title: &str) -> Option<String> {
    let first = [" - ", " | ", " — ", " – "]
        .iter()
        .filter_map(|sep| title.find(sep))
        .min()
        .map_or(title, |i| &title[..i]);
    let words: Vec<&str> = first
        .split_whitespace()
        .filter(|w| {
            // Version numbers: "1.21.1", "v2.0", "(64-bit)".
            let w = w.trim_matches(|c| c == '(' || c == ')');
            let v = w.strip_prefix(['v', 'V']).unwrap_or(w);
            !(v.contains('.') && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
                && !w.eq_ignore_ascii_case("64-bit")
                && !w.eq_ignore_ascii_case("32-bit")
        })
        .collect();
    let t = words.join(" ");
    let generic = [
        "",
        "game",
        "unrealwindow",
        "unity",
        "direct3d",
        "opengl",
        "vulkan",
    ];
    let lower = t.to_lowercase();
    (t.chars().count() >= 2 && !generic.contains(&lower.as_str())).then_some(t)
}

/// Product names that belong to an engine or runtime, not the game.
fn is_generic_product(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "openjdk",
        "java",
        "unreal engine",
        "ue4",
        "ue5",
        "unity",
        "electron",
        "microsoft®",
        "bootstrap",
        "godot",
        "nw.js",
        "chromium",
    ]
    .iter()
    .any(|g| n.contains(g))
        || n.trim().len() < 2
}

/// The game's folder, skipping engine folders, Unreal's project folder (a code
/// name: "…\Palworld\Pal\Binaries\Win64\x.exe" → "Palworld") and version suffixes
/// ("Hydroneer.v3.0.9" → "Hydroneer").
fn folder_name(exe: &Path) -> Option<String> {
    let generic = [
        "binaries",
        "win64",
        "win32",
        "x64",
        "x86",
        "bin",
        "bin64",
        "game",
        "engine",
        "common",
        "retail",
        "release",
        "shipping",
        "client",
        "build",
        "work",
        "steamapps",
    ];
    let dirs: Vec<String> = exe
        .parent()?
        .ancestors()
        .filter_map(|p| p.file_name())
        .map(|f| f.to_string_lossy().into_owned())
        .collect();
    let start = dirs
        .iter()
        .position(|d| d.eq_ignore_ascii_case("binaries"))
        .map_or(0, |i| i + 2);
    dirs.get(start..)?
        .iter()
        .filter(|d| !generic.contains(&d.to_lowercase().as_str()))
        .find_map(|d| clean_title(&strip_version(d)))
}

/// Cuts a version suffix off a folder name: "Hydroneer.v3.0.9", "Vein v0.022h5".
fn strip_version(name: &str) -> String {
    let lower = name.to_lowercase();
    let cut = [".v", " v", "_v", "-v"]
        .iter()
        .filter_map(|sep| {
            lower.match_indices(sep).find_map(|(i, _)| {
                let next = lower[i + sep.len()..].chars().next()?;
                next.is_ascii_digit().then_some(i)
            })
        })
        .min();
    match cut {
        Some(i) => name[..i].to_owned(),
        None => name.to_owned(),
    }
}

fn norm_path(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

fn norm_dir(p: &Path) -> String {
    let mut s = norm_path(p);
    if !s.ends_with('\\') {
        s.push('\\');
    }
    s
}

/// The library game whose folder holds `exe` (lowercase; the deepest folder wins).
fn find(games: &[Game], exe: &str) -> Option<String> {
    games
        .iter()
        .filter(|g| exe.starts_with(&g.dir))
        .max_by_key(|g| g.dir.len())
        .map(|g| g.name.clone())
}

/// Executables Game Bar recognised as games (Windows' GameConfigStore).
fn gamebar_games() -> Vec<String> {
    let base = r"System\GameConfigStore\Children";
    // Tools that Game Bar lists but that aren't games.
    let not_games = ["robloxstudiobeta.exe"];
    reg::subkeys(reg::HKCU, base)
        .into_iter()
        .filter_map(|id| reg::string(reg::HKCU, &format!(r"{base}\{id}"), "MatchedExeFullPath"))
        .map(|p| norm_path(Path::new(&p)))
        .filter(|p| !not_games.iter().any(|n| p.ends_with(n)))
        .collect()
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

/// The foreground window's executable and title (not Vido's own windows).
#[cfg(windows)]
fn foreground() -> Option<(PathBuf, String)> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

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
        let exe = PathBuf::from(String::from_utf16_lossy(&buf[..len as usize]));
        let mut t = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut t);
        let title = String::from_utf16_lossy(&t[..n.max(0) as usize]);
        Some((exe, title))
    }
}

#[cfg(not(windows))]
fn foreground() -> Option<(PathBuf, String)> {
    None
}

/// The exe's "Product name" (its version resource), unless it names an engine.
#[cfg(windows)]
fn product_name(exe: &Path) -> Option<String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    let path = HSTRING::from(exe.as_os_str());
    // SAFETY: the buffer is sized by GetFileVersionInfoSizeW; VerQueryValueW
    // returns pointers into it, read while it is alive.
    unsafe {
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()).ok()?;
        let query = |q: &str| -> Option<(*const u8, u32)> {
            let mut ptr = std::ptr::null_mut();
            let mut len = 0u32;
            let q = HSTRING::from(q);
            VerQueryValueW(data.as_ptr().cast(), PCWSTR(q.as_ptr()), &mut ptr, &mut len)
                .as_bool()
                .then_some((ptr.cast::<u8>().cast_const(), len))
        };
        // The first language/code page pair, else US English Unicode.
        let lang = query(r"\VarFileInfo\Translation")
            .filter(|(_, len)| *len >= 4)
            .map(|(p, _)| {
                let w = std::slice::from_raw_parts(p.cast::<u16>(), 2);
                format!("{:04x}{:04x}", w[0], w[1])
            })
            .unwrap_or_else(|| "040904b0".into());
        let (p, len) = query(&format!(r"\StringFileInfo\{lang}\ProductName"))?;
        let s = std::slice::from_raw_parts(p.cast::<u16>(), len as usize);
        let name = String::from_utf16_lossy(s)
            .trim_end_matches('\0')
            .trim()
            .to_owned();
        (!is_generic_product(&name)).then_some(name)
    }
}

#[cfg(not(windows))]
fn product_name(_exe: &Path) -> Option<String> {
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
        let exe = norm_path(Path::new(
            r"D:\Games\Valorant\live\ShooterGame\Binaries\Win64\VALORANT-Win64-Shipping.exe",
        ));
        assert_eq!(find(&games, &exe).as_deref(), Some("VALORANT"));
        // Prefix of a folder name is not a match.
        let tools = norm_path(Path::new(r"D:\Games\ValorantTools\x.exe"));
        assert_eq!(find(&games[1..], &tools), None);
        assert_eq!(find(&games, r"c:\windows\explorer.exe"), None);
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

    #[test]
    fn names_for_games_no_library_knows() {
        // Titles: cut at the separator, version numbers dropped, generic ones ignored.
        assert_eq!(
            clean_title("Minecraft 1.21.1 - Multiplayer (3rd-party Server)").as_deref(),
            Some("Minecraft")
        );
        assert_eq!(clean_title("Teardown").as_deref(), Some("Teardown"));
        assert_eq!(
            clean_title("Hydroneer v3.0.9 | Steam").as_deref(),
            Some("Hydroneer")
        );
        assert_eq!(clean_title("UnrealWindow"), None);
        assert_eq!(clean_title(""), None);
        // Folders: engine folders are skipped.
        let exe =
            Path::new(r"D:\HydraGames\Bodycam\Bodycam\Binaries\Win64\Bodycam-Win64-Shipping.exe");
        assert_eq!(folder_name(exe).as_deref(), Some("Bodycam"));
        let pal = Path::new(
            r"D:\SteamLibrary\steamapps\common\Palworld\Pal\Binaries\Win64\Palworld-Win64-Shipping.exe",
        );
        assert_eq!(folder_name(pal).as_deref(), Some("Palworld"));
        let hydro = Path::new(
            r"D:\Games\Hydroneer.v3.0.9\game\Mining\Binaries\Win64\Mining-Win64-Shipping.exe",
        );
        assert_eq!(folder_name(hydro).as_deref(), Some("Hydroneer"));
        let side = Path::new(r"D:\Games\Side Effects v1.03\Side Effects\x.exe");
        assert_eq!(folder_name(side).as_deref(), Some("Side Effects"));
        // A version-only folder is skipped.
        let beat = Path::new(r"C:\BSManager\Beat Saber\1.40.8\Beat Saber.exe");
        assert_eq!(folder_name(beat).as_deref(), Some("Beat Saber"));
        // Built-in names win over everything that isn't a library name.
        let mc = Path::new(
            r"D:\WpSystem\S-1\AppData\Local\Packages\Microsoft.4297127D64EC6_8wekyb3d8bbwe\LocalCache\Local\runtime\java-runtime-epsilon\windows-x64\java-runtime-epsilon\bin\javaw.exe",
        );
        assert_eq!(name_for(mc, "Minecraft* 1.21"), "Minecraft");
        let roblox =
            Path::new(r"C:\Users\x\AppData\Local\Roblox\Versions\version-1\RobloxPlayerBeta.exe");
        assert_eq!(name_for(roblox, "Roblox"), "Roblox");
        // No title: the folder (the exe here has no version resource).
        let missing = Path::new(r"D:\Games\Teardown\teardown.exe");
        assert_eq!(name_for(missing, ""), "Teardown");
        assert!(is_generic_product("OpenJDK Platform binary"));
        assert!(is_generic_product("Unreal Engine"));
        assert!(!is_generic_product("Teardown"));
    }

    /// Lists the games found in this PC's libraries and Game Bar, with the names
    /// Vido would give the Game Bar ones (read-only).
    #[test]
    #[ignore = "reads this machine's game libraries; run by hand"]
    fn list_installed_games() {
        for g in scan() {
            println!("library  {:<40} {}", g.name, g.dir);
        }
        let mut gb: Vec<_> = gamebar_games().into_iter().collect();
        gb.sort();
        for p in gb {
            let path = Path::new(&p);
            println!("gamebar  {:<40} {}", name_for(path, ""), p);
        }
    }
}
