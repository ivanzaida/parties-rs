# Stream UI Rules

These rules define how the voice-channel UI behaves when live streams are available.

## Core Rule

The user can be in a voice channel without watching any stream.

Multiple streams may be live at the same time, but the client watches only one selected stream at a time. The UI must not show a multi-watch grid unless the protocol and client explicitly support watching multiple streams concurrently.

## Stream Count Thresholds

| Active streams | UI behavior |
| --- | --- |
| 0 | Hide stream picker UI. Show the normal voice-channel state. |
| 1 | Show one compact watch target. If the user is already watching it, show the single stream viewer. |
| 2-5 | Show an inline stream picker in the voice stage. Each stream is a watch target, not a preview. |
| 6+ | Show overflow handling: first 5 visible streams, search/filter affordance, and an `N more / browse` action. |
| Large counts, e.g. 50 | Use the full stream browser after explicit user action. Show paged/virtualized results, not 50 cards in the voice stage. |

## Browse Behavior

Clicking `browse` opens the full stream browser.

Opening the browser does not start watching a stream. The user remains in voice-only mode until they select a specific stream row and trigger `watch`.

## Watch Behavior

Clicking `watch` on a stream selects that stream and switches the stage to the single stream viewer.

If another stream is already being watched, selecting a different stream replaces the current viewed stream. It does not add a second simultaneous viewer.

## Left Rail Behavior

For small stream counts, the left rail may list individual live stream entries.

For larger stream counts, the left rail should collapse to a count summary, such as `50 live streams`, and leave discovery to the stage picker or full stream browser.

## Design References

- `QQEJr`: in voice, streams available, not watching.
- `Opxse`: compact overflow stream picker for large counts.
- `CtDtg`: overflow browse row.
- `y30dT3`: full stream browser opened after clicking browse.

