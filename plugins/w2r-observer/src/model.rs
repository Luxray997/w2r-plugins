//! What an observer is given, with no game access. Tables are the game's own: the alliance
//! matrix is 8 rows of 16 bytes (`row * 16 + column`, row = whose view), and the vision masks hold,
//! for each player, a bit per player who gets that player's vision.

pub const MAX_PLAYERS: usize = 8;
pub const STANCE_ROW: usize = 16;
/// The lobby team value that marks an observer (teams 1 to 8 are players).
pub const OBSERVER_TEAM: u8 = 9;

/// Who observes: the bit mask of lobby slots whose team is [`OBSERVER_TEAM`] and that are in the
/// match as a human. A computer is never an observer.
pub fn observers(teams: &[u8; MAX_PLAYERS], is_human: impl Fn(usize) -> bool) -> u8 {
    (0..MAX_PLAYERS)
        .filter(|&i| teams[i] == OBSERVER_TEAM && is_human(i))
        .fold(0, |mask, i| mask | 1 << i)
}

/// Whether the map spawn of a unit for `owner` is skipped: during the map spawn an observer is
/// given nothing, so it owns no base and never counts as a survivor on any machine.
pub fn skips_spawn(spawning: bool, owner: usize, observers: u8) -> bool {
    spawning && owner < MAX_PLAYERS && observers >> owner & 1 == 1
}

/// Gives every observer an ally row (one-sided: others do not treat the observer as an ally) and
/// every player's vision. Returns whether anything changed.
pub fn apply(stance: &mut [u8; MAX_PLAYERS * STANCE_ROW], masks: &mut [u8; MAX_PLAYERS], observers: u8) -> bool {
    let mut changed = false;
    for obs in (0..MAX_PLAYERS).filter(|&i| observers >> i & 1 == 1) {
        for other in (0..MAX_PLAYERS).filter(|&j| j != obs) {
            let cell = &mut stance[obs * STANCE_ROW + other];
            if *cell != 1 {
                *cell = 1;
                changed = true;
            }
            // The other player shares vision with the observer, but the observer shares none.
            if masks[other] >> obs & 1 == 0 {
                masks[other] |= 1 << obs;
                changed = true;
            }
            if observers >> other & 1 == 0 && masks[obs] >> other & 1 == 1 {
                masks[obs] &= !(1 << other);
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_human_team_nine_slots_observe() {
        let teams = [1, 9, 2, 9, 0, 0, 0, 0];
        assert_eq!(observers(&teams, |i| i != 3), 0b10);
        assert_eq!(observers(&[1; 8], |_| true), 0);
    }

    #[test]
    fn observer_sees_everyone_and_nobody_sees_through_it() {
        let mut stance = [0u8; 128];
        let mut masks: [u8; 8] = core::array::from_fn(|i| 1 << i);
        masks[2] |= 1 << 5; // the observer was given vision by player 2's team before
        assert!(apply(&mut stance, &mut masks, 1 << 2));
        for j in (0..8).filter(|&j| j != 2) {
            assert_eq!(stance[2 * 16 + j], 1);
            assert_eq!(masks[j] >> 2 & 1, 1, "player {j} shares vision with the observer");
            assert_eq!(stance[j * 16 + 2], 0, "others do not ally the observer");
        }
        assert_eq!(masks[2], 1 << 2, "the observer shares no vision");
        assert!(!apply(&mut stance, &mut masks, 1 << 2), "second pass changes nothing");
    }

    #[test]
    fn only_observers_lose_their_spawn() {
        assert!(skips_spawn(true, 2, 1 << 2));
        assert!(!skips_spawn(false, 2, 1 << 2), "units trained later are not skipped");
        assert!(!skips_spawn(true, 1, 1 << 2));
        assert!(!skips_spawn(true, 15, 0xff), "neutral owner");
    }

    #[test]
    fn no_observers_changes_nothing() {
        let mut stance = [0u8; 128];
        let mut masks: [u8; 8] = core::array::from_fn(|i| 1 << i);
        assert!(!apply(&mut stance, &mut masks, 0));
        assert_eq!(stance, [0u8; 128]);
    }
}
