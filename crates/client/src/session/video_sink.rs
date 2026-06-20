#[cfg(target_os = "windows")]
use std::collections::VecDeque;
use std::{
  collections::HashMap,
  sync::Arc,
  time::{Duration, Instant},
};

use lurq::images::{ImageData, StreamingImage};
use parking_lot::Mutex;

use super::{LobbyState, LobbyUpdatePublisher, VideoStreamError, video};
use crate::{
  network::protocol::{UserId, control::ScreenShareMetadata},
  services::video::{DecodedVideoFrame, DecodedVideoPixelFormat},
};

#[allow(dead_code)]
enum VideoFrameImage {
  Cpu(StreamingImage),
  #[cfg(target_os = "macos")]
  MacosNative(ImageData),
  #[cfg(target_os = "windows")]
  Dx12Surface {
    image: ImageData,
    native: lurq::images::NativeImageData,
    slot: lurq::images::Dx12Nv12ImageSlot,
  },
}

impl VideoFrameImage {
  fn is_cpu_image(&self) -> bool {
    matches!(self, Self::Cpu(_))
  }

  fn image_data(&self) -> ImageData {
    match self {
      Self::Cpu(image) => image.image_data(),
      #[cfg(target_os = "macos")]
      Self::MacosNative(image) => image.clone(),
      #[cfg(target_os = "windows")]
      Self::Dx12Surface { image, .. } => image.clone(),
    }
  }

  fn set_cpu_pixels(&self, format: DecodedVideoPixelFormat, pixels: Vec<u8>) -> bool {
    match self {
      Self::Cpu(image) => {
        match format {
          DecodedVideoPixelFormat::Rgba8 => image.set_rgba(pixels),
          DecodedVideoPixelFormat::Nv12 => image.set_nv12(pixels),
        }
        true
      }
      #[cfg(target_os = "macos")]
      Self::MacosNative(_) => false,
      #[cfg(target_os = "windows")]
      Self::Dx12Surface { .. } => false,
    }
  }

  fn take_cpu_buffer(&self) -> Option<Vec<u8>> {
    match self {
      Self::Cpu(image) => match image.image_data().format() {
        lurq::images::ImagePixelFormat::Rgba8 => image.take_rgba_buffer(),
        lurq::images::ImagePixelFormat::Nv12 => image.take_nv12_buffer(),
      },
      #[cfg(target_os = "macos")]
      Self::MacosNative(_) => None,
      #[cfg(target_os = "windows")]
      Self::Dx12Surface { .. } => None,
    }
  }
}

pub(super) struct VideoFrameSink {
  frames: Arc<Mutex<HashMap<UserId, VideoFrameImage>>>,
  errors: Arc<Mutex<HashMap<UserId, VideoStreamError>>>,
  metadata: Arc<Mutex<HashMap<UserId, ScreenShareMetadata>>>,
  render_fps: Arc<Mutex<HashMap<UserId, VideoRenderFpsCounter>>>,
  #[cfg(target_os = "windows")]
  dx12_surface_reuse: Arc<Mutex<Dx12SurfaceReuseTracker>>,
  lobby: Arc<Mutex<LobbyState>>,
  lobby_updates: LobbyUpdatePublisher,
  #[cfg(target_os = "windows")]
  dx12_video_surfaces: Option<lurq::app::dx12_render::Dx12VideoSurfaceAllocator>,
}

struct VideoRenderFpsCounter {
  window_started_at: Instant,
  frames: u32,
  fps: u32,
  last_frame_number: Option<u32>,
  old_frames: u32,
}

#[cfg(target_os = "windows")]
struct Dx12SurfaceReuseTracker {
  frames: HashMap<isize, Dx12SurfaceFrameState>,
  stats: Dx12SurfaceReuseStats,
}

#[cfg(target_os = "windows")]
struct Dx12SurfaceFrameState {
  user_id: UserId,
  frame_number: u32,
  updated_at: Instant,
}

