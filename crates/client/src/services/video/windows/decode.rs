use super::{
  AMD_VENDOR_ID, AmdAmfVideoDecoder, MftH264VideoDecoder, NVIDIA_VENDOR_ID, NativeVideoDecoder, NvdecVideoDecoder,
  codec_label, install_native_logger, windows_output_dxgi_adapter_vendor_id,
};
use crate::{
  network::protocol::VideoCodecId,
  services::video::{NativeVideoBackend, VideoDecodeConfig, VideoDecoder, VideoError, software::SoftwareVideoDecoder},
};

// Keep backend selection in provider modules. macOS should mirror this with
// macos/decode.rs and VideoToolbox/software providers, while concrete codec
// decoders can move into codec/backend modules behind VideoFrameDecoder.
struct WindowsDecoderContext {
  adapter_vendor_id: Option<u32>,
}

struct WindowsDecoderBuild {
  decoder: NativeVideoDecoder,
  backend: NativeVideoBackend,
  ready_path: WindowsDecoderReadyPath,
}

enum WindowsDecoderReadyPath {
  Named(&'static str),
  Software,
}

trait WindowsDecoderProvider {
  fn name(&self) -> &'static str;
  fn supports(&self, config: &VideoDecodeConfig, context: &WindowsDecoderContext) -> bool;
  fn create(&self, config: &VideoDecodeConfig) -> Result<WindowsDecoderBuild, VideoError>;
}

struct MftH264DecoderProvider;
struct NvdecDecoderProvider;
struct AmdAmfDecoderProvider;
struct SoftwareDecoderProvider;

pub(in crate::services::video) fn decode(config: VideoDecodeConfig) -> Result<VideoDecoder, VideoError> {
  install_native_logger();

  let context = WindowsDecoderContext {
    adapter_vendor_id: config
      .hardware_decoding
      .then(windows_output_dxgi_adapter_vendor_id)
      .flatten(),
  };
  let providers: [&dyn WindowsDecoderProvider; 4] = [
    &MftH264DecoderProvider,
    &NvdecDecoderProvider,
    &AmdAmfDecoderProvider,
    &SoftwareDecoderProvider,
  ];
  let Some(provider) = providers
    .iter()
    .copied()
    .find(|provider| provider.supports(&config, &context))
  else {
    return Err(no_windows_decoder_provider_error(&config, &context));
  };
  let build = provider.create(&config).map_err(|error| {
    if config.hardware_decoding {
      tracing::warn!(target: "video::decode::windows", "[video:decode/windows] {} decoder unavailable and software decoding is disabled by setting: {error}", provider.name());
    }
    error
  })?;
  log_windows_decoder_ready(&config, &build);

  Ok(VideoDecoder::from_decoder(
    Box::new(build.decoder),
    config,
    build.backend,
  ))
}

impl WindowsDecoderProvider for MftH264DecoderProvider {
  fn name(&self) -> &'static str {
    "Media Foundation"
  }

  fn supports(&self, config: &VideoDecodeConfig, _context: &WindowsDecoderContext) -> bool {
    !config.hardware_decoding && config.codec == VideoCodecId::H264
  }

  fn create(&self, config: &VideoDecodeConfig) -> Result<WindowsDecoderBuild, VideoError> {
    Ok(WindowsDecoderBuild {
      decoder: NativeVideoDecoder::MftH264(MftH264VideoDecoder::new(config)?),
      backend: NativeVideoBackend::WindowsMediaFoundation,
      ready_path: WindowsDecoderReadyPath::Named("Media Foundation"),
    })
  }
}

impl WindowsDecoderProvider for NvdecDecoderProvider {
  fn name(&self) -> &'static str {
    "NVDEC"
  }

  fn supports(&self, config: &VideoDecodeConfig, context: &WindowsDecoderContext) -> bool {
    config.hardware_decoding && context.adapter_vendor_id == Some(NVIDIA_VENDOR_ID)
  }

  fn create(&self, config: &VideoDecodeConfig) -> Result<WindowsDecoderBuild, VideoError> {
    Ok(WindowsDecoderBuild {
      decoder: NativeVideoDecoder::Nvdec(NvdecVideoDecoder::new(config)?),
      backend: NativeVideoBackend::NvidiaNvdec,
      ready_path: WindowsDecoderReadyPath::Named("NVDEC"),
    })
  }
}

impl WindowsDecoderProvider for AmdAmfDecoderProvider {
  fn name(&self) -> &'static str {
    "AMF"
  }

  fn supports(&self, config: &VideoDecodeConfig, context: &WindowsDecoderContext) -> bool {
    config.hardware_decoding && context.adapter_vendor_id == Some(AMD_VENDOR_ID)
  }

  fn create(&self, config: &VideoDecodeConfig) -> Result<WindowsDecoderBuild, VideoError> {
    Ok(WindowsDecoderBuild {
      decoder: NativeVideoDecoder::AmdAmf(AmdAmfVideoDecoder::new(config)?),
      backend: NativeVideoBackend::AmdAmf,
      ready_path: WindowsDecoderReadyPath::Named("AMF"),
    })
  }
}

impl WindowsDecoderProvider for SoftwareDecoderProvider {
  fn name(&self) -> &'static str {
    "software"
  }

  fn supports(&self, config: &VideoDecodeConfig, _context: &WindowsDecoderContext) -> bool {
    !config.hardware_decoding && config.codec != VideoCodecId::H264
  }

  fn create(&self, config: &VideoDecodeConfig) -> Result<WindowsDecoderBuild, VideoError> {
    let decoder = SoftwareVideoDecoder::new(config)?;
    let backend = decoder.backend();
    Ok(WindowsDecoderBuild {
      decoder: NativeVideoDecoder::Software(decoder),
      backend,
      ready_path: WindowsDecoderReadyPath::Software,
    })
  }
}

fn log_windows_decoder_ready(config: &VideoDecodeConfig, build: &WindowsDecoderBuild) {
  match build.ready_path {
    WindowsDecoderReadyPath::Named(path) => {
      tracing::info!(target: "video::decode::windows",
        "[video:decode/windows] decoder ready through {path}: codec={:?} size={}x{}",
        config.codec,
        config.width,
        config.height
      );
    }
    WindowsDecoderReadyPath::Software => {
      tracing::info!(target: "video::decode::windows",
        "[video:decode/windows] decoder ready through software: backend={:?} codec={:?} size={}x{}",
        build.backend,
        config.codec,
        config.width,
        config.height
      );
    }
  }
}

fn no_windows_decoder_provider_error(config: &VideoDecodeConfig, context: &WindowsDecoderContext) -> VideoError {
  let error = if config.hardware_decoding {
    match context.adapter_vendor_id {
      Some(vendor_id) => VideoError::new(format!(
        "Selected Windows GPU vendor_id=0x{vendor_id:04x} has no hardware decoder wired for {}.",
        codec_label(config.codec)
      )),
      None => VideoError::new(format!(
        "Failed to resolve selected Windows GPU; software decoding is disabled by setting for {}.",
        codec_label(config.codec)
      )),
    }
  } else {
    VideoError::new(format!(
      "No Windows software decoder provider is wired for {}.",
      codec_label(config.codec)
    ))
  };
  tracing::warn!(target: "video::decode::windows", "[video:decode/windows] {error}");
  error
}
