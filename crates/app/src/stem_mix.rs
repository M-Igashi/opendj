//! What the Stems look draws for each deck's stems: their levels on the
//! mixer (0 when muted), and which stem is in front, the one last touched.
//! Muting or turning a stem down shrinks its shape on the waveform and
//! leaves a faint outline at its full size, so it shows where it comes back.

use opendj_waveform::{StemMix, MAX_STEMS};

/// One deck's stems as last seen, and the order they were touched in.
#[derive(Clone, Debug, Default)]
struct Deck {
    track: Option<usize>,
    seen: Option<[(f32, bool); MAX_STEMS]>,
    rank: [u32; MAX_STEMS],
    tick: u32,
}

#[derive(Debug)]
pub struct StemMixes<const N: usize> {
    decks: [Deck; N],
}

impl<const N: usize> Default for StemMixes<N> {
    fn default() -> Self {
        Self {
            decks: std::array::from_fn(|_| Deck::default()),
        }
    }
}

impl<const N: usize> StemMixes<N> {
    /// Deck `d`'s stems this frame, with the track it has loaded and each
    /// stem's level and mute; whether that changed since the last frame.
    /// A new track starts with no stem in front, so the reset that comes
    /// with loading it doesn't count as a touch.
    pub fn update(
        &mut self,
        d: usize,
        track: Option<usize>,
        volume: [f32; MAX_STEMS],
        mute: [bool; MAX_STEMS],
    ) -> (StemMix, bool) {
        let deck = &mut self.decks[d];
        let now: [(f32, bool); MAX_STEMS] = std::array::from_fn(|s| (volume[s], mute[s]));
        let mut changed = false;
        if deck.track != track {
            *deck = Deck {
                track,
                ..Deck::default()
            };
            changed = true;
        }
        match deck.seen {
            Some(seen) => {
                for s in 0..MAX_STEMS {
                    if seen[s] != now[s] {
                        deck.tick += 1;
                        deck.rank[s] = deck.tick;
                        changed = true;
                    }
                }
            }
            None => changed = true,
        }
        deck.seen = Some(now);
        let mix = StemMix {
            gain: std::array::from_fn(|s| if mute[s] { 0.0 } else { volume[s].max(0.0) }),
            rank: deck.rank,
        };
        (mix, changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: [f32; MAX_STEMS] = [1.0; MAX_STEMS];
    const OPEN: [bool; MAX_STEMS] = [false; MAX_STEMS];

    #[test]
    fn a_muted_stem_has_no_level() {
        let mut m = StemMixes::<1>::default();
        let mut mute = OPEN;
        mute[3] = true;
        let (mix, _) = m.update(0, Some(1), FULL, mute);
        assert_eq!(mix.gain[3], 0.0);
        assert_eq!(mix.gain[0], 1.0);
    }

    #[test]
    fn the_stem_last_touched_is_in_front() {
        let mut m = StemMixes::<1>::default();
        m.update(0, Some(1), FULL, OPEN);
        let mut volume = FULL;
        volume[1] = 0.5;
        let (mix, changed) = m.update(0, Some(1), volume, OPEN);
        assert!(changed);
        assert!(mix.rank[1] > mix.rank[0]);
        let mut mute = OPEN;
        mute[3] = true;
        let (mix, _) = m.update(0, Some(1), volume, mute);
        assert!(mix.rank[3] > mix.rank[1], "vocal touched last");
        let (_, changed) = m.update(0, Some(1), volume, mute);
        assert!(!changed, "nothing moved");
    }

    #[test]
    fn loading_a_track_is_not_a_touch() {
        let mut m = StemMixes::<1>::default();
        let mut mute = OPEN;
        mute[3] = true;
        m.update(0, Some(1), FULL, mute);
        m.update(0, Some(1), FULL, OPEN);
        // The load unmutes every stem in the same frame the track changes.
        let (mix, _) = m.update(0, Some(2), FULL, OPEN);
        assert_eq!(mix.rank, [0; MAX_STEMS]);
    }
}