#[cfg(target_os = "windows")]
struct Dx12SurfaceReuseStats {
  started_at: Instant,
  reuses: u32,
  short_reuses: u32,
  backwards_reuses: u32,
  min_frame_delta: Option<u32>,
  max_frame_delta: u32,
  min_age: Option<Duration>,
  max_age: Duration,
  last_user_id: Option<UserId>,
}

impl VideoFrameSink {
  pub(super) fn new(lobby: Arc<Mutex<LobbyState>>, lobby_updates: LobbyUpdatePublisher) -> Self {
    Self {
      frames: Arc::new(Mutex::new(HashMap::new())),
      errors: Arc::new(Mutex::new(HashMap::new())),
      metadata: Arc::new(Mutex::new(HashMap::new())),
      render_fps: Arc::new(Mutex::new(HashMap::new())),
      #[cfg(target_os = "windows")]
      dx12_surface_reuse: Arc::new(Mutex::new(Dx12SurfaceReuseTracker::new())),
      lobby,
      lobby_updates,
      #[cfg(target_os = "windows")]
      dx12_video_surfaces: None,
    }
  }

  #[cfg(target_os = "windows")]
  pub(super) fn with_dx12_video_surface_allocator(
    lobby: Arc<Mutex<LobbyState>>,
    lobby_updates: LobbyUpdatePublisher,
    dx12_video_surfaces: lurq::app::dx12_render::Dx12VideoSurfaceAllocator,
  ) -> Self {
    Self {
      frames: Arc::new(Mutex::new(HashMap::new())),
      errors: Arc::new(Mutex::new(HashMap::new())),
      metadata: Arc::new(Mutex::new(HashMap::new())),
      render_fps: Arc::new(Mutex::new(HashMap::new())),
      dx12_surface_reuse: Arc::new(Mutex::new(Dx12SurfaceReuseTracker::new())),
      lobby,
      lobby_updates,
      dx12_video_surfaces: Some(dx12_video_surfaces),
    }
  }

  pub(super) fn clear_all(&self) {
    self.frames.lock().clear();
    self.errors.lock().clear();
    self.metadata.lock().clear();
    self.render_fps.lock().clear();
    #[cfg(target_os = "windows")]
    self.dx12_surface_reuse.lock().clear();
  }

  pub(super) fn image_data(&self, user_id: UserId) -> Option<ImageData> {
    self.frames.lock().get(&user_id).map(VideoFrameImage::image_data)
  }

  pub(super) fn error(&self, user_id: UserId) -> Option<VideoStreamError> {
    self.errors.lock().get(&user_id).cloned()
  }

  pub(super) fn render_stats(&self, user_id: UserId) -> (u32, Option<u32>, u32) {
    let now = Instant::now();
    let mut counters = self.render_fps.lock();
    let counter = counters
      .entry(user_id)
      .or_insert_with(|| VideoRenderFpsCounter::new(now));
    counter.sample(now)
  }

  pub(super) fn retain_user(&self, watched_user_id: Option<UserId>) {
    let mut frames = self.frames.lock();
    let mut errors = self.errors.lock();
    let mut metadata = self.metadata.lock();
    let mut render_fps = self.render_fps.lock();
    match watched_user_id {
      Some(user_id) => {
        frames.retain(|cached_user_id, _| *cached_user_id == user_id);
        errors.retain(|cached_user_id, _| *cached_user_id == user_id);
        metadata.retain(|cached_user_id, _| *cached_user_id == user_id);
        render_fps.retain(|cached_user_id, _| *cached_user_id == user_id);
        #[cfg(target_os = "windows")]
        self.dx12_surface_reuse.lock().retain_user(user_id);
      }
      None => {
        frames.clear();
        errors.clear();
        metadata.clear();
        render_fps.clear();
        #[cfg(target_os = "windows")]
        self.dx12_surface_reuse.lock().clear();
      }
    }
  }

