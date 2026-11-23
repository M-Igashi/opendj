# openDJ Studio (test app)

A clip-launching DAW on openDJ's engine, as a separate app. This first test
app is a Session grid: four tracks by eight scenes of MIDI clips playing
built-in sounds (a Groove Box kit and a small synth), launched on the next
bar of the studio's own clock or of an Ableton Link session shared with
openDJ. Plan and later steps: `openspec/changes/add-opendj-studio`.

```sh
cargo run --release -p opendj-studio                    # the window
cargo run --release -p opendj-studio -- --check         # launch timing, CPU, allocations
cargo run --release -p opendj-studio -- --render a.wav  # 32 bars through the scenes
cargo run --release -p opendj-studio -- --link-check    # joins a Link session, checks a launch lands on its bar
```

Click a clip to launch it on the next bar (the **Launch** pill switches
between 1 Bar, 1/4 and None); **▶ N** launches a whole scene and stops
tracks with an empty slot in it; **■** stops a track on the next bar. Turn
on **LINK** with openDJ's Link on, and launches land on the bars of the mix.

## How it's built

- `clip.rs`: clips and the demo set (the model, no sample rate).
- `engine.rs`: the audio thread's side. The model is compiled into an
  immutable `Program` that arrives over a wait-free queue (the old one goes
  back to be dropped off the audio thread); each track's playing state
  (playing clip, pending launch, sounding notes) lives outside it and
  carries over. Launches are split into the block on their exact frame.
  The clock's beat is worked out from the frame count since the last tempo
  change, or read from Link each block, so it never drifts.
- `link.rs`: Ableton Link, quantum 4 as in openDJ.
- `sound.rs`: the built-in drum kit and synth; fixed voice arrays.
- `audio.rs`: the sound card through cpal, or a timer when there is none.
- `check.rs`: the offline checks, with a counting allocator.

The design follows a review of Suhina, transit, maolan and Ardour
(`notes/daw-architecture-review.md` in the project files).

openDJ never depends on this crate, so it can move to its own repository.

## Ableton Push 2

Plug in a Push 2 (with its power supply, for full brightness) and the
studio picks it up within two seconds; Ableton Live doesn't need to run.
If Live is open, set Push's Control Surface to None in Live's preferences,
or Live takes the lights back.

- Pads: the clip grid (Session) or the selected instrument (Note: drum pads,
  or a keyboard in A minor going up a fourth per row; Octave buttons shift
  it).
- Steps (press Note again, or Layout): the selected slot as a stage
  clip, one pad column per stage. The scene buttons pick what the pads
  set: Pitch (two neighbouring pads for the semitone between), Octave,
  Velocity, Gate (tap pulses; Select + pad sets the stage's length,
  Shift + pad ties it), Chance (the bottom pad skips the stage) and Lock
  amount. Hold stages and turn encoders to lock knobs on them; Delete
  while holding clears their locks. An empty slot becomes a stage clip on
  the first tap; Up/Down or Shift + scene button choose the slot.
- Scene buttons right of the pads launch scenes; Stop stops every clip.
- Buttons over the screen stop a track; buttons under it select one.
- Encoders: the selected instrument's eight knobs (Device), or volume, pan
  and sends across the tracks (Mix; press Mix again or use Page for the
  next control). The master encoder is the main volume, the tempo encoder
  the tempo; hold Shift for fine steps.
- Play starts and stops the transport; Quantize steps the launch quantise.

`opendj-studio --push-preview push.png` draws what the screen shows.
Code: `src/push/` (`surface.rs` for the controls and lights, `display.rs`
and `ui/push.slint` for the screen, drawn by Slint's software renderer),
`src/steps.rs` for stage clips; the protocol is Ableton's Push 2 MIDI and Display
Interface Manual.
