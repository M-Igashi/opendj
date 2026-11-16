## ADDED Requirements

### Requirement: 2-Deck in three layers
2-Deck SHALL show row 1 at all times: each deck's header over a row holding a mini jog under the cover, the loop length with LOOP under it in the middle, and CUE and PLAY by the mixer, with the loop, CUE and PLAY the same height, either side of the mixer. The PADS button SHALL add a layer under row 1 with each deck's pads under that deck and the Beat FX under the mixer. The FX button SHALL add the FX bar under that, with an X-Y pad per deck in its middle. PADS and FX SHALL start off and be remembered (`pads_visible`, `fx_bar_visible`). There SHALL be no tabs between the layers.

#### Scenario: A new DJ
- **WHEN** a DJ opens 2-Deck for the first time
- **THEN** only row 1 shows under the waveforms, and the library has the rest of the window

#### Scenario: Pads and FX
- **WHEN** the DJ turns PADS and FX on
- **THEN** the pads layer shows under row 1 and the FX bar under the pads, and both stay on after a restart

### Requirement: Tempo in the mixer
In 2-Deck the mixer SHALL be as tall as row 1, and each deck's tempo SHALL run down the mixer's outer edge next to that deck: from the top SYNC, its BPM, its tempo % (opening the tempo panel), and the tempo fader the rest of the height. The mixer SHALL keep HIGH, MID and LOW down its outsides, a bigger filter knob either side of the crossfader with its Color FX ▾ outside it, and a headphone icon under each channel fader that turns lilac while cueing.

#### Scenario: Riding the tempo
- **WHEN** the DJ drags deck 1's tempo fader on the left edge of the mixer
- **THEN** deck 1's BPM above the fader follows

### Requirement: Jogs
Row 1 SHALL start at the outer edge with a mini jog under the cover: a dark disc with one line that turns with the playhead and no text. With JOG on, a big jog as tall as the cover SHALL sit on the inner side of each deck's header, and the mini jogs SHALL go.

#### Scenario: JOG on
- **WHEN** the DJ turns JOG on
- **THEN** each header has a big jog on its mixer side and row 1 has no mini jog

### Requirement: X-Y FX pads
Each deck's X-Y pad in the FX bar SHALL play one of that deck's three effects, picked by clicking its name: it SHALL turn the effect on while pressed and back off when let go (unless it was on before), set its length in beats from 1/16 to 4 across (its first setting for an effect without beats), and its D/W from the bottom up.

#### Scenario: A sweep
- **WHEN** the DJ presses near the top right of deck 1's pad showing Echo and lets go
- **THEN** deck 1's Echo plays at 4 beats with D/W near full while pressed, and stops when let go

### Requirement: Header clocks
A 2-Deck header SHALL show the time left large and the time elapsed small and grey under it, with no clocks in the jogs.

#### Scenario: Two clocks
- **WHEN** a 5:00 track has played 1:30
- **THEN** the header shows -3:30 large and +1:30 small

## REMOVED Requirements

### Requirement: Deck section order
**Reason**: The 2-Deck deck now has row 1's order (LOOP, CUE, PLAY, jog from the outer edge), with FX in the FX bar.
**Migration**: None; the layout changes for everyone.