  pub(super) fn clear_user(&self, user_id: UserId) {
    self.frames.lock().remove(&user_id);
    self.errors.lock().remove(&user_id);
    self.metadata.lock().remove(&user_id);
    #[cfg(target_os = "windows")]
    self.dx12_surface_reuse.lock().clear_user(user_id);
  }

  pub(super) fn set_error(&self, user_id: UserId, error: VideoStreamError) {
    self.frames.lock().remove(&user_id);
    self.metadata.lock().remove(&user_id);
    let changed = {
      let mut errors = self.errors.lock();
      let changed = errors.get(&user_id) != Some(&error);
      errors.insert(user_id, error);
      changed
    };
    if changed {
      self.publish_lobby_update();
    }
  }

  pub(super) fn clear_error(&self, user_id: UserId) {
    let cleared = self.errors.lock().remove(&user_id).is_some();
    if cleared {
      self.publish_lobby_update();
    }
  }

  pub(super) fn take_pixel_buffer(&self, user_id: UserId, width: u16, height: u16) -> Option<Vec<u8>> {
    self
      .frames
      .lock()
      .get(&user_id)
      .filter(|image| {
        image.image_data().width() == u32::from(width) && image.image_data().height() == u32::from(height)
      })
      .and_then(VideoFrameImage::take_cpu_buffer)
  }

  pub(super) fn has_frame(&self, user_id: UserId, width: u16, height: u16) -> bool {
    self.frames.lock().get(&user_id).is_some_and(|image| {
      image.image_data().width() == u32::from(width) && image.image_data().height() == u32::from(height)
    })
  }

  pub(super) fn present_decoded(
    &self,
    frame: DecodedVideoFrame,
    watched_user_id: Option<UserId>,
    prefer_cpu_frame: bool,
  ) {
    #[cfg(not(target_os = "macos"))]
    let _ = prefer_cpu_frame;
    if watched_user_id != Some(frame.sender_id) {
      return;
    }
    self.clear_error(frame.sender_id);

    let mut should_publish_update = false;
    #[cfg(target_os = "macos")]
    if !prefer_cpu_frame && let Some(native_image) = frame.native_image.clone() {
      {
        let mut frames = self.frames.lock();
        frames.insert(frame.sender_id, VideoFrameImage::MacosNative(native_image));
      }

      self.update_share_metadata(
        frame.sender_id,
        ScreenShareMetadata {
          codec: frame.codec,
          width: frame.width,
          height: frame.height,
        },
      );
      self.publish_lobby_update();
      self.record_rendered_frame(frame.sender_id, frame.frame_number);
      return;
    }

    {
      let _span = crate::services::profiler::span("video.render.cpu_image_update");
      let mut frames = self.frames.lock();
      match frames.get(&frame.sender_id) {
        Some(image)
          if image.is_cpu_image()
            && image.image_data().width() == u32::from(frame.width)
            && image.image_data().height() == u32::from(frame.height)
            && image.image_data().format() == decoded_pixel_format_to_lurq(frame.format) =>
        {
          let _span = crate::services::profiler::span("video.render.cpu_pixels_replace");
          image.set_cpu_pixels(frame.format, frame.pixels);
        }
        _ => {
          tracing::debug!(target: "video::decode",
            "[video:decode] creating streamed image for user {}: {}x{} format={:?}",
            frame.sender_id,
            frame.width,
            frame.height,
            frame.format
          );
          should_publish_update = true;
          let image = {
            let _span = crate::services::profiler::span("video.render.cpu_image_create");
            match frame.format {
              DecodedVideoPixelFormat::Rgba8 => {
                StreamingImage::new_rgba(frame.pixels, u32::from(frame.width), u32::from(frame.height))
              }
              DecodedVideoPixelFormat::Nv12 => {
                StreamingImage::new_nv12(frame.pixels, u32::from(frame.width), u32::from(frame.height))
              }
            }
          };
          tracing::debug!(
            target: "video::watch",
            "watched stream cpu image replace user={} image_id={} size={}x{} format={:?}",
            frame.sender_id,
            image.image_data().id(),
            frame.width,
            frame.height,
            frame.format
          );
          frames.insert(frame.sender_id, VideoFrameImage::Cpu(image));
        }
      }
    }

    should_publish_update |= self.update_share_metadata(
      frame.sender_id,
      ScreenShareMetadata {
        codec: frame.codec,
        width: frame.width,
        height: frame.height,
      },
    );

    if should_publish_update {
      self.publish_lobby_update();
    }
    self.record_rendered_frame(frame.sender_id, frame.frame_number);
  }

