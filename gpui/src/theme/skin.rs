//! Appearance preferences belong to this local UI, not provider configuration.
use serde::{Deserialize, Serialize};
use std::{cell::Cell, io::Read, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skin {
    Blue,
    Violet,
    Rose,
    Amber,
    Teal,
    #[default]
    Monochrome,
    StudioIndigo,
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub primary: u32,
    pub dark: u32,
    pub tint: u32,
    pub border: u32,
    pub ring: u32,
}

impl Skin {
    pub const ALL: [Self; 7] = [
        Self::Monochrome,
        Self::Blue,
        Self::Violet,
        Self::Rose,
        Self::Amber,
        Self::Teal,
        Self::StudioIndigo,
    ];

    pub fn label(self, zh: bool) -> &'static str {
        match (self, zh) {
            (Self::Blue, true) => "蓝白",
            (Self::Blue, false) => "Blue",
            (Self::Violet, true) => "紫藤",
            (Self::Violet, false) => "Violet",
            (Self::Rose, true) => "玫瑰",
            (Self::Rose, false) => "Rose",
            (Self::Amber, true) => "琥珀",
            (Self::Amber, false) => "Amber",
            (Self::Teal, true) => "青绿",
            (Self::Teal, false) => "Teal",
            (Self::Monochrome, true) => "黑白",
            (Self::Monochrome, false) => "Mono",
            (Self::StudioIndigo, true) => "Studio 靛蓝",
            (Self::StudioIndigo, false) => "Studio Indigo",
        }
    }

    pub fn palette(self) -> Palette {
        let (primary, dark, tint, border, ring) = match self {
            Self::Blue => (0x2563eb, 0x1d4ed8, 0xeff6ff, 0xbfdbfe, 0xdbeafe),
            Self::Violet => (0x7c3aed, 0x6d28d9, 0xf5f3ff, 0xddd6fe, 0xede9fe),
            Self::Rose => (0xbe185d, 0x9d174d, 0xfff1f2, 0xfecdd3, 0xffe4e6),
            Self::Amber => (0xb45309, 0x92400e, 0xfffbeb, 0xfde68a, 0xfef3c7),
            Self::Teal => (0x0f766e, 0x115e59, 0xf0fdfa, 0x99f6e4, 0xccfbf1),
            Self::Monochrome => (0x27272a, 0x18181b, 0xf4f4f5, 0xd4d4d8, 0xe4e4e7),
            // Opt-in V2 variables: accent, accent-dark, soft-blue, line-strong.
            Self::StudioIndigo => (0x655cf4, 0x4f46d9, 0xf4f6ff, 0xd8dde9, 0xf4f6ff),
        };
        Palette {
            primary,
            dark,
            tint,
            border,
            ring,
        }
    }
}

// GPUI renders and changes the theme on its UI thread. This small snapshot lets
// stateless render helpers share the palette without another theme/config store.
thread_local! {
    static CURRENT: Cell<Skin> = const { Cell::new(Skin::Monochrome) };
}

pub fn current_skin() -> Skin {
    CURRENT.get()
}

pub fn palette() -> Palette {
    current_skin().palette()
}

pub(super) fn select(skin: Skin) {
    CURRENT.set(skin);
}

const FILE: &str = "appearance.json";

#[derive(Serialize, Deserialize)]
struct Preferences {
    version: u8,
    skin: Skin,
}

pub fn load_skin(dir: &Path) -> Skin {
    let read = || -> Option<Skin> {
        let file = std::fs::File::open(dir.join(FILE)).ok()?;
        let mut bytes = Vec::new();
        file.take(4097).read_to_end(&mut bytes).ok()?;
        if bytes.len() > 4096 {
            return None;
        }
        let preferences: Preferences = serde_json::from_slice(&bytes).ok()?;
        (preferences.version == 1).then_some(preferences.skin)
    };
    read().unwrap_or_default()
}

pub fn save_skin(dir: &Path, skin: Skin) -> Result<(), String> {
    crate::config::write_json(&dir.join(FILE), &Preferences { version: 1, skin })
}

#[cfg(test)]
mod tests {
    use super::{Skin, load_skin, save_skin};

    #[test]
    fn appearance_roundtrips_without_touching_provider_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.json");
        std::fs::write(&config, b"provider fixture").unwrap();
        assert_eq!(load_skin(dir.path()), Skin::Monochrome);
        for skin in Skin::ALL {
            save_skin(dir.path(), skin).unwrap();
            assert_eq!(load_skin(dir.path()), skin);
        }
        assert_eq!(std::fs::read(config).unwrap(), b"provider fixture");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn legacy_appearance_files_load_without_migration_or_rewriting() {
        let dir = tempfile::tempdir().unwrap();
        for (key, expected) in [
            ("blue", Skin::Blue),
            ("violet", Skin::Violet),
            ("rose", Skin::Rose),
            ("amber", Skin::Amber),
            ("teal", Skin::Teal),
            ("monochrome", Skin::Monochrome),
        ] {
            let bytes = format!(r#"{{"version":1,"skin":"{key}"}}"#).into_bytes();
            let path = dir.path().join("appearance.json");
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(load_skin(dir.path()), expected);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        assert_eq!(Skin::default(), Skin::Monochrome);
        save_skin(dir.path(), Skin::StudioIndigo).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("appearance.json")).unwrap())
                .unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["skin"], "studio_indigo");
    }

    #[test]
    fn damaged_or_future_preferences_fall_back_to_monochrome() {
        let dir = tempfile::tempdir().unwrap();
        for text in [
            "{",
            r#"{"version":1,"skin":"unknown"}"#,
            r#"{"version":2,"skin":"teal"}"#,
            r#"{"skin":"teal"}"#,
        ] {
            std::fs::write(dir.path().join("appearance.json"), text).unwrap();
            assert_eq!(load_skin(dir.path()), Skin::Monochrome);
        }
        std::fs::write(dir.path().join("appearance.json"), vec![b' '; 4097]).unwrap();
        assert_eq!(load_skin(dir.path()), Skin::Monochrome);
        assert!(save_skin(&dir.path().join("missing"), Skin::Rose).is_err());
    }

    #[test]
    fn preset_button_and_selected_text_have_readable_contrast() {
        fn luminance(hex: u32) -> f64 {
            [16, 8, 0]
                .into_iter()
                .zip([0.2126, 0.7152, 0.0722])
                .map(|(shift, weight)| {
                    let c = ((hex >> shift) & 255) as f64 / 255.;
                    weight
                        * if c <= 0.04045 {
                            c / 12.92
                        } else {
                            ((c + 0.055) / 1.055).powf(2.4)
                        }
                })
                .sum()
        }
        for skin in Skin::ALL {
            let palette = skin.palette();
            assert!(
                1.05 / (luminance(palette.primary) + 0.05) >= 4.5,
                "{skin:?}"
            );
            assert!(
                (luminance(palette.tint) + 0.05) / (luminance(palette.dark) + 0.05) >= 4.5,
                "{skin:?}"
            );
        }
    }
}
