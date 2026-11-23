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