  #[cfg(target_os = "windows")]
  pub(super) fn shared_nv12_planes_surface_for_decode(
    &self,
    surface_cache: &mut HashMap<(UserId, usize, usize), Arc<lurq::app::dx12_render::Dx12Nv12Surface>>,
    user_id: UserId,
    width: u16,
    height: u16,
    y_shared_handle: usize,
    uv_shared_handle: usize,
  ) -> Option<Arc<lurq::app::dx12_render::Dx12Nv12Surface>> {
    if y_shared_handle == 0 || uv_shared_handle == 0 {
      return None;
    }

    if let Some(surface) = surface_cache.get(&(user_id, y_shared_handle, uv_shared_handle)) {
      let image = surface.image_data();
      if image.width() == u32::from(width)
        && image.height() == u32::from(height)
        && image.format() == lurq::images::ImagePixelFormat::Nv12
        && !surface.is_packed_nv12()
        && surface.y_shared_handle_raw() as usize == y_shared_handle
        && surface.uv_shared_handle_raw() as usize == uv_shared_handle
      {
        return Some(surface.clone());
      }
      surface_cache.remove(&(user_id, y_shared_handle, uv_shared_handle));
    }

    {
      let frames = self.frames.lock();
      if let Some(VideoFrameImage::Dx12Surface { image, .. }) = frames.get(&user_id) {
        if image.width() == u32::from(width)
          && image.height() == u32::from(height)
          && image.format() == lurq::images::ImagePixelFormat::Nv12
          && let Some(surface) = surface_cache.get(&(user_id, y_shared_handle, uv_shared_handle))
        {
          return Some(surface.clone());
        }
      }
    }

    let allocator = self.dx12_video_surface_allocator()?;
    match allocator.open_shared_nv12_planes_surface(
      u32::from(width),
      u32::from(height),
      y_shared_handle as isize,
      uv_shared_handle as isize,
    ) {
      Ok(Some(surface)) => {
        let surface = Arc::new(surface);
        let native = surface.native_image_data();
        tracing::debug!(target: "video::decode",
          "[video:decode] opened shared NV12 planes DX12 surface: user={user_id} image={} y_handle=0x{y_shared_handle:x} uv_handle=0x{uv_shared_handle:x} size={}x{} cache_entries={}",
          native.id(),
          width,
          height,
          surface_cache.len() + 1
        );
        let current_key = (user_id, y_shared_handle, uv_shared_handle);
        while surface_cache.len() >= video::SHARED_NV12_PLANES_SURFACE_CACHE_LIMIT {
          let Some(stale_key) = surface_cache.keys().copied().find(|key| *key != current_key) else {
            break;
          };
          surface_cache.remove(&stale_key);
        }
        surface_cache.insert(current_key, surface.clone());
        Some(surface)
      }
      Ok(None) => {
        tracing::debug!(target: "video::decode", "[video:decode] failed to open shared NV12 planes surface: DX12 video surface allocator is not ready");
        None
      }
      Err(error) => {
        tracing::debug!(target: "video::decode", "[video:decode] failed to open shared NV12 planes surface: y_handle=0x{y_shared_handle:x} uv_handle=0x{uv_shared_handle:x} size={}x{} error={error}", width, height);
        None
      }
    }
  }

