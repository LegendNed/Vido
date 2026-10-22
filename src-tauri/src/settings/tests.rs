use super::*;

#[test]
fn defaults_are_valid() {
    Settings::default().validate().unwrap();
}

/// Keeps `config/default-settings.json` in sync with `Settings::default()`.
/// Regenerate with `VIDO_UPDATE_DEFAULTS=1 cargo test default_settings_file`.
#[test]
fn default_settings_file_is_current() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../config/default-settings.json");
    let json = serde_json::to_string_pretty(&Settings::default()).unwrap() + "\n";
    if std::env::var_os("VIDO_UPDATE_DEFAULTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &json).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert_eq!(
        on_disk, json,
        "config/default-settings.json is stale; rerun with VIDO_UPDATE_DEFAULTS=1"
    );
    // The shipped file must parse back to the defaults.
    let parsed: Settings = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(parsed, Settings::default());
}

#[test]
fn settings_roundtrip_json() {
    let mut s = Settings::default();
    s.capture.mode = CaptureMode::Region;
    s.capture.region = Some(RegionRect {
        monitor_id: r"\\.\DISPLAY1".into(),
        x: 10,
        y: 20,
        width: 1280,
        height: 720,
    });
    s.capture.scale = ScalePreset::P1080;
    s.export.rate_mode = RateMode::TargetSize;
    let json = serde_json::to_string_pretty(&s).unwrap();
    assert!(json.contains("\"scale\": \"1080p\""));
    assert!(json.contains("\"rateMode\": \"targetSize\""));
    assert!(json.contains("\"saveClip\": \"Ctrl+Shift+F6\""));
    assert!(!json.contains("save30"), "legacy hotkeys are never written");
    let back: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
}

#[test]
fn missing_fields_fall_back_to_defaults() {
    let s: Settings =
        serde_json::from_str(r#"{ "capture": { "fps": 30 }, "unknownField": 1 }"#).unwrap();
    assert_eq!(s.capture.fps, 30);
    assert_eq!(s.capture.buffer_seconds, 30);
    assert_eq!(s.audio, AudioSettings::default());
}

#[test]
fn sidebar_mode_is_lowercase_and_optional() {
    // Settings files written before the option existed keep the full rail.
    let s: Settings = serde_json::from_str(r#"{ "general": { "closeToTray": false } }"#).unwrap();
    assert_eq!(s.general.sidebar, SidebarMode::Full);
    assert!(!s.general.close_to_tray);
    let s: Settings = serde_json::from_str(r#"{ "general": { "sidebar": "none" } }"#).unwrap();
    assert_eq!(s.general.sidebar, SidebarMode::None);
    assert_eq!(
        serde_json::to_string(&SidebarMode::Small).unwrap(),
        "\"small\""
    );
}

#[test]
fn hud_settings_from_older_files() {
    // nabled was renamed to overlay; the clip popup is on for everyone.
    let load = |json: &str| {
        let s: Settings = serde_json::from_str(json).unwrap();
        let (s, notes) = s.repaired();
        s.validate().unwrap();
        (s, notes)
    };
    let (s, notes) = load(
        r#"{ "general": { "hud": { "enabled": false, "position": "bottomLeft",
             "showDetails": false } } }"#,
    );
    assert_eq!(notes.len(), 1, "{notes:?}");
    let h = &s.general.hud;
    assert!(!h.overlay);
    assert!(h.clip_popup);
    assert_eq!(h.position, HudPosition::BottomLeft);
    assert_eq!(
        h,
        &HudSettings {
            overlay: false,
            position: HudPosition::BottomLeft,
            ..Default::default()
        }
    );
    let (s, _) = load(r#"{ "general": { "hud": { "enabled": true } } }"#);
    assert_eq!(
        s.general.hud,
        HudSettings {
            overlay: true,
            ..Default::default()
        }
    );
    // Written back under the new names only.
    let json = serde_json::to_string(&s.general.hud).unwrap();
    assert!(json.contains("\"overlay\":true"), "{json}");
    assert!(json.contains("\"clipPopup\":true"), "{json}");
    assert!(!json.contains("enabled"), "{json}");
    // Current files and the new centre positions.
    let (s, notes) = load(
        r#"{ "general": { "hud": { "overlay": false, "clipPopup": false,
             "position": "topCenter" } } }"#,
    );
    assert!(notes.is_empty(), "{notes:?}");
    assert!(!s.general.hud.overlay && !s.general.hud.clip_popup);
    assert_eq!(s.general.hud.position, HudPosition::TopCenter);
    assert_eq!(
        serde_json::to_string(&HudPosition::BottomCenter).unwrap(),
        "\"bottomCenter\""
    );
}

