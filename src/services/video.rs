use std::{
  fmt,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  thread::JoinHandle,
};

use crate::{
  network::{protocol::VideoCodecId, server::Server},
  services::screen_share_sources::ScreenShareSourceKind,
};

#[derive(Clone, Debug)]
pub struct VideoBroadcastConfig {
  pub source_kind: ScreenShareSourceKind,
  pub source_id: u32,
  pub source_width: u16,
  pub source_height: u16,
  pub output_width: u16,
  pub output_height: u16,
  pub codec: VideoCodecId,
  pub fps: u32,
  pub bitrate_kbps: u32,
}

#[derive(Debug)]
pub struct VideoError {
  message: String,
}

impl VideoError {
  fn new(message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
    }
  }
}

impl fmt::Display for VideoError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.message)
  }
}

impl std::error::Error for VideoError {}

pub struct VideoBroadcast {
  stop: Arc<AtomicBool>,
  threads: Vec<JoinHandle<()>>,
  #[cfg(feature = "gstreamer-video")]
  pipeline: gstreamer::Pipeline,
}

impl VideoBroadcast {
  pub fn start(server: Arc<Server>, config: VideoBroadcastConfig) -> Result<Self, VideoError> {
    validate_config(&config)?;
    start_backend(server, config)
  }
}

impl Drop for VideoBroadcast {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Relaxed);
    #[cfg(feature = "gstreamer-video")]
    {
      use gstreamer::prelude::*;
      let _ = self.pipeline.set_state(gstreamer::State::Null);
    }
    for thread in self.threads.drain(..) {
      let _ = thread.join();
    }
  }
}

fn validate_config(config: &VideoBroadcastConfig) -> Result<(), VideoError> {
  if !config.codec.is_supported_stream_codec() {
    return Err(VideoError::new("Video codec must be AV1, H.265, or H.264."));
  }

  if config.source_width == 0
    || config.source_height == 0
    || config.output_width == 0
    || config.output_height == 0
  {
    return Err(VideoError::new("Selected stream source has no capture dimensions."));
  }

  if config.fps == 0 {
    return Err(VideoError::new("Video FPS must be greater than zero."));
  }

  if config.bitrate_kbps == 0 {
    return Err(VideoError::new("Video bitrate must be greater than zero."));
  }

  Ok(())
}

#[cfg(not(feature = "gstreamer-video"))]
fn start_backend(_server: Arc<Server>, config: VideoBroadcastConfig) -> Result<VideoBroadcast, VideoError> {
  let _ = (&config.source_kind, config.source_id);
  Err(VideoError::new(
    "GStreamer video backend is not enabled. Build with --features gstreamer-video.",
  ))
}

#[cfg(feature = "gstreamer-video")]
fn start_backend(server: Arc<Server>, config: VideoBroadcastConfig) -> Result<VideoBroadcast, VideoError> {
  gstreamer_backend::start(server, config)
}

