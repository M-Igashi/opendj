# Tasks

## 1. Test app: a Session grid of MIDI clips

- [x] 1.1 `crates/studio` app: 4 tracks x 8 scenes of demo MIDI clips, clip, scene and stop buttons, transport, tempo
- [x] 1.2 Launches quantised to the bar (or beat, or immediately), sample-accurate in the audio callback, with tests
- [x] 1.3 Built-in sounds: a Groove Box kit for drums and a small polyphonic synth for bass, chords and lead
- [x] 1.4 Ableton Link: join the session, follow its tempo and phase, change the session's tempo
- [x] 1.5 Offline render and a timing and CPU check (`opendj-studio --check`)
- [ ] 1.6 Try it on macOS next to openDJ over Link

## 2. Instruments and sets

- [ ] 2.1 Plugin instruments on tracks through `opendj-plugin-host`
- [ ] 2.2 Save and open a set
- [ ] 2.3 Controllers: launch clips from a pad controller

## 3. Clip editing

- [ ] 3.1 New and duplicate clips, clip length
- [ ] 3.2 Piano roll
- [ ] 3.3 Record MIDI into a slot

## 4. Audio clips

- [ ] 4.1 Audio clips warped to the tempo from beatgrids
- [ ] 4.2 Drag from openDJ's library; stems to tracks

## 5. Shared transport

- [ ] 5.1 Link, MIDI clock and transport split out of `opendj-engine` into a crate both apps use

## 6. Arrangement

- [ ] 6.1 Arrangement timeline and recording the Session into it
- [ ] 6.2 Automation and export
