//! Pure recolor rules (wc2r-plus `recolour.cpp`).

/// The black team's colour index.
pub const BLACK: usize = 5;
/// Length of the black ramp in the render palette (first entry is the base shade).
pub const RAMP: usize = 4;

pub const KEY: &str = "black";
pub const MAX_CHOICE: i32 = 2;

/// Choice 1 pink, 2 cyan; anything else (0) leaves black alone.
pub fn target(choice: i64) -> Option<[u8; 3]> {
    match choice {
        1 => Some([0xe8, 0x7b, 0xdd]),
        2 => Some([0x00, 0xc6, 0xe0]),
        _ => None,
    }
}

fn luma(c: [u8; 3]) -> i32 {
    (c[0] as i32 * 299 + c[1] as i32 * 587 + c[2] as i32 * 114) / 1000
}

/// The ramp for `rgb`: the first entry is `rgb`, the rest keep the stock ramp's brightness ratio
/// to its first entry.
pub fn ramp(stock: &[[u8; 3]; RAMP], rgb: [u8; 3]) -> [[u8; 3]; RAMP] {
    let base = luma(stock[0]);
    let mut out = [[0u8; 3]; RAMP];
    for k in 0..RAMP {
        for c in 0..3 {
            let mut v = rgb[c] as i32;
            if k > 0 && base > 0 {
                v = v * luma(stock[k]) / base;
            }
            out[k][c] = v.min(255) as u8;
        }
    }
    out
}

/// The HD table entry (r, g, b as 0..1) for `rgb`.
pub fn float_rgb(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|c| c as f32 / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices() {
        assert_eq!(target(0), None);
        assert_eq!(target(1), Some([0xe8, 0x7b, 0xdd]));
        assert_eq!(target(2), Some([0x00, 0xc6, 0xe0]));
        assert_eq!(target(9), None);
    }

    #[test]
    fn ramp_keeps_the_stock_shading() {
        let stock = [[40, 40, 40], [20, 20, 20], [60, 60, 60], [80, 80, 80]];
        let r = ramp(&stock, [200, 100, 50]);
        assert_eq!(r[0], [200, 100, 50]);
        assert_eq!(r[1], [100, 50, 25]);
        assert_eq!(r[2], [255, 150, 75]);
        assert_eq!(r[3], [255, 200, 100]);
    }

    #[test]
    fn black_base_leaves_the_ramp_flat() {
        let r = ramp(&[[0; 3]; RAMP], [10, 20, 30]);
        assert!(r.iter().all(|e| *e == [10, 20, 30]));
    }
}