#[cfg(feature = "gstreamer-video")]
mod gstreamer_backend {
  use std::{
    sync::{
      Arc,
      atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
  };

  use gstreamer as gst;
  use gstreamer::prelude::*;
  use gstreamer_app as gst_app;
  use xcap::{Monitor, Window};

  use super::{VideoBroadcast, VideoBroadcastConfig, VideoError};
  use crate::{
    network::{
      protocol::{VideoCodecId, VideoFrame},
      server::Server,
    },
    services::screen_share_sources::ScreenShareSourceKind,
  };

  const APPSRC_NAME: &str = "parties_video_source";
  const APPSINK_NAME: &str = "parties_encoded_sink";

  pub fn start(server: Arc<Server>, config: VideoBroadcastConfig) -> Result<VideoBroadcast, VideoError> {
    gst::init().map_err(|error| VideoError::new(format!("Failed to initialize GStreamer: {error}")))?;
    ensure_plugins(config.codec)?;

    let pipeline = build_pipeline(&config)?;
    let appsrc = pipeline
      .by_name(APPSRC_NAME)
      .ok_or_else(|| VideoError::new("GStreamer appsrc was not created."))?
      .downcast::<gst_app::AppSrc>()
      .map_err(|_| VideoError::new("GStreamer appsrc has the wrong type."))?;
    let appsink = pipeline
      .by_name(APPSINK_NAME)
      .ok_or_else(|| VideoError::new("GStreamer appsink was not created."))?
      .downcast::<gst_app::AppSink>()
      .map_err(|_| VideoError::new("GStreamer appsink has the wrong type."))?;

    pipeline
      .set_state(gst::State::Playing)
      .map_err(|error| VideoError::new(format!("Failed to start GStreamer pipeline: {error:?}")))?;

    let stop = Arc::new(AtomicBool::new(false));
    let tokio = tokio::runtime::Handle::try_current()
      .map_err(|_| VideoError::new("Video broadcast must start from a Tokio runtime."))?;
    let capture_thread = spawn_capture_thread(appsrc, config.clone(), stop.clone())?;
    let send_thread = spawn_send_thread(appsink, server, config, tokio, stop.clone())?;

    Ok(VideoBroadcast {
      stop,
      threads: vec![capture_thread, send_thread],
      pipeline,
    })
  }

  fn build_pipeline(config: &VideoBroadcastConfig) -> Result<gst::Pipeline, VideoError> {
    let encoder = encoder_pipeline(config)?;
    let description = format!(
      "appsrc name={APPSRC_NAME} is-live=true format=time do-timestamp=true caps=video/x-raw,format=RGBA,width={},height={},framerate={}/1 \
       ! queue max-size-buffers=2 leaky=downstream \
       ! videoconvert \
       ! videoscale \
       ! video/x-raw,format=I420,width={},height={},framerate={}/1 \
       ! {encoder} \
       ! appsink name={APPSINK_NAME} emit-signals=false sync=false max-buffers=4 drop=true",
      config.source_width,
      config.source_height,
      config.fps,
      config.output_width,
      config.output_height,
      config.fps,
    );

    gst::parse::launch(&description)
      .map_err(|error| VideoError::new(format!("Failed to build GStreamer pipeline: {error}")))?
      .downcast::<gst::Pipeline>()
      .map_err(|_| VideoError::new("GStreamer did not create a pipeline."))
  }

  fn encoder_pipeline(config: &VideoBroadcastConfig) -> Result<String, VideoError> {
    let key_interval = config.fps.saturating_mul(2).max(1);
    Ok(match config.codec {
      VideoCodecId::H264 => format!(
        "x264enc tune=zerolatency speed-preset=veryfast bitrate={} key-int-max={} byte-stream=true \
         ! h264parse config-interval=1 \
         ! video/x-h264,stream-format=byte-stream,alignment=au",
        config.bitrate_kbps, key_interval
      ),
      VideoCodecId::H265 => format!(
        "x265enc tune=zerolatency speed-preset=ultrafast bitrate={} key-int-max={} \
         ! h265parse config-interval=1 \
         ! video/x-h265,stream-format=byte-stream,alignment=au",
        config.bitrate_kbps, key_interval
      ),
      VideoCodecId::Av1 => {
        "svtav1enc ! av1parse ! video/x-av1,stream-format=obu-stream,alignment=tu".to_owned()
      }
      VideoCodecId::Unknown => return Err(VideoError::new("Unsupported video codec.")),
    })
  }

  fn ensure_plugins(codec: VideoCodecId) -> Result<(), VideoError> {
    let names: &[&str] = match codec {
      VideoCodecId::H264 => &["appsrc", "videoconvert", "videoscale", "x264enc", "h264parse", "appsink"],
      VideoCodecId::H265 => &["appsrc", "videoconvert", "videoscale", "x265enc", "h265parse", "appsink"],
      VideoCodecId::Av1 => &["appsrc", "videoconvert", "videoscale", "svtav1enc", "av1parse", "appsink"],
      VideoCodecId::Unknown => return Err(VideoError::new("Unsupported video codec.")),
    };

    let missing = names
      .iter()
      .copied()
      .filter(|name| gst::ElementFactory::find(name).is_none())
      .collect::<Vec<_>>();

    if missing.is_empty() {
      Ok(())
    } else {
      Err(VideoError::new(format!(
        "Missing GStreamer plugin(s): {}",
        missing.join(", ")
      )))
    }
  }

  fn spawn_capture_thread(
    appsrc: gst_app::AppSrc,
    config: VideoBroadcastConfig,
    stop: Arc<AtomicBool>,
  ) -> Result<JoinHandle<()>, VideoError> {
    thread::Builder::new()
      .name("parties-video-capture".to_owned())
      .spawn(move || {
        let frame_duration = Duration::from_secs_f64(1.0 / config.fps as f64);
        let frame_duration_ns = frame_duration.as_nanos().min(u128::from(u64::MAX)) as u64;
        let start = Instant::now();
        let mut frame_number = 0u64;

        while !stop.load(Ordering::Relaxed) {
          let frame_start = Instant::now();
          let Some(mut buffer) = capture_buffer(&config, frame_duration_ns, frame_number) else {
            thread::sleep(Duration::from_millis(50));
            continue;
          };
          if let Some(buffer_ref) = buffer.get_mut() {
            buffer_ref.set_pts(gst::ClockTime::from_nseconds(
              start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
            ));
            buffer_ref.set_duration(gst::ClockTime::from_nseconds(frame_duration_ns));
          }

          if appsrc.push_buffer(buffer).is_err() {
            break;
          }

          frame_number = frame_number.wrapping_add(1);
          if let Some(remaining) = frame_duration.checked_sub(frame_start.elapsed()) {
            thread::sleep(remaining);
          }
        }

        let _ = appsrc.end_of_stream();
      })
      .map_err(|error| VideoError::new(format!("Failed to start video capture thread: {error}")))
  }

  fn capture_buffer(
    config: &VideoBroadcastConfig,
    frame_duration_ns: u64,
    frame_number: u64,
  ) -> Option<gst::Buffer> {
    let image = match config.source_kind {
      ScreenShareSourceKind::Screen => Monitor::all()
        .ok()?
        .into_iter()
        .find(|monitor| monitor.id().ok() == Some(config.source_id))?
        .capture_image()
        .ok()?,
      ScreenShareSourceKind::Window => Window::all()
        .ok()?
        .into_iter()
        .find(|window| window.id().ok() == Some(config.source_id))?
        .capture_image()
        .ok()?,
    };
    let (width, height) = image.dimensions();
    if width != u32::from(config.source_width) || height != u32::from(config.source_height) {
      return None;
    }

    let mut buffer = gst::Buffer::from_mut_slice(image.into_raw());
    if let Some(buffer_ref) = buffer.get_mut() {
      buffer_ref.set_dts(gst::ClockTime::from_nseconds(frame_number.saturating_mul(frame_duration_ns)));
    }
    Some(buffer)
  }

  fn spawn_send_thread(
    appsink: gst_app::AppSink,
    server: Arc<Server>,
    config: VideoBroadcastConfig,
    tokio: tokio::runtime::Handle,
    stop: Arc<AtomicBool>,
  ) -> Result<JoinHandle<()>, VideoError> {
    thread::Builder::new()
      .name("parties-video-send".to_owned())
      .spawn(move || {
        let started = Instant::now();
        let mut frame_number = 0u32;

        while !stop.load(Ordering::Relaxed) {
          let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(100)) else {
            continue;
          };
          let Some(buffer) = sample.buffer() else {
            continue;
          };
          let Ok(map) = buffer.map_readable() else {
            continue;
          };
          let encoded = map.as_slice().to_vec();
          if encoded.is_empty() {
            continue;
          }

          let keyframe = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);
          let frame = VideoFrame {
            frame_number,
            timestamp: started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
            keyframe,
            width: config.output_width,
            height: config.output_height,
            codec: config.codec,
            encoded,
          };

          if tokio.block_on(server.send_video_frame(frame)).is_err() {
            break;
          }
          frame_number = frame_number.wrapping_add(1);
        }
      })
      .map_err(|error| VideoError::new(format!("Failed to start video sender thread: {error}")))
  }
}
