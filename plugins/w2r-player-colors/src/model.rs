//! What the bar shows and where, with no game access: which players get a box, where the team
//! dividers go, how wide the bar is and which side of the food count it sits on.

/// The game has eight player slots.
pub const MAX_PLAYERS: usize = 8;

/// Box edge, gap between boxes, gap around a team divider and the divider's width (UI units).
pub const BOX: f32 = 10.0;
pub const GAP: f32 = 3.0;
pub const TEAM_GAP: f32 = 9.0;
pub const DIVIDER: f32 = 2.0;
/// Space between the food count and the first box.
pub const MARGIN: f32 = 8.0;

/// A player in the match, as read when it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Player {
    pub index: u8,
    pub team: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Box(u8),
    Divider,
}

/// Boxes in team order (then player order), with a divider between teams. A divider only means
/// something when some team has more than one player; in a free-for-all every player is their own
/// team and the bar is a plain row. Players for whom `present` is false are left out, and a team
/// with nobody left takes its divider with it.
pub fn layout(players: &[Player], present: impl Fn(u8) -> bool) -> Vec<Item> {
    let team_game = players
        .iter()
        .any(|a| players.iter().any(|b| a.index != b.index && a.team == b.team));
    let mut live: Vec<Player> = players.iter().copied().filter(|p| present(p.index)).collect();
    live.sort_by_key(|p| (p.team, p.index));
    let mut items = Vec::with_capacity(live.len() * 2);
    let mut last_team = None;
    for p in live {
        if team_game && last_team.is_some_and(|t| t != p.team) {
            items.push(Item::Divider);
        }
        last_team = Some(p.team);
        items.push(Item::Box(p.index));
    }
    items
}

/// The distance from the bar's left edge to a box or divider, and the bar's total width.
pub fn offsets(items: &[Item]) -> (Vec<f32>, f32) {
    let mut at = 0.0f32;
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let before = match (i.checked_sub(1).map(|j| items[j]), item) {
            (None, _) => 0.0,
            (Some(Item::Divider), _) | (_, Item::Divider) => 0.0,
            _ => GAP,
        };
        at += before;
        out.push(at);
        at += match item {
            Item::Box(_) => BOX,
            Item::Divider => TEAM_GAP * 2.0 + DIVIDER,
        };
    }
    (out, at)
}

/// Where the bar starts: right of the food cell if it fits before the panel's right edge,
/// otherwise left of the resource strip (the strip is flush right when the player has moved the
/// resource bar there). `None` if it fits on neither side.
pub fn start_x(
    strip_left: f32,
    strip_right: f32,
    panel_left: f32,
    panel_right: f32,
    width: f32,
) -> Option<f32> {
    let right = strip_right + MARGIN;
    if right + width <= panel_right {
        return Some(right);
    }
    let left = strip_left - MARGIN - width;
    (left >= panel_left).then_some(left)
}

/// An opaque color in the game's byte order (R in the low byte) from the game's float colors.
pub fn pack(rgb: [f32; 3]) -> u32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    0xff00_0000 | byte(rgb[2]) << 16 | byte(rgb[1]) << 8 | byte(rgb[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pl(list: &[(u8, u8)]) -> Vec<Player> {
        list.iter().map(|&(index, team)| Player { index, team }).collect()
    }

    #[test]
    fn free_for_all_is_a_plain_row() {
        let items = layout(&pl(&[(0, 0), (1, 1), (2, 2)]), |_| true);
        assert_eq!(items, [Item::Box(0), Item::Box(1), Item::Box(2)]);
    }

    #[test]
    fn two_v_two_gets_one_divider() {
        let items = layout(&pl(&[(0, 0), (1, 1), (2, 0), (3, 1)]), |_| true);
        assert_eq!(
            items,
            [Item::Box(0), Item::Box(2), Item::Divider, Item::Box(1), Item::Box(3)]
        );
    }

    #[test]
    fn leaver_vanishes_and_an_empty_team_takes_its_divider() {
        let players = pl(&[(0, 0), (1, 1), (2, 0), (3, 1)]);
        assert_eq!(
            layout(&players, |p| p != 1),
            [Item::Box(0), Item::Box(2), Item::Divider, Item::Box(3)]
        );
        assert_eq!(layout(&players, |p| p % 2 == 0), [Item::Box(0), Item::Box(2)]);
        assert!(layout(&players, |_| false).is_empty());
    }

    #[test]
    fn mixed_teams_group_the_pair_and_separate_the_loners() {
        // 1v1v2: the pair is grouped and each loner is set apart.
        let items = layout(&pl(&[(0, 0), (1, 1), (2, 2), (3, 2)]), |_| true);
        assert_eq!(
            items,
            [Item::Box(0), Item::Divider, Item::Box(1), Item::Divider, Item::Box(2), Item::Box(3)]
        );
    }

    #[test]
    fn widths() {
        assert_eq!(offsets(&[]).1, 0.0);
        assert_eq!(offsets(&[Item::Box(0)]), (vec![0.0], BOX));
        let (at, width) = offsets(&[Item::Box(0), Item::Box(1)]);
        assert_eq!(at, [0.0, BOX + GAP]);
        assert_eq!(width, 2.0 * BOX + GAP);
        let (at, width) = offsets(&[Item::Box(0), Item::Divider, Item::Box(1)]);
        assert_eq!(at, [0.0, BOX, BOX + TEAM_GAP * 2.0 + DIVIDER]);
        assert_eq!(width, 2.0 * BOX + TEAM_GAP * 2.0 + DIVIDER);
    }

    #[test]
    fn placement_prefers_the_right_then_the_left() {
        assert_eq!(start_x(100.0, 400.0, 0.0, 1000.0, 50.0), Some(400.0 + MARGIN));
        // Strip flush right: no room beyond it, so it goes before the strip.
        assert_eq!(
            start_x(700.0, 1000.0, 0.0, 1000.0, 50.0),
            Some(700.0 - MARGIN - 50.0)
        );
        assert_eq!(start_x(20.0, 320.0, 0.0, 330.0, 50.0), None);
    }

    #[test]
    fn colors_pack_game_byte_order() {
        assert_eq!(pack([1.0, 0.0, 0.0]), 0xff00_00ff);
        assert_eq!(pack([0.0, 0.0, 1.0]), 0xffff_0000);
        assert_eq!(pack([2.0, -1.0, 0.5]), 0xff80_00ff);
    }
}