  #[cfg(target_os = "windows")]
  pub(super) fn dx12_surface_for_decode(
    &self,
    surface_cache: &mut HashMap<(UserId, u16, u16), VecDeque<Arc<lurq::app::dx12_render::Dx12Nv12Surface>>>,
    user_id: UserId,
    width: u16,
    height: u16,
  ) -> Option<Arc<lurq::app::dx12_render::Dx12Nv12Surface>> {
    if !*video::DX12_NATIVE_STREAM_DECODE_SUPPORTED {
      return None;
    }

    let key = (user_id, width, height);
    let surfaces = surface_cache.entry(key).or_default();
    if surfaces.len() >= video::DX12_DECODE_SURFACE_RING_SIZE
      && let Some(surface) = surfaces.pop_front()
    {
      surfaces.push_back(surface.clone());
      return Some(surface);
    }

    let allocator = self.dx12_video_surface_allocator()?;
    match allocator.create_nv12_surface(u32::from(width), u32::from(height)) {
      Ok(Some(surface)) => {
        let surface = Arc::new(surface);
        surfaces.push_back(surface.clone());
        Some(surface)
      }
      Ok(None) => None,
      Err(error) => {
        tracing::debug!(target: "video::decode", "[video:decode] failed to allocate DX12 video surface: {error}");
        None
      }
    }
  }

  #[cfg(target_os = "windows")]
  pub(super) fn present_dx12_frame(
    &self,
    sender_id: UserId,
    frame_number: u32,
    codec: crate::network::protocol::VideoCodecId,
    width: u16,
    height: u16,
    surface: Arc<lurq::app::dx12_render::Dx12Nv12Surface>,
  ) {
    let mut should_publish_update = false;
    let mut dx12_image = surface.dx12_nv12_image();
    dx12_image.frame_number = Some(frame_number);
    let shared_handle = surface.y_shared_handle_raw();
    self
      .dx12_surface_reuse
      .lock()
      .record(sender_id, frame_number, shared_handle);
    let packed_nv12 = surface.is_packed_nv12();
    let (image_id, bumped_version, replace) = {
      let mut frames = self.frames.lock();
      match frames.get_mut(&sender_id) {
        Some(VideoFrameImage::Dx12Surface { image, native, slot })
          if image.width() == u32::from(width)
            && image.height() == u32::from(height)
            && image.format() == lurq::images::ImagePixelFormat::Nv12 =>
        {
          slot.set_image(dx12_image);
          native.bump_version();
          (image.id(), u64::from(frame_number), false)
        }
        _ => {
          let slot = lurq::images::Dx12Nv12ImageSlot::new(dx12_image);
          let native =
            lurq::images::NativeImageData::from_dx12_nv12_slot(u32::from(width), u32::from(height), slot.clone());
          let image = native.image_data();
          native.bump_version();
          let image_id = image.id();
          frames.insert(sender_id, VideoFrameImage::Dx12Surface { image, native, slot });
          (image_id, u64::from(frame_number), true)
        }
      }
    };
    {
      if replace {
        tracing::debug!(
          target: "video::watch",
          "watched stream dx12 image replace user={} image_id={} size={}x{} packed={} handle=0x{:x} version={}",
          sender_id,
          image_id,
          width,
          height,
          packed_nv12,
          shared_handle,
          bumped_version
        );
        tracing::debug!(target: "video::decode",
          "[video:decode] storing DX12 video frame for user {sender_id}: size={width}x{height} packed={packed_nv12} handle=0x{shared_handle:x} version={bumped_version} replace=true"
        );
      } else if bumped_version == 1 || bumped_version % 120 == 0 {
        tracing::debug!(target: "video::decode",
          "[video:decode] updating DX12 video frame for user {sender_id}: size={width}x{height} packed={packed_nv12} handle=0x{shared_handle:x} version={bumped_version} replace=false"
        );
      }
    }

    should_publish_update |= self.update_share_metadata(sender_id, ScreenShareMetadata { codec, width, height });

    if should_publish_update && (bumped_version == 1 || bumped_version % 120 == 0) {
      tracing::debug!(target: "video::decode",
        "[video:decode] publishing lobby update for DX12 frame metadata: user={sender_id} packed={packed_nv12} handle=0x{shared_handle:x} version={bumped_version} forced={should_publish_update}"
      );
    }
    if should_publish_update {
      self.publish_lobby_update();
    }
    self.record_rendered_frame(sender_id, frame_number);
  }