#[test]
fn rejects_out_of_range_values() {
    let mut s = Settings::default();
    s.capture.buffer_seconds = 3;
    let e = s.validate().unwrap_err();
    assert_eq!(e.field.as_deref(), Some("capture.bufferSeconds"));

    let mut s = Settings::default();
    s.capture.fps = 59;
    assert_eq!(
        s.validate().unwrap_err().field.as_deref(),
        Some("capture.fps")
    );

    let mut s = Settings::default();
    s.audio.codec = AudioCodec::Opus;
    s.audio.sample_rate = 44_100;
    assert_eq!(
        s.validate().unwrap_err().field.as_deref(),
        Some("audio.sampleRate")
    );
}

#[test]
fn rejects_duplicate_hotkeys() {
    let mut s = Settings::default();
    s.hotkeys.open_editor = Some("ctrl+shift+f6".into());
    let e = s.validate().unwrap_err();
    assert_eq!(e.field.as_deref(), Some("hotkeys.openEditor"));
    assert!(e.message.contains("Save clip"), "{}", e.message);
}

/// Loads a settings file with the given `hotkeys` section through the same path as
/// startup (parse + repair).
fn load_hotkeys(hotkeys: &str) -> (HotkeySettings, Vec<String>) {
    let s: Settings = serde_json::from_str(&format!(r#"{{ "hotkeys": {hotkeys} }}"#)).unwrap();
    let (s, notes) = s.repaired();
    s.validate().unwrap();
    (s.hotkeys, notes)
}

#[test]
fn old_default_hotkeys_become_save_clip() {
    let (h, notes) = load_hotkeys(
        r#"{ "save15": "Ctrl+Shift+F5", "save30": "Ctrl+Shift+F6",
             "save60": "Ctrl+Shift+F7", "saveCustom": "Ctrl+Shift+F8", "customSeconds": 0,
             "toggleRecording": "Ctrl+Shift+F9", "togglePause": "Ctrl+Shift+F10",
             "openLastClip": "Ctrl+Shift+F11", "openEditor": "Ctrl+Shift+F12" }"#,
    );
    assert_eq!(h, HotkeySettings::default());
    assert_eq!(notes.len(), 1, "{notes:?}");
    // Written back without the old fields.
    let json = serde_json::to_string(&h).unwrap();
    assert!(
        !json.contains("save30") && !json.contains("customSeconds"),
        "{json}"
    );
}

#[test]
fn custom_save_hotkeys_keep_the_first_bound_one() {
    // 30 s wins over the others.
    let (h, _) = load_hotkeys(
        r#"{ "save15": "Shift+F8", "save30": "F8", "save60": "Ctrl+F8",
             "saveCustom": "Alt+F8", "customSeconds": 120, "toggleRecording": "F9" }"#,
    );
    assert_eq!(h.save_clip.as_deref(), Some("F8"));
    assert_eq!(h.toggle_recording.as_deref(), Some("F9"));
    assert_eq!(h.open_editor.as_deref(), Some("Ctrl+Shift+F12"));

    // Then 60 s, 15 s and custom length.
    let (h, _) = load_hotkeys(
        r#"{ "save15": "Ctrl+1", "save30": null, "save60": "Ctrl+2", "saveCustom": "Ctrl+3" }"#,
    );
    assert_eq!(h.save_clip.as_deref(), Some("Ctrl+2"));
    let (h, _) = load_hotkeys(
        r#"{ "save15": "Ctrl+1", "save30": null, "save60": null, "saveCustom": "Ctrl+3" }"#,
    );
    assert_eq!(h.save_clip.as_deref(), Some("Ctrl+1"));
    let (h, _) = load_hotkeys(
        r#"{ "save15": null, "save30": null, "save60": null, "saveCustom": "Ctrl+3" }"#,
    );
    assert_eq!(h.save_clip.as_deref(), Some("Ctrl+3"));
}

