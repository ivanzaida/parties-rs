# Video Encoder/Decoder Implementation

This document describes the current video codec implementation and the intended direction for cleaner encoder/decoder abstractions.

The stream protocol supports these codec IDs:

| Codec | Protocol ID | `VideoCodecId` |
| --- | ---: | --- |
| Unknown | `0x00` | `Unknown` |
| AV1 | `0x01` | `Av1` |
| H.265 / HEVC | `0x02` | `H265` |
| H.264 / AVC | `0x03` | `H264` |

Only AV1, H.265, and H.264 are valid stream codecs. `src/services/video/mod.rs` validates this for both broadcast and decode configs.

## Main Rust Surface

The public-ish service layer lives in `src/services/video/mod.rs`:

- `VideoBroadcastConfig`: capture source, output size, codec, FPS, bitrate, and audio flag.
- `VideoDecodeConfig`: codec and frame dimensions for one remote stream.
- `VideoBroadcast`: owns the active encoder/broadcast runtime and stop handles.
- `VideoDecoder`: owns one native decoder instance.
- `NativeVideoBackend`: labels the selected implementation, for example `NvidiaNvenc`, `NvidiaNvdec`, `AmdAmf`, or `AppleVideoToolbox`.

Session code now consumes that surface through the split session modules:

- `src/session/video_stream.rs` starts and stops local broadcast with `VideoBroadcast::start_with_loopback`.
- `src/session/video.rs` receives remote video packets, owns the decode worker loop, and keeps a per-user `VideoDecodePool`.
- `src/session/video_sink.rs` owns decoded frame presentation, image cache state, and DX12 surface cache plumbing.

The desired high-level flow already mostly exists in pieces, but the real API should be allocation-aware. This shape is conceptual, not a request to clone packet or pixel buffers:

```rust
let packet = queue.pop_next();              // move packet ownership out of the queue
let output_buffer = sink.take_reusable_buf(); // optional CPU pixel buffer reuse
let decoded = decoder.decode(&packet, output_buffer)?;
sink.present(decoded);
```

The current implementation is optimized around batching and avoiding avoidable allocations rather than exposing that flow as a small API yet. Packets are drained by move, encoded bytes are passed to native decode by reference, and CPU decoded pixels should reuse an output buffer when the sink has one available. Windows CPU decode and macOS rav1d fallback already use this buffer; macOS VideoToolbox CPU fallback currently copies a `CVPixelBuffer` into a newly allocated NV12 `Vec`, although it can also return native image data. GPU decode paths should keep frames as native surfaces and avoid CPU pixel vectors entirely.

## Encoding

`VideoBroadcast::start_with_loopback(server, config, loopback)` validates config and dispatches to the platform encoder:

- Windows: `src/services/video/windows.rs::encode`
- macOS: `src/services/video/macos.rs::encode`

There is no software video encoder path today. AV1 encode is implemented, but only through native/hardware APIs.

### Windows Encode

Windows encoding is native hardware-first. `BroadcastEncoder` currently has four paths:

- `GpuNvenc`: GPU capture plus NVIDIA NVENC.
- `GpuAmf`: GPU capture plus AMD AMF.
- `Nvenc`: CPU capture/upload plus NVIDIA NVENC.
- `AmdAmf`: CPU capture/upload plus AMD AMF.

Screen and window capture prefer GPU capture when possible. Webcam/CPU paths capture frames on the Rust side and feed RGBA/BGRA data into the native encoder.

Codec selection is passed to the native C++ bridge as `config.codec as u8`. The native Windows code maps that value to AV1, H.265, or H.264:

- NVIDIA encode uses NVENC, including `NV_ENC_CODEC_AV1_GUID` for AV1.
- AMD encode uses AMF, including AV1-specific encoder properties from `VideoEncoderAV1.h`.

The Windows broadcast loop can exclude a failed encoder label and retry another backend. This gives runtime fallback across available hardware paths without changing the session layer.

### macOS Encode

macOS encoding uses Apple native APIs:

- Screen/window capture uses ScreenCaptureKit and VideoToolbox where possible.
- Webcam capture uses AVFoundation and VideoToolbox.
- CPU desktop capture can also feed VideoToolbox.

`VTEncoder` creates a `VTCompressionSession` with a codec type from `compression_codec_type(config.codec)`:

- `H264` -> `K_CM_VIDEO_CODEC_TYPE_H264`
- `H265` -> `K_CM_VIDEO_CODEC_TYPE_HEVC`
- `Av1` -> `K_CM_VIDEO_CODEC_TYPE_AV1`

This means AV1 encode is wired on macOS through VideoToolbox, subject to OS and hardware support. There is no rav1e/SVT-AV1 style software encoder.

## Decoding

`VideoDecoder::start(config)` validates codec and dimensions, then dispatches to the platform decoder:

- Windows: `src/services/video/windows.rs::decode`
- macOS: `src/services/video/macos.rs::decode`