  fn record_rendered_frame(&self, user_id: UserId, frame_number: u32) {
    let now = Instant::now();
    let mut counters = self.render_fps.lock();
    counters
      .entry(user_id)
      .or_insert_with(|| VideoRenderFpsCounter::new(now))
      .record(now, frame_number);
  }

  #[cfg(target_os = "windows")]
  fn dx12_video_surface_allocator(&self) -> Option<lurq::app::dx12_render::Dx12VideoSurfaceAllocator> {
    self.dx12_video_surfaces.clone()
  }

  fn update_share_metadata(&self, sender_id: UserId, metadata: ScreenShareMetadata) -> bool {
    {
      let cached_metadata = self.metadata.lock();
      if cached_metadata.get(&sender_id) == Some(&metadata) {
        return false;
      }
    }

    let mut lobby = self.lobby.lock();
    let Some(share) = lobby
      .screen_shares
      .iter_mut()
      .find(|share| share.sharer_user_id == sender_id)
    else {
      self.metadata.lock().remove(&sender_id);
      return false;
    };

    let changed = if share.metadata != metadata {
      share.metadata = metadata;
      true
    } else {
      false
    };
    self.metadata.lock().insert(sender_id, share.metadata.clone());
    changed
  }

  fn publish_lobby_update(&self) {
    self.lobby_updates.publish();
  }
}

impl VideoRenderFpsCounter {
  fn new(now: Instant) -> Self {
    Self {
      window_started_at: now,
      frames: 0,
      fps: 0,
      last_frame_number: None,
      old_frames: 0,
    }
  }

  fn record(&mut self, now: Instant, frame_number: u32) {
    if self
      .last_frame_number
      .is_some_and(|last_frame_number| frame_number_before(frame_number, last_frame_number))
    {
      self.old_frames = self.old_frames.saturating_add(1);
    }
    self.last_frame_number = Some(frame_number);
    self.frames = self.frames.saturating_add(1);
    self.refresh(now);
  }

  fn sample(&mut self, now: Instant) -> (u32, Option<u32>, u32) {
    self.refresh(now);
    (self.fps, self.last_frame_number, self.old_frames)
  }

  fn refresh(&mut self, now: Instant) {
    let elapsed = now.duration_since(self.window_started_at);
    if elapsed < Duration::from_secs(1) {
      return;
    }
    let elapsed_secs = elapsed.as_secs_f32().max(0.001);
    self.fps = ((self.frames as f32) / elapsed_secs).round() as u32;
    self.frames = 0;
    self.window_started_at = now;
  }
}

#[cfg(target_os = "windows")]
impl Dx12SurfaceReuseTracker {
  fn new() -> Self {
    let now = Instant::now();
    Self {
      frames: HashMap::new(),
      stats: Dx12SurfaceReuseStats::new(now),
    }
  }

  fn clear(&mut self) {
    self.frames.clear();
    self.stats = Dx12SurfaceReuseStats::new(Instant::now());
  }

  fn retain_user(&mut self, user_id: UserId) {
    self.frames.retain(|_, state| state.user_id == user_id);
    self.stats = Dx12SurfaceReuseStats::new(Instant::now());
  }

  fn clear_user(&mut self, user_id: UserId) {
    self.frames.retain(|_, state| state.user_id != user_id);
    self.stats = Dx12SurfaceReuseStats::new(Instant::now());
  }