#[test]
fn all_unbound_save_hotkeys_mean_no_save_clip_hotkey() {
    let (h, notes) = load_hotkeys(
        r#"{ "save15": null, "save30": null, "save60": null, "saveCustom": null,
             "customSeconds": 0, "openEditor": null }"#,
    );
    assert_eq!(h.save_clip, None);
    assert_eq!(h.open_editor, None);
    assert_eq!(h.toggle_recording.as_deref(), Some("Ctrl+Shift+F9"));
    assert_eq!(notes.len(), 1);
}

#[test]
fn current_hotkey_files_are_left_alone() {
    let (h, notes) = load_hotkeys(r#"{ "saveClip": "Ctrl+Alt+S" }"#);
    assert_eq!(h.save_clip.as_deref(), Some("Ctrl+Alt+S"));
    assert!(notes.is_empty(), "{notes:?}");
    let (h, notes) = load_hotkeys(r#"{ "saveClip": null }"#);
    assert_eq!(h.save_clip, None);
    assert!(notes.is_empty(), "{notes:?}");
}

fn track(id: &str, kind: TrackKind, device: Option<&str>, enabled: bool) -> AudioTrackSettings {
    AudioTrackSettings {
        device_id: device.map(Into::into),
        ..AudioTrackSettings::new(id, kind, id, enabled)
    }
}

fn field_of(a: &AudioSettings) -> Option<String> {
    a.validate().unwrap_err().field
}

#[test]
fn audio_allows_sixteen_sources() {
    assert_eq!(MAX_AUDIO_TRACKS, 16);
    let mut a = AudioSettings {
        tracks: (0..MAX_AUDIO_TRACKS)
            .map(|i| {
                let kind = if i % 2 == 0 {
                    TrackKind::Desktop
                } else {
                    TrackKind::Microphone
                };
                track(&format!("t{i}"), kind, Some(&format!("{{dev-{i}}}")), true)
            })
            .collect(),
        ..Default::default()
    };
    a.validate().unwrap();
    a.tracks
        .push(track("extra", TrackKind::Desktop, Some("{extra}"), false));
    assert_eq!(field_of(&a).as_deref(), Some("audio.tracks"));
}

#[test]
fn audio_rejects_two_tracks_on_one_device() {
    let mut a = AudioSettings::default();
    // The same endpoint id as output (loopback) and input is two different sources,
    // and "Windows default" is its own entry next to explicit devices.
    a.tracks
        .push(track("out-a", TrackKind::Desktop, Some("{a}"), true));
    a.tracks
        .push(track("in-a", TrackKind::Microphone, Some("{a}"), true));
    a.validate().unwrap();

    let mut b = a.clone();
    b.tracks
        .push(track("out-a2", TrackKind::Desktop, Some("{a}"), false));
    assert_eq!(field_of(&b).as_deref(), Some("audio.tracks.4.deviceId"));

    let mut c = a.clone();
    c.tracks
        .push(track("desktop2", TrackKind::Desktop, None, true));
    assert_eq!(field_of(&c).as_deref(), Some("audio.tracks.4.deviceId"));

    // Legacy `input` tracks are inputs, so they collide with microphone tracks.
    let mut d = a.clone();
    d.tracks
        .push(track("input-1", TrackKind::Input, Some("{a}"), true));
    assert_eq!(field_of(&d).as_deref(), Some("audio.tracks.4.deviceId"));

    // Ids stay unique too.
    let mut e = a.clone();
    e.tracks
        .push(track("out-a", TrackKind::Desktop, Some("{b}"), true));
    assert_eq!(field_of(&e).as_deref(), Some("audio.tracks.4.id"));
}

#[test]
fn legacy_input_tracks_still_load() {
    let s: Settings = serde_json::from_str(
        r#"{ "audio": { "tracks": [
            { "id": "desktop", "kind": "desktop", "name": "Desktop audio", "deviceId": null,
              "enabled": true, "muted": false, "volume": 1, "gainDb": 0, "mono": false,
              "noiseSuppression": false },
            { "id": "input-1", "kind": "input", "name": "Input 1", "deviceId": "{x}",
              "enabled": true, "muted": false, "volume": 1.5, "gainDb": 6, "mono": true,
              "noiseSuppression": true } ] } }"#,
    )
    .unwrap();
    s.validate().unwrap();
    let t = &s.audio.tracks[1];
    assert_eq!(t.kind, TrackKind::Input);
    assert_eq!(t.device_key(), (DeviceKind::Input, Some("{x}")));
    assert_eq!(s.repaired().1, Vec::<String>::new());
}

