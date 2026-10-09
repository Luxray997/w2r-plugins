//! The saved settings and their text form. Everything read back from disk is untrusted: a
//! value that does not parse, or is out of range, is dropped and the game keeps its default.

/// Largest value of the game-speed slider (`0..=8`, as on the create screen).
pub const MAX_SPEED: u32 = 8;
/// Longest dropdown value we accept; the game's own keys are short words.
pub const MAX_VALUE_LEN: usize = 32;
/// Longest map folder, name or path we keep (keeps the saved block under the 1,024-byte settings limit).
pub const MAX_MAP_TEXT: usize = 200;
/// The settings key holding the encoded block.
pub const KEY: &str = "last";

/// Where a map lives in the game's list: the folder the list was showing, and the entry's name
/// and path. The entry is found again by comparing both, so a moved or removed map is skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapRef {
    pub folder: String,
    pub name: String,
    pub path: String,
}

/// Map text is printable ASCII only: it is compared with the game's own strings, never opened.
pub fn valid_map_text(v: &str) -> bool {
    !v.is_empty() && v.len() <= MAX_MAP_TEXT && v.bytes().all(|b| (0x20..0x7f).contains(&b))
}

impl MapRef {
    pub fn valid(&self) -> bool {
        valid_map_text(&self.folder) && valid_map_text(&self.name) && valid_map_text(&self.path)
    }
}

/// The last created game's options. `None` means "not saved, leave the game's value alone".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub speed: Option<u32>,
    pub resources: Option<String>,
    pub units: Option<String>,
    pub tileset: Option<String>,
    pub start_location: Option<String>,
    pub map: Option<MapRef>,
}

/// A dropdown value is a short key such as `"Forest"`; anything else is not ours to write into
/// the game.
pub fn valid_value(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= MAX_VALUE_LEN
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b' ' | b'.'))
}

impl Saved {
    pub fn is_empty(&self) -> bool {
        *self == Saved::default()
    }

    /// `key=value` lines, in a fixed order.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        if let Some(v) = self.speed {
            out.push_str(&format!("speed={v}\n"));
        }
        for (key, value) in [
            ("resources", &self.resources),
            ("units", &self.units),
            ("tileset", &self.tileset),
            ("start", &self.start_location),
        ] {
            if let Some(v) = value {
                out.push_str(&format!("{key}={v}\n"));
            }
        }
        if let Some(m) = self.map.as_ref().filter(|m| m.valid()) {
            out.push_str(&format!(
                "mapfolder={}\nmapname={}\nmappath={}\n",
                m.folder, m.name, m.path
            ));
        }
        out
    }

    /// Parses what `encode` wrote. Unknown keys, bad values and malformed lines are ignored.
    pub fn decode(text: &str) -> Saved {
        let mut saved = Saved::default();
        let (mut folder, mut name, mut path) = (None, None, None);
        for line in text.lines().take(16) {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "speed" => {
                    saved.speed = value.parse::<u32>().ok().filter(|&v| v <= MAX_SPEED);
                }
                "resources" | "units" | "tileset" | "start" if valid_value(value) => {
                    let slot = match key {
                        "resources" => &mut saved.resources,
                        "units" => &mut saved.units,
                        "tileset" => &mut saved.tileset,
                        _ => &mut saved.start_location,
                    };
                    *slot = Some(value.to_string());
                }
                "mapfolder" | "mapname" | "mappath" if valid_map_text(value) => {
                    let slot = match key {
                        "mapfolder" => &mut folder,
                        "mapname" => &mut name,
                        _ => &mut path,
                    };
                    *slot = Some(value.to_string());
                }
                _ => {}
            }
        }
        if let (Some(folder), Some(name), Some(path)) = (folder, name, path) {
            saved.map = Some(MapRef { folder, name, path });
        }
        saved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Saved {
        Saved {
            speed: Some(5),
            resources: Some("High".into()),
            units: Some("Peasants Only".into()),
            tileset: Some("Forest".into()),
            start_location: Some("Fixed".into()),
            map: Some(MapRef {
                folder: "Maps\\Custom".into(),
                name: "Gold Rush = 4".into(),
                path: "Maps\\Custom\\goldrush.pud".into(),
            }),
        }
    }

    #[test]
    fn map_needs_all_three_parts() {
        let text = "mapfolder=Maps\nmapname=Foo\n";
        assert_eq!(Saved::decode(text).map, None);
        let text = "mapfolder=Maps\nmapname=Foo\nmappath=Maps\\foo.pud\n";
        assert!(Saved::decode(text).map.is_some());
    }

    #[test]
    fn map_text_is_bounded_printable_ascii() {
        let long = "a".repeat(MAX_MAP_TEXT + 1);
        assert!(!valid_map_text(&long));
        assert!(!valid_map_text("caf\u{e9}"));
        assert!(!valid_map_text("a\tb"));
        let text = format!("mapfolder={long}\nmapname=Foo\nmappath=p\n");
        assert_eq!(Saved::decode(&text).map, None);
    }

    #[test]
    fn round_trips() {
        assert_eq!(Saved::decode(&full().encode()), full());
    }

    #[test]
    fn empty_encodes_to_nothing() {
        assert_eq!(Saved::default().encode(), "");
        assert!(Saved::decode("").is_empty());
    }

    #[test]
    fn speed_out_of_range_is_dropped() {
        assert_eq!(Saved::decode("speed=9\n").speed, None);
        assert_eq!(Saved::decode("speed=-1\n").speed, None);
        assert_eq!(Saved::decode("speed=x\n").speed, None);
        assert_eq!(Saved::decode("speed=8\n").speed, Some(8));
        assert_eq!(Saved::decode("speed=0\n").speed, Some(0));
    }

    #[test]
    fn hostile_values_are_dropped() {
        let long = "a".repeat(MAX_VALUE_LEN + 1);
        let text = format!("resources={long}\nunits=bad\u{0}\ntileset=\nstart=ok\nunknown=1\nnoequals\n");
        let saved = Saved::decode(&text);
        assert_eq!(saved.resources, None);
        assert_eq!(saved.units, None);
        assert_eq!(saved.tileset, None);
        assert_eq!(saved.start_location.as_deref(), Some("ok"));
    }

    #[test]
    fn value_charset() {
        assert!(valid_value("Peasants Only"));
        assert!(!valid_value(""));
        assert!(!valid_value("a=b"));
        assert!(!valid_value("a\nb"));
        assert!(!valid_value("é"));
    }
}
