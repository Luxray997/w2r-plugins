//! What the overlay does with no game access: the hotkey toggle and where the picture goes.

/// Smallest and largest edge of the picture the user can pick (UI units).
pub const SIZE_MIN: i32 = 128;
pub const SIZE_MAX: i32 = 512;
pub const SIZE_DEFAULT: i32 = 256;
/// Space between the top bar and the picture, and between the picture and the screen's left edge.
pub const MARGIN: f32 = 8.0;
/// Height of the in-match top bar (UI units), which the picture sits under.
pub const TOP_BAR_H: f32 = 16.0;

/// Flips on the press of the hotkey, not while it is held, however many frames the press lasts.
#[derive(Debug, Default)]
pub struct Toggle {
    open: bool,
    was_down: bool,
}

impl Toggle {
    pub fn new(open: bool) -> Toggle {
        Toggle {
            open,
            was_down: false,
        }
    }

    /// Feed the key state once per frame; returns whether the overlay is open afterwards.
    pub fn update(&mut self, down: bool) -> bool {
        if down && !self.was_down {
            self.open = !self.open;
        }
        self.was_down = down;
        self.open
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }
}

pub fn clamp_size(size: i64) -> i32 {
    size.clamp(SIZE_MIN as i64, SIZE_MAX as i64) as i32
}

/// A rectangle in canvas units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The square the picture is fitted in: under the top bar, at its left edge. `None` when the panel
/// is too small to hold it (a tiny window), so nothing is drawn rather than something clipped.
pub fn place(panel: Rect, size: f32, screen_h: f32) -> Option<Rect> {
    let r = Rect {
        x: panel.x + MARGIN,
        y: panel.y + panel.h + MARGIN,
        w: size,
        h: size,
    };
    (size > 0.0 && r.y + size <= screen_h).then_some(r)
}

/// Fits a `w` by `h` picture inside a square of `size`, keeping its shape, centred.
pub fn fit(area: Rect, w: usize, h: usize) -> Rect {
    let (fw, fh) = (w.max(1) as f32, h.max(1) as f32);
    let (mut rw, mut rh) = (area.w, area.h);
    if fw >= fh {
        rh = area.h * fh / fw;
    } else {
        rw = area.w * fw / fh;
    }
    Rect {
        x: area.x + (area.w - rw) * 0.5,
        y: area.y + (area.h - rh) * 0.5,
        w: rw,
        h: rh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_key_flips_once() {
        let mut t = Toggle::new(false);
        assert!(!t.update(false));
        assert!(t.update(true));
        assert!(t.update(true));
        assert!(t.update(false));
        assert!(!t.update(true));
    }

    #[test]
    fn sizes_are_clamped() {
        assert_eq!(clamp_size(-5), SIZE_MIN);
        assert_eq!(clamp_size(9999), SIZE_MAX);
        assert_eq!(clamp_size(300), 300);
    }

    #[test]
    fn the_picture_sits_under_the_bar_and_stays_on_screen() {
        let bar = Rect { x: 0.0, y: 0.0, w: 1920.0, h: 30.0 };
        let r = place(bar, 256.0, 1080.0).unwrap();
        assert_eq!((r.x, r.y, r.w), (MARGIN, 30.0 + MARGIN, 256.0));
        assert!(place(bar, 256.0, 200.0).is_none());
        assert!(place(bar, 0.0, 1080.0).is_none());
    }

    #[test]
    fn a_wide_map_is_letterboxed_and_a_tall_one_pillarboxed() {
        let sq = Rect { x: 10.0, y: 20.0, w: 100.0, h: 100.0 };
        let wide = fit(sq, 128, 64);
        assert_eq!((wide.x, wide.y, wide.w, wide.h), (10.0, 45.0, 100.0, 50.0));
        let tall = fit(sq, 64, 128);
        assert_eq!((tall.x, tall.y, tall.w, tall.h), (35.0, 20.0, 50.0, 100.0));
    }
}