#[test]
fn repair_keeps_one_track_per_device() {
    let mut s = Settings::default();
    s.audio.bitrate_kbps = 256;
    // Old settings could record the default microphone twice ("mic" is off by default).
    s.audio
        .tracks
        .push(track("input-1", TrackKind::Input, None, true));
    s.audio
        .tracks
        .push(track("desktop-2", TrackKind::Desktop, None, false));
    assert!(s.validate().is_err());
    let (fixed, notes) = s.repaired();
    fixed.validate().unwrap();
    let ids: Vec<&str> = fixed.audio.tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["desktop", "input-1"]);
    assert_eq!(notes.len(), 2, "{notes:?}");
    // The rest of the audio section survives.
    assert_eq!(fixed.audio.bitrate_kbps, 256);
}

#[test]
fn saving_options_from_older_files_get_defaults() {
    // Written before `reencode` and `copyVideo` existed.
    let s: Settings = serde_json::from_str(
        r#"{ "saving": { "saveMode": "reencode", "container": "mkv",
             "postSave": { "openFolder": true } } }"#,
    )
    .unwrap();
    s.validate().unwrap();
    assert_eq!(s.saving.save_mode, SaveMode::Reencode);
    assert_eq!(s.saving.container, Container::Mkv);
    assert_eq!(s.saving.reencode, ExportDefaults::high_quality());
    assert!(s.saving.post_save.open_folder);
    assert!(
        s.saving.post_save.copy_video,
        "copying the video is on by default"
    );
    assert!(!s.saving.post_save.copy_path);
}

#[test]
fn reencode_on_save_defaults_to_high_quality() {
    let d = SavingSettings::default().reencode;
    assert_eq!(d.container, Container::Mp4);
    assert_eq!(d.video_codec, VideoCodec::H264);
    assert_eq!(d.rate_mode, RateMode::Quality);
    assert_eq!(d.quality, 18);
    assert_eq!(d.scale, ScalePreset::Native);
    assert_eq!(d.fps, 0);
    // Keeps every track of the clip it replaces.
    assert!(!d.mix_audio);
    d.validate_at("saving.reencode").unwrap();
}

#[test]
fn reencode_errors_name_their_field() {
    let mut s = Settings::default();
    s.saving.reencode.quality = 99;
    assert_eq!(
        s.validate().unwrap_err().field.as_deref(),
        Some("saving.reencode.quality")
    );
    let mut s = Settings::default();
    s.saving.reencode.container = Container::Webm;
    assert_eq!(
        s.validate().unwrap_err().field.as_deref(),
        Some("saving.reencode.container")
    );
    // Re-encoding may produce WebM even though instant saves can't.
    s.saving.reencode.video_codec = VideoCodec::Av1;
    s.saving.reencode.audio_codec = AudioCodec::Opus;
    s.validate().unwrap();
    // The editor defaults keep their own field names.
    let mut s = Settings::default();
    s.export.fps = 59;
    assert_eq!(
        s.validate().unwrap_err().field.as_deref(),
        Some("export.fps")
    );
}