`src/session/video.rs` keeps decoder instances in `VideoDecodePool`, keyed by user/config. That avoids recreating decoders for every packet and lets stream changes replace the decoder only when codec or dimensions change.

### Windows Decode

Windows decode selects a native hardware decoder based on the default DXGI adapter vendor:

- NVIDIA adapter -> NVDEC.
- AMD adapter -> AMF.

The native C++ decode layer maps AV1 to CUDA/AMF decoder support, for example `cudaVideoCodec_AV1` on NVDEC. H.265 and H.264 follow the same bridge pattern.

The session decode path tries the fastest presentation mode available:

- AMD shared NV12 planes into a DX12 surface cache.
- NVIDIA/Windows DX12 decode surfaces.
- CPU-visible decoded frame output when native surface output is unavailable.

The important performance rule is that decode and presentation should stay on the GPU path when possible, and only fall back to CPU pixels when required.

### macOS Decode

macOS decode uses VideoToolbox for H.264, H.265, and AV1. AV1 has an additional software fallback:

- Try VideoToolbox first.
- If VideoToolbox AV1 is unavailable, fall back to rav1d when software AV1 is explicitly enabled.

Software AV1 controls:

- `PARTIES_MACOS_SOFTWARE_AV1=1` or `true` enables rav1d fallback.
- `PARTIES_SIMULATE_UNSUPPORTED_AV1=1` or `true` forces the unsupported-VideoToolbox path for testing.

The rav1d fallback is intentionally guarded because realtime AV1 software decode can be expensive. The current code limits fallback to streams up to `1920 * 1080` pixels and uses two decoder threads.

## Current Capability Matrix

| Platform | AV1 encode | H.265 encode | H.264 encode | AV1 decode | H.265 decode | H.264 decode |
| --- | --- | --- | --- | --- | --- | --- |
| Windows | Native hardware | Native hardware | Native hardware | Native hardware | Native hardware | Native hardware |
| macOS | VideoToolbox | VideoToolbox | VideoToolbox | VideoToolbox or rav1d fallback | VideoToolbox | VideoToolbox |

Notes:

- AV1 is implemented on encode side.
- AV1 software decode exists on macOS only, behind an environment flag.
- AV1 software encode does not exist.
- Non-Windows/non-macOS native video backends are not implemented.

## Abstraction Gaps

The current service shape is usable, but backend policy is still spread across platform files:

- `VideoBroadcast` and `VideoDecoder` are wrappers, not backend traits.
- Windows encoder fallback policy lives inside `BroadcastEncoder::new_excluding`.
- Windows decoder selection is tied to the default DXGI adapter.
- macOS AV1 fallback policy lives inside the native decoder implementation.
- Session decode knows about native presentation variants: shared NV12 planes, DX12 surfaces, and CPU pixels.

This is acceptable for performance, but it makes codec capability, fallback order, and test coverage harder to reason about.

## Refactor Direction

Keep the hot path allocation-aware and monomorphized where it matters, but move policy into explicit selectors.

Suggested next abstractions:

1. `VideoEncoderBackend`

   A small trait or enum-owned backend interface for start, encode/send loop ownership, keyframe requests, and stop. This should probably remain enum-dispatched on the hot path, because the backend is selected once per stream and does not need dynamic dispatch per frame.

2. `VideoDecoderBackend`

   A decoder interface with separate decode output methods:

   - `decode_to_cpu`
   - `decode_to_dx12_surface`
   - `decode_to_shared_nv12_planes`

   The platform implementation can expose only the modes it supports. Session code should ask for the preferred output mode rather than knowing each backend's internals.

3. `CodecCapabilities`

   A capability model that answers:

   - Can this backend encode AV1/H.265/H.264 at this resolution/FPS?
   - Can this backend decode AV1/H.265/H.264 at this resolution?
   - Does it support GPU-native output?
   - Does it need a software fallback flag?

4. `EncoderSelector` and `DecoderSelector`

   Selection should be explicit and testable:

   - Input: codec, dimensions, source kind, adapter/vendor info, user settings.
   - Output: ordered backend candidates plus reasons skipped.

5. `DecodedFrameSink`

   Session presentation can be cleaner if decode workers emit one enum:

   ```rust
   enum DecodedFrameOutput {
     Cpu(DecodedVideoFrame),
     Dx12Surface { user_id: UserId, surface: Arc<Dx12Nv12Surface> },
     SharedNv12Planes { user_id: UserId, surface: Arc<Dx12Nv12Surface> },
   }
   ```

   That would keep GPU-specific fast paths intact while removing backend-specific branching from the packet loop.

## Performance Rules For Future Work

- Select encoder/decoder backends once per stream or stream config change, not per packet.
- Keep decode instances cached per watched user/config.
- Reuse output buffers for CPU frames.
- Prefer native GPU surfaces over CPU readback.
- Keep packet receive, decode, and presentation decoupled with bounded queues.
- Treat software AV1 as opt-in unless profiling proves it is safe for realtime playback.
- Keep backend selection logs detailed enough to explain why AV1/H.265/H.264 did or did not select a path.