  fn record(&mut self, user_id: UserId, frame_number: u32, shared_handle: isize) {
    let now = Instant::now();
    let previous = self.frames.insert(
      shared_handle,
      Dx12SurfaceFrameState {
        user_id,
        frame_number,
        updated_at: now,
      },
    );
    let Some(previous) = previous else {
      self.log_if_due(now);
      return;
    };
    if previous.user_id != user_id {
      self.log_if_due(now);
      return;
    }

    let backwards = frame_number_before(frame_number, previous.frame_number);
    let frame_delta = if backwards {
      previous.frame_number.wrapping_sub(frame_number)
    } else {
      frame_number.wrapping_sub(previous.frame_number)
    };
    let age = now.duration_since(previous.updated_at);
    let short_reuse = frame_delta < 8 || age < Duration::from_millis(80);

    self.stats.reuses = self.stats.reuses.saturating_add(1);
    self.stats.last_user_id = Some(user_id);
    self.stats.min_frame_delta = Some(
      self
        .stats
        .min_frame_delta
        .map_or(frame_delta, |min| min.min(frame_delta)),
    );
    self.stats.max_frame_delta = self.stats.max_frame_delta.max(frame_delta);
    self.stats.min_age = Some(self.stats.min_age.map_or(age, |min| min.min(age)));
    self.stats.max_age = self.stats.max_age.max(age);
    if short_reuse {
      self.stats.short_reuses = self.stats.short_reuses.saturating_add(1);
    }
    if backwards {
      self.stats.backwards_reuses = self.stats.backwards_reuses.saturating_add(1);
    }

    if short_reuse || backwards {
      tracing::debug!(
        target: "video::watch",
        "watched stream dx12 surface reuse suspicious user={} handle=0x{:x} previous_frame={} current_frame={} frame_delta={} age_ms={:.1} backwards={} short_reuse={}",
        user_id,
        shared_handle,
        previous.frame_number,
        frame_number,
        frame_delta,
        duration_ms(age),
        backwards,
        short_reuse
      );
    }

    self.log_if_due(now);
  }

  fn log_if_due(&mut self, now: Instant) {
    if now.duration_since(self.stats.started_at) < Duration::from_secs(1) {
      return;
    }
    if self.stats.reuses > 0 {
      tracing::debug!(
        target: "video::watch",
        "watched stream dx12 surface reuse cadence user={:?} tracked_surfaces={} reuses={} short_reuses={} backwards_reuses={} min_frame_delta={:?} max_frame_delta={} min_age_ms={:.1} max_age_ms={:.1}",
        self.stats.last_user_id,
        self.frames.len(),
        self.stats.reuses,
        self.stats.short_reuses,
        self.stats.backwards_reuses,
        self.stats.min_frame_delta,
        self.stats.max_frame_delta,
        self.stats.min_age.map(duration_ms).unwrap_or(0.0),
        duration_ms(self.stats.max_age)
      );
    }
    self.stats = Dx12SurfaceReuseStats::new(now);
  }
}

#[cfg(target_os = "windows")]
impl Dx12SurfaceReuseStats {
  fn new(now: Instant) -> Self {
    Self {
      started_at: now,
      reuses: 0,
      short_reuses: 0,
      backwards_reuses: 0,
      min_frame_delta: None,
      max_frame_delta: 0,
      min_age: None,
      max_age: Duration::ZERO,
      last_user_id: None,
    }
  }
}

fn frame_number_before(frame_number: u32, previous_frame_number: u32) -> bool {
  let delta = previous_frame_number.wrapping_sub(frame_number);
  delta != 0 && delta < (u32::MAX / 2)
}

fn duration_ms(duration: Duration) -> f64 {
  duration.as_secs_f64() * 1000.0
}

fn decoded_pixel_format_to_lurq(format: DecodedVideoPixelFormat) -> lurq::images::ImagePixelFormat {
  match format {
    DecodedVideoPixelFormat::Rgba8 => lurq::images::ImagePixelFormat::Rgba8,
    DecodedVideoPixelFormat::Nv12 => lurq::images::ImagePixelFormat::Nv12,
  }
}