#[test]
fn repair_resets_only_the_reencode_options() {
    let mut s = Settings::default();
    s.saving.filename_template = "Clip_{time}".into();
    s.saving.reencode.target_size_mb = 0.0;
    let (fixed, notes) = s.repaired();
    fixed.validate().unwrap();
    assert_eq!(fixed.saving.reencode, ExportDefaults::high_quality());
    assert_eq!(fixed.saving.filename_template, "Clip_{time}");
    assert_eq!(notes.len(), 1, "{notes:?}");
}

#[test]
fn video_and_path_share_one_clipboard() {
    // An older file that copied the path also gets the new video copy, which wins.
    let s: Settings =
        serde_json::from_str(r#"{ "saving": { "postSave": { "copyPath": true } } }"#).unwrap();
    assert!(s.saving.post_save.copy_video && s.saving.post_save.copy_path);
    let (fixed, notes) = s.repaired();
    assert!(fixed.saving.post_save.copy_video);
    assert!(!fixed.saving.post_save.copy_path);
    assert_eq!(notes.len(), 1);
    // Path only, video explicitly off: kept as is.
    let s: Settings = serde_json::from_str(
        r#"{ "saving": { "postSave": { "copyPath": true, "copyVideo": false } } }"#,
    )
    .unwrap();
    let (fixed, notes) = s.repaired();
    assert!(fixed.saving.post_save.copy_path && !fixed.saving.post_save.copy_video);
    assert!(notes.is_empty());
}

#[test]
fn repair_resets_only_broken_sections() {
    let mut s = Settings::default();
    s.capture.buffer_seconds = 10_000;
    s.audio.bitrate_kbps = 256;
    let (fixed, notes) = s.repaired();
    assert_eq!(fixed.capture, CaptureSettings::default());
    assert_eq!(fixed.audio.bitrate_kbps, 256);
    assert_eq!(notes.len(), 1);
}

/// `config/export-presets.json`: every preset is a valid set of export
/// options, and "High quality" matches the re-encode-on-save default.
#[test]
fn export_presets_config_is_valid() {
    let presets: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../config/export-presets.json")).unwrap();
    assert!(!presets.is_empty());
    let hq = serde_json::to_value(ExportDefaults::high_quality()).unwrap();
    for p in &presets {
        let id = p["id"].as_str().unwrap();
        assert!(p["label"].is_string() && p["hint"].is_string(), "{id}");
        let mut v = serde_json::to_value(ExportDefaults::default()).unwrap();
        for (k, val) in p["values"].as_object().unwrap() {
            assert!(v.get(k).is_some(), "{id}: unknown option {k}");
            v[k] = val.clone();
            if id == "hq" {
                assert_eq!(
                    &hq[k], val,
                    "hq.{k} differs from ExportDefaults::high_quality()"
                );
            }
        }
        let d: ExportDefaults = serde_json::from_value(v).unwrap();
        d.validate_at(id).unwrap();
    }
}

/// `config/recording-presets.json` and `config/clip-lengths.json` hold valid
/// capture settings; every recommended tier has a preset.
#[test]
fn recording_config_is_valid() {
    #[derive(Deserialize)]
    struct Preset {
        id: String,
        scale: ScalePreset,
        fps: u32,
        kbps: Option<u32>,
    }
    let presets: Vec<Preset> =
        serde_json::from_str(include_str!("../../../config/recording-presets.json")).unwrap();
    for p in &presets {
        let mut c = CaptureSettings {
            scale: p.scale,
            fps: p.fps,
            ..Default::default()
        };
        if let Some(k) = p.kbps {
            c.quality = BufferQuality::Custom;
            c.custom_bitrate_kbps = k;
        }
        c.validate()
            .unwrap_or_else(|e| panic!("{}: {}", p.id, e.message));
    }
    for tier in [QualityTier::Low, QualityTier::Standard, QualityTier::High] {
        let id = serde_json::to_value(tier).unwrap();
        assert!(presets.iter().any(|p| id == p.id.as_str()), "{id}");
    }
    let lengths: Vec<u32> =
        serde_json::from_str(include_str!("../../../config/clip-lengths.json")).unwrap();
    for s in lengths {
        let c = CaptureSettings {
            buffer_seconds: s,
            ..Default::default()
        };
        c.validate()
            .unwrap_or_else(|e| panic!("{s} s: {}", e.message));
    }
}

#[test]
fn clips_and_cache_folders_must_be_specific() {
    let ok = |p: &str| check_folder(&Some(p.into()), "f").is_ok();
    assert!(ok(r"D:\Clips"));
    assert!(!ok(r"C:\"));
    assert!(!ok(r"D:\"));
    assert!(check_folder(&None, "f").is_ok());
    if let Ok(home) = std::env::var("USERPROFILE") {
        assert!(!ok(&home));
        assert!(ok(&format!(r"{home}\Videos\Vido")));
    }
    if let Ok(win) = std::env::var("SystemRoot") {
        assert!(!ok(&win));
        assert!(!ok(&format!(r"{win}\Temp\clips")));
    }
}

#[test]
fn filename_template_validation() {
    assert!(validate_filename_template("Vido_{date}_{time}").is_ok());
    assert!(validate_filename_template("{source} - {duration}").is_ok());
    assert!(validate_filename_template("{game}_{date}_{time}").is_ok());
    assert!(validate_filename_template("").is_err());
    assert!(validate_filename_template("a/b").is_err());
    assert!(validate_filename_template("{nope}").is_err());
    assert!(validate_filename_template("{date").is_err());
}

#[test]
fn encoder_options_parsing() {
    assert_eq!(
        parse_encoder_options("spatial_aq=1; rc-lookahead=20").unwrap(),
        vec![
            ("spatial_aq".into(), "1".into()),
            ("rc-lookahead".into(), "20".into())
        ]
    );
    assert!(parse_encoder_options("bad").is_err());
    assert!(parse_encoder_options("a=b c").is_err());
    assert!(parse_encoder_options("").unwrap().is_empty());
}

#[test]
fn webm_requires_av1_opus() {
    let mut e = ExportDefaults {
        container: Container::Webm,
        ..Default::default()
    };
    assert!(e.validate().is_err());
    e.video_codec = VideoCodec::Av1;
    e.audio_codec = AudioCodec::Opus;
    assert!(e.validate().is_ok());
}

#[test]
fn buffer_bitrate_estimates_are_sane() {
    let c = CaptureSettings {
        quality: BufferQuality::High,
        ..Default::default()
    };
    let k1080 = c.video_bitrate_kbps(1920, 1080, 60.0);
    let k1440 = c.video_bitrate_kbps(2560, 1440, 60.0);
    assert!((15_000..=30_000).contains(&k1080), "{k1080}");
    assert!(k1440 > k1080);
    let custom = CaptureSettings {
        quality: BufferQuality::Custom,
        custom_bitrate_kbps: 12_345,
        ..Default::default()
    };
    assert_eq!(custom.video_bitrate_kbps(1920, 1080, 60.0), 12_345);
}

#[test]
fn recommended_quality_follows_the_hardware() {
    assert_eq!(recommend_tier(false, 24.0, 2160), QualityTier::Low);
    assert_eq!(recommend_tier(true, 8.0, 1440), QualityTier::Standard);
    assert_eq!(recommend_tier(true, 32.0, 1080), QualityTier::Standard);
    assert_eq!(recommend_tier(true, 32.0, 1440), QualityTier::High);
}
