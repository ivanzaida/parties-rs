# Weak Spots

## Video Payload Format

The current screen-share video payload must remain compatible with the existing
server and other clients. Do not switch macOS H.264/H.265 output from Annex B to
VideoToolbox length-prefixed samples unless the protocol and all relevant
clients explicitly support that format.

Why this matters:

- VideoToolbox naturally produces H.264/H.265 samples as length-prefixed NAL
  units.
- The existing wire payload is treated as Annex B NAL units with start codes.
- Repacking length-prefixed samples into Annex B costs CPU and creates another
  encoded buffer.
- Removing that repack would improve macOS zero-copy behavior, but it would be a
  wire-format change and could break existing viewers.

For now, keep Annex B on the wire and optimize around it. Safe optimization work
includes reducing copies after the Annex B payload is built, avoiding CPU pixel
readback, and gating or removing CPU video fallback paths on macOS.
