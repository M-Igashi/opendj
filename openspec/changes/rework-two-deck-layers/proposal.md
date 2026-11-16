# Proposal

Planned for: **v0.8 · From the booth to the studio** (2026-11-16, a user request after a review of how DJ apps hide less-used controls; issue #792)

## Why

2-Deck squeezes four control groups into each deck (tempo column, eight pads with page tabs, loop column, jog with PLAY and CUE), each at a small size, and at a MacBook's 1512×982 its tempo fader is only a few centimetres tall. 2-Deck Basic went the other way, with a mostly empty control row, and switching its mixer when PADS is on. Two 2-Deck layouts are also two things to keep in step.

Other DJ apps keep a deck clean by showing the controls used mid-mix and putting the rest in panels the DJ opens (djay Pro's panel under the decks, rekordbox's panel buttons, Traktor's and Mixxx's deck sizes). DJs on a controller mostly need the screen for state, while the library gets the height.

## What Changes

One 2-Deck layout built from three fixed layers, with no tabs between them:

1. **Row 1, always shown.** Each deck's header shows the time left large and the time elapsed small and grey under it, then the key. Under the header, from the outer edge in: LOOP ‹ n ›, CUE, PLAY and a mini jog: a dark disc with one line that turns with the playhead, nothing written in it.
2. **Tempo in the mixer.** The mixer is exactly as tall as row 1. Each deck's tempo column runs down the mixer's outer edge, next to that deck's jog: the live BPM and tempo % (which opens the tempo panel) on top, the tempo fader the full height under them, SYNC at the foot.
3. **Layer 2, the PADS button:** each deck's eight pads with their page tabs, full width under the deck, and an X-Y pad for the Beat FX under the mixer: pressing turns the Beat FX on, across sets its length in beats, up and down its D/W, and letting go turns it off again.
4. **Layer 3, the FX button:** the FX bar as today, its Beat FX as wide as the mixer above.
5. **JOG** swaps the mini jogs for big ones on the inner side of each header, as tall as the cover art.
6. PADS and FX are off for a new DJ, and the layers shown are saved.
7. **2-Deck Basic goes away:** a saved Basic opens 2-Deck. Deck colors stay as they are.

A deck holding a Groove Box, an instrument or the looper shows its full panel as today.

## Capabilities

### Modified Capabilities

- `app-layout`: 2-Deck in three layers, tempo on the mixer's edges, mini and big jogs, 2-Deck Basic removed

## Out of Scope

- 4-Deck, Player and Prep.
- A custom pad page ("My pads", one action per pad such as a quiet Play/Pause): its own change.
- Hot cue pads showing their time and a color bar: follows in the pads work.
- Sample and loop pack bars like djay's Looper and Sampler.

## Impact

`crates/app/ui` lite.slint (row 1, MixerTempo, XyFxPad, LiteJog in the header), main.slint (the 2-Deck block, top-bar buttons), deck.slint (elapsed under the time), fxbar.slint (Beat FX width); `crates/app/src` main.rs (PADS and FX off by default), layouts.rs (2-Deck Basic left out). The old 2-Deck deck (`Deck` with Pitch, pads, loop column and JogSection) stays for live decks and 4-Deck's fallback.

## Difficulty

**Medium: three or four PRs, 3 to 4 days.** Most parts exist (2-Deck Basic's decks, slim mixer, pad bar, JOG button); the new pieces are the tempo columns, the mini jog and the X-Y pad.

## Performance

Fewer widgets on screen than today's 2-Deck. The mini jog redraws one line per frame while playing, as the big jog does today.
