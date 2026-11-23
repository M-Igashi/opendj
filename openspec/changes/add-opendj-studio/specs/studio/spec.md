## ADDED Requirements

### Requirement: Session grid
openDJ Studio SHALL show tracks as columns and scenes as rows of clip slots. Pressing a clip SHALL launch it at the next launch point of the clock (by default the next bar); a playing clip on the same track SHALL stop at that point. Pressing a scene SHALL launch every clip in its row at the next launch point and stop tracks whose slot in that row is empty. A track's stop button SHALL stop it at the next launch point. A clip waiting to launch SHALL show as queued until it plays.

#### Scenario: Launching on the bar
- **WHEN** the transport is at beat 5.5 of a 4/4 bar grid and a clip is pressed
- **THEN** the clip starts exactly at beat 8, to the sample

#### Scenario: Launching a scene
- **WHEN** scene 3 is pressed while scene 1 plays and track 4 has no clip in scene 3
- **THEN** at the next bar tracks 1-3 switch to scene 3's clips and track 4 stops

### Requirement: Looping MIDI clips
A MIDI clip SHALL loop over its length in beats from the beat it launched, playing its notes on its track's instrument; notes that are sounding when a clip stops SHALL be released.

#### Scenario: A one-bar loop
- **WHEN** a one-bar clip with a note on beat 0 has played for three bars
- **THEN** the note has sounded three times, once at the start of each bar

### Requirement: Ableton Link
openDJ Studio SHALL be able to join an Ableton Link session, take its tempo and beat phase, and change the session's tempo when the tempo is changed in the studio, so its launch points line up with the bars of openDJ and other Link apps.

#### Scenario: Playing with openDJ
- **WHEN** openDJ and openDJ Studio are both in a Link session and the studio's clip is launched
- **THEN** it starts on a bar of the session, in time with openDJ's master deck
