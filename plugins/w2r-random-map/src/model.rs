//! The plugin's behaviour, independent of the host: what to show each frame and which map to
//! pick. The host sits behind [`Maps`] and [`Rows`], so everything here is tested with fakes.

/// The most players a map has.
pub const MAX_PLAYERS: i32 = 8;

/// Why the host would not answer a map question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    /// The host is still building its map index; ask again next frame.
    Busy,
    /// Anything else (an older host, not inside the create screen).
    Failed,
}

/// The host's map index, filtered by maximum players (0 = any).
pub trait Maps {
    fn count(&mut self, players: u32, folder: &str) -> Result<u32, Unavailable>;
    /// True when the `nth` such map is now the selected one.
    fn select(&mut self, players: u32, folder: &str, nth: u32) -> bool;
}

/// The rows the host draws for us.
pub trait Rows {
    fn label(&mut self, text: &str);
    /// True when the value changed this frame.
    fn slider(&mut self, text: &str, value: &mut i32, min: i32, max: i32) -> bool;
    /// True when pressed this frame.
    fn button(&mut self, text: &str) -> bool;
}

/// A small, fast, non-cryptographic generator (SplitMix64). Picking a map needs nothing better.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..n`; `n` must be above zero.
    pub fn below(&mut self, n: u32) -> u32 {
        (self.next_u64() % u64::from(n)) as u32
    }
}

/// Which of `count` maps to pick. When there is a choice it is never `last`, so pressing the
/// button twice does not look like nothing happened.
pub fn pick(count: u32, last: Option<u32>, rng: &mut Rng) -> u32 {
    debug_assert!(count > 0);
    match last {
        Some(last) if count > 1 && last < count => {
            let n = rng.below(count - 1);
            if n >= last { n + 1 } else { n }
        }
        _ => rng.below(count),
    }
}

/// The longest folder setting kept (the edit box holds one byte less).
pub const FOLDER_CAP: usize = 128;

/// A folder setting as typed: trimmed, `/` turned into `\`, no leading or trailing separators.
pub fn clean_folder(text: &str) -> String {
    text.trim()
        .replace('/', "\\")
        .trim_matches('\\')
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(FOLDER_CAP - 1)
        .collect()
}

/// The saved slider value, or "any" when it is missing or out of range.
pub fn valid_players(v: i64) -> i32 {
    if (0..=i64::from(MAX_PLAYERS)).contains(&v) {
        v as i32
    } else {
        0
    }
}

fn players_text(players: i32) -> String {
    if players == 0 {
        "Random map: any number of players".to_owned()
    } else {
        format!("Random map: {players} players")
    }
}

/// The plugin's state between frames.
#[derive(Clone, Debug)]
pub struct Controller {
    players: i32,
    /// The index among the filtered maps that the last press picked.
    last: Option<u32>,
    rng: Rng,
    /// Only maps in this folder (under the maps folder) and below; empty = all.
    folder: String,
}

impl Controller {
    pub fn new(players: i32, seed: u64) -> Controller {
        Controller {
            players: players.clamp(0, MAX_PLAYERS),
            last: None,
            rng: Rng::new(seed),
            folder: String::new(),
        }
    }

    pub fn folder(&self) -> &str {
        &self.folder
    }

    /// Restricts the pick to `folder` (as typed; cleaned here). True when it changed.
    pub fn set_folder(&mut self, folder: &str) -> bool {
        let folder = clean_folder(folder);
        if folder == self.folder {
            return false;
        }
        self.folder = folder;
        self.last = None;
        true
    }

    pub fn players(&self) -> i32 {
        self.players
    }

