# Video Architecture

The main detailed design note is [Video Encoder/Decoder Implementation](../video-codecs.md).

## Client Surface

The public service layer is in `crates/client/src/services/video/mod.rs` and exposes:

- `VideoBroadcastConfig`
- `VideoDecodeConfig`
- `VideoBroadcast`
- `VideoDecoder`
- `NativeVideoBackend`

## Windows

Windows video code uses native C++ bridge code under `src/native/windows_video` and Rust orchestration in `src/services/video/windows.rs`.

The build script compiles:

- Windows capture and bridge code.
- NVIDIA NVENC/NVDEC loader and wrappers.
- AMD AMF encoder/decoder wrappers.
- Media Foundation H.264 decode bridge.
- libhevc software decode bridge.

## macOS

macOS video code uses native Apple APIs:

- ScreenCaptureKit for screen/window capture where available.
- AVFoundation for webcam capture.
- VideoToolbox for encode/decode.
- Optional rav1d AV1 fallback through `shiguredo_dav1d`.

## Important Constraints

The current wire payload for H.264/H.265 is treated as Annex B. See [Weak Spots](../weak-spots.md) before changing video payload formats.