    /// One frame of the create screen. Returns true when the slider changed (to be saved).
    pub fn frame(&mut self, rows: &mut dyn Rows, maps: &mut dyn Maps) -> bool {
        let mut players = self.players;
        let changed = rows.slider(&players_text(players), &mut players, 0, MAX_PLAYERS);
        if changed {
            self.players = players.clamp(0, MAX_PLAYERS);
            self.last = None;
        }
        let filter = self.players as u32;
        match maps.count(filter, &self.folder) {
            Err(Unavailable::Busy) => rows.label("Indexing maps..."),
            Err(Unavailable::Failed) => rows.label("Random map is not available"),
            Ok(0) if !self.folder.is_empty() => rows.label(&format!(
                "No {}maps found in \"{}\"",
                if filter == 0 {
                    String::new()
                } else {
                    format!("{filter}-player ")
                },
                self.folder
            )),
            Ok(0) => rows.label(&format!("No {filter}-player maps found")),
            Ok(count) => {
                let text = format!("Random map ({count})");
                if rows.button(&text) {
                    let nth = pick(count, self.last, &mut self.rng);
                    if maps.select(filter, &self.folder, nth) {
                        self.last = Some(nth);
                    }
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeRows {
        labels: Vec<String>,
        buttons: Vec<String>,
        slider_texts: Vec<String>,
        set_slider: Option<i32>,
        press: bool,
    }

    impl Rows for FakeRows {
        fn label(&mut self, text: &str) {
            self.labels.push(text.to_owned());
        }
        fn slider(&mut self, text: &str, value: &mut i32, _min: i32, _max: i32) -> bool {
            self.slider_texts.push(text.to_owned());
            match self.set_slider.take() {
                Some(v) => {
                    *value = v;
                    true
                }
                None => false,
            }
        }
        fn button(&mut self, text: &str) -> bool {
            self.buttons.push(text.to_owned());
            core::mem::take(&mut self.press)
        }
    }

    struct FakeMaps {
        /// Maximum players of each indexed map.
        maps: Vec<u32>,
        busy: bool,
        selected: Vec<(u32, u32)>,
        /// Folder of each map, parallel to `maps`.
        dirs: Vec<&'static str>,
        folders_seen: Vec<String>,
    }

    impl FakeMaps {
        fn new(maps: &[u32]) -> FakeMaps {
            FakeMaps {
                maps: maps.to_vec(),
                busy: false,
                selected: Vec::new(),
                dirs: vec![""; maps.len()],
                folders_seen: Vec::new(),
            }
        }
        fn matching(&self, players: u32, folder: &str) -> Vec<usize> {
            (0..self.maps.len())
                .filter(|&i| players == 0 || self.maps[i] == players)
                .filter(|&i| folder.is_empty() || self.dirs[i] == folder)
                .collect()
        }
    }

    impl Maps for FakeMaps {
        fn count(&mut self, players: u32, folder: &str) -> Result<u32, Unavailable> {
            self.folders_seen.push(folder.to_owned());
            if self.busy {
                return Err(Unavailable::Busy);
            }
            Ok(self.matching(players, folder).len() as u32)
        }
        fn select(&mut self, players: u32, folder: &str, nth: u32) -> bool {
            self.selected.push((players, nth));
            (nth as usize) < self.matching(players, folder).len()
        }
    }

    #[test]
    fn pick_covers_every_map_and_avoids_the_last() {
        let mut rng = Rng::new(1);
        for count in 1..=6u32 {
            let mut seen = vec![false; count as usize];
            for _ in 0..2000 {
                let n = pick(count, None, &mut rng);
                assert!(n < count);
                seen[n as usize] = true;
            }
            assert!(seen.iter().all(|&s| s), "count {count}");
        }
        for last in 0..5 {
            for _ in 0..500 {
                let n = pick(5, Some(last), &mut rng);
                assert!(n < 5 && n != last);
            }
        }
        // A single map is picked again: there is nothing else.
        assert_eq!(pick(1, Some(0), &mut rng), 0);
        // A stale `last` beyond the count is ignored.
        assert!(pick(3, Some(9), &mut rng) < 3);
    }

    #[test]
    fn the_button_selects_among_the_filtered_maps() {
        let mut c = Controller::new(4, 7);
        let mut maps = FakeMaps::new(&[2, 4, 8, 4, 4]);
        let mut rows = FakeRows {
            press: true,
            ..Default::default()
        };
        assert!(!c.frame(&mut rows, &mut maps));
        assert_eq!(rows.buttons, ["Random map (3)"]);
        assert_eq!(rows.slider_texts, ["Random map: 4 players"]);
        assert_eq!(maps.selected.len(), 1);
        let (players, nth) = maps.selected[0];
        assert_eq!(players, 4);
        assert!(nth < 3);
    }

    #[test]
    fn no_press_selects_nothing() {
        let mut c = Controller::new(0, 7);
        let mut maps = FakeMaps::new(&[2, 4]);
        let mut rows = FakeRows::default();
        c.frame(&mut rows, &mut maps);
        assert_eq!(rows.buttons, ["Random map (2)"]);
        assert!(maps.selected.is_empty());
    }

    #[test]
    fn repeated_presses_change_the_map() {
        let mut c = Controller::new(0, 3);
        let mut maps = FakeMaps::new(&[2, 2, 2]);
        let mut previous = None;
        for _ in 0..50 {
            let mut rows = FakeRows {
                press: true,
                ..Default::default()
            };
            c.frame(&mut rows, &mut maps);
            let (_, nth) = *maps.selected.last().unwrap();
            assert_ne!(Some(nth), previous);
            previous = Some(nth);
        }
    }

    #[test]
    fn moving_the_slider_is_reported_and_forgets_the_last_pick() {
        let mut c = Controller::new(0, 3);
        let mut maps = FakeMaps::new(&[2, 2, 4]);
        let mut rows = FakeRows {
            press: true,
            ..Default::default()
        };
        c.frame(&mut rows, &mut maps);
        assert!(c.last.is_some());
        let mut rows = FakeRows {
            set_slider: Some(2),
            ..Default::default()
        };
        assert!(c.frame(&mut rows, &mut maps));
        assert_eq!(c.players(), 2);
        assert!(c.last.is_none());
        assert_eq!(rows.buttons, ["Random map (2)"]);
    }

    #[test]
    fn a_slider_value_out_of_range_is_clamped() {
        let mut c = Controller::new(99, 3);
        assert_eq!(c.players(), MAX_PLAYERS);
        let mut maps = FakeMaps::new(&[]);
        let mut rows = FakeRows {
            set_slider: Some(-4),
            ..Default::default()
        };
        c.frame(&mut rows, &mut maps);
        assert_eq!(c.players(), 0);
    }

    #[test]
    fn says_so_when_there_is_nothing_to_pick() {
        let mut c = Controller::new(6, 3);
        let mut maps = FakeMaps::new(&[2, 4]);
        let mut rows = FakeRows::default();
        c.frame(&mut rows, &mut maps);
        assert_eq!(rows.labels, ["No 6-player maps found"]);
        assert!(rows.buttons.is_empty());
    }

    #[test]
    fn says_so_while_the_index_is_building() {
        let mut c = Controller::new(0, 3);
        let mut maps = FakeMaps::new(&[2]);
        maps.busy = true;
        let mut rows = FakeRows::default();
        c.frame(&mut rows, &mut maps);
        assert_eq!(rows.labels, ["Indexing maps..."]);
        assert!(rows.buttons.is_empty());
    }

    #[test]
    fn the_folder_limit_reaches_the_host() {
        let mut c = Controller::new(0, 3);
        let mut maps = FakeMaps::new(&[2, 2, 4]);
        maps.dirs = vec!["mp", "single", "mp"];
        assert!(c.set_folder(" /mp/ "));
        assert_eq!(c.folder(), "mp");
        assert!(!c.set_folder("mp"));
        let mut rows = FakeRows::default();
        c.frame(&mut rows, &mut maps);
        assert_eq!(rows.buttons, ["Random map (2)"]);
        assert_eq!(maps.folders_seen, ["mp"]);
        c.set_folder("nowhere");
        let mut rows = FakeRows::default();
        c.frame(&mut rows, &mut maps);
        assert_eq!(rows.labels, ["No maps found in \"nowhere\""]);
        c.set_folder("");
        let mut rows = FakeRows::default();
        c.frame(&mut rows, &mut maps);
        assert_eq!(rows.buttons, ["Random map (3)"]);
    }

    #[test]
    fn folder_text_is_cleaned() {
        assert_eq!(clean_folder("  mp  "), "mp");
        assert_eq!(clean_folder("/Maps/mp/"), "Maps\\mp");
        assert_eq!(clean_folder("a\\b\\"), "a\\b");
        assert_eq!(clean_folder("   "), "");
        assert!(clean_folder(&"x".repeat(500)).len() < FOLDER_CAP);
    }

    #[test]
    fn saved_values_are_validated() {
        assert_eq!(valid_players(0), 0);
        assert_eq!(valid_players(8), 8);
        assert_eq!(valid_players(9), 0);
        assert_eq!(valid_players(-1), 0);
        assert_eq!(valid_players(i64::MAX), 0);
    }
}
