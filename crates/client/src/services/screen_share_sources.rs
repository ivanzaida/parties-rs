use std::{
  collections::HashMap,
  sync::{Mutex, OnceLock},
  thread,
  time::{Duration, Instant},
};

use lurq::images::ImageData;

use crate::services::{
  desktop_capture::{DesktopCaptureSource, DesktopCaptureSourceKind, DesktopFrame},
  profiler,
  webcam_devices::{webcam_device_id, webcam_devices_with_fallbacks},
};

const DEFAULT_WEBCAM_WIDTH: u32 = 1280;
const DEFAULT_WEBCAM_HEIGHT: u32 = 720;
const MIN_WINDOW_SOURCE_WIDTH: u32 = 128;
const MIN_WINDOW_SOURCE_HEIGHT: u32 = 128;
const PREVIEW_MAX_WIDTH: u32 = 480;
const PREVIEW_MAX_HEIGHT: u32 = 270;

static PREVIEW_CACHE: OnceLock<Mutex<HashMap<ScreenSharePreviewKey, ScreenSharePreview>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub enum ScreenShareSourceKind {
  Screen,
  Window,
  Webcam,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, lurq::DevtoolsInspectable)]
pub struct ScreenSharePreviewKey {
  pub kind: ScreenShareSourceKind,
  pub id: u32,
  pub width: u32,
  pub height: u32,
}

#[derive(Clone)]
pub struct ScreenSharePreview {
  pub image: Option<ImageData>,
}

impl ScreenSharePreview {
  fn empty() -> Self {
    Self { image: None }
  }
}

impl PartialEq for ScreenSharePreview {
  fn eq(&self, other: &Self) -> bool {
    self.image.as_ref().map(ImageData::id) == other.image.as_ref().map(ImageData::id)
  }
}

impl Eq for ScreenSharePreview {}

impl lurq::app::component::DevtoolsInspectable for ScreenSharePreview {
  fn write_info(&self, buffer: &mut Vec<lurq::app::component::ComponentInfo>) {
    let value = self
      .image
      .as_ref()
      .map(|image| format!("loaded {}x{}", image.width(), image.height()))
      .unwrap_or_else(|| "none".to_owned());
    buffer.push(lurq::app::component::ComponentInfo::with_value(
      "preview",
      std::any::type_name::<Self>(),
      value,
    ));
  }
}

#[derive(Clone)]
pub struct ScreenShareSource {
  pub kind: ScreenShareSourceKind,
  pub id: u32,
  pub name: String,
  #[allow(dead_code)]
  pub description: String,
  pub width: u32,
  pub height: u32,
  pub resolution: Option<String>,
}

pub async fn load_source_preview(key: ScreenSharePreviewKey) -> ScreenSharePreview {
  let _span = profiler::span("stream.thumbnail.load");
  let started_at = Instant::now();
  let cache = PREVIEW_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
  if let Some(preview) = cache.lock().expect("preview cache lock poisoned").get(&key) {
    tracing::debug!(target: "stream::thumbnails",
      "[stream/thumbnails/profile] cache hit for {} preview source id={} size={}x{} elapsed={}",
      source_kind_label(key.kind),
      key.id,
      key.width,
      key.height,
      format_duration(started_at.elapsed())
    );
    return preview.clone();
  }

  let (sender, receiver) = tokio::sync::oneshot::channel();
  let thread_name = format!("stream-thumbnail-{}-{}", source_kind_label(key.kind), key.id);
  tracing::debug!(target: "stream::thumbnails",
    "[stream/thumbnails/profile] starting {} preview source id={} size={}x{}",
    source_kind_label(key.kind),
    key.id,
    key.width,
    key.height
  );
  let spawned = thread::Builder::new().name(thread_name).spawn(move || {
    let _span = profiler::span("stream.thumbnail.worker");
    let worker_started_at = Instant::now();
    let _ = sender.send(capture_source_preview(key));
    tracing::debug!(target: "stream::thumbnails",
      "[stream/thumbnails/profile] worker finished {} preview source id={} size={}x{} elapsed={}",
      source_kind_label(key.kind),
      key.id,
      key.width,
      key.height,
      format_duration(worker_started_at.elapsed())
    );
  });

  if let Err(error) = spawned {
    crate::log_once!(warn, target: "stream::thumbnails", "[stream/thumbnails] failed to spawn {} preview task for source id={} size={}x{}: {}",
      source_kind_label(key.kind),
      key.id,
      key.width,
      key.height,
      error
    );
    let preview = store_source_preview(key, ScreenSharePreview::empty());
    tracing::debug!(target: "stream::thumbnails",
      "[stream/thumbnails/profile] finished {} preview source id={} size={}x{} status=spawn_failed elapsed={}",
      source_kind_label(key.kind),
      key.id,
      key.width,
      key.height,
      format_duration(started_at.elapsed())
    );
    return preview;
  }

  let preview = match receiver.await {
    Ok(preview) => preview,
    Err(error) => {
      crate::log_once!(warn, target: "stream::thumbnails", "[stream/thumbnails] failed to receive {} preview task for source id={} size={}x{}: {}",
        source_kind_label(key.kind),
        key.id,
        key.width,
        key.height,
        error
      );
      ScreenSharePreview::empty()
    }
  };

  let preview = store_source_preview(key, preview);
  tracing::debug!(target: "stream::thumbnails",
    "[stream/thumbnails/profile] finished {} preview source id={} size={}x{} status={} elapsed={}",
    source_kind_label(key.kind),
    key.id,
    key.width,
    key.height,
    if preview.image.is_some() { "loaded" } else { "fallback" },
    format_duration(started_at.elapsed())
  );
  preview
}

fn store_source_preview(key: ScreenSharePreviewKey, preview: ScreenSharePreview) -> ScreenSharePreview {
  let cache = PREVIEW_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
  let mut cache = cache.lock().expect("preview cache lock poisoned");
  if cache.len() >= 64 {
    cache.clear();
  }
  cache.entry(key).or_insert_with(|| preview.clone()).clone()
}

pub fn list_screen_sources() -> Vec<ScreenShareSource> {
  let screens = match DesktopCaptureSource::list_screens() {
    Ok(screens) => screens,
    Err(error) => {
      crate::log_once!(warn, target: "stream::thumbnails", "[stream/thumbnails] failed to list screen sources: {error}");
      return Vec::new();
    }
  };

  screens.into_iter().map(desktop_source).collect()
}

pub fn list_window_sources() -> Vec<ScreenShareSource> {
  let windows = match DesktopCaptureSource::list_windows() {
    Ok(windows) => windows,
    Err(error) => {
      crate::log_once!(warn, target: "stream::thumbnails", "[stream/thumbnails] failed to list window sources: {error}");
      return Vec::new();
    }
  };

  windows.into_iter().filter_map(desktop_window_source).take(12).collect()
}

pub fn list_webcam_sources() -> Vec<ScreenShareSource> {
  list_webcam_sources_with_labels("", "", &|_| String::new())
}

pub fn list_webcam_sources_with_labels(
  camera_description: &str,
  default_camera_label: &str,
  indexed_camera_label: &dyn Fn(usize) -> String,
) -> Vec<ScreenShareSource> {
  webcam_devices_with_fallbacks(default_camera_label, indexed_camera_label)
    .into_iter()
    .map(|device| {
      let id = webcam_device_id(&device.value);
      ScreenShareSource {
        kind: ScreenShareSourceKind::Webcam,
        id,
        name: device.label,
        description: camera_description.to_owned(),
        width: DEFAULT_WEBCAM_WIDTH,
        height: DEFAULT_WEBCAM_HEIGHT,
        resolution: source_resolution(DEFAULT_WEBCAM_WIDTH, DEFAULT_WEBCAM_HEIGHT),
      }
    })
    .collect()
}

fn desktop_source(source: DesktopCaptureSource) -> ScreenShareSource {
  ScreenShareSource {
    kind: screen_share_source_kind(source.kind),
    id: source.id,
    name: source.name,
    description: source.description,
    width: source.width,
    height: source.height,
    resolution: source_resolution(source.width, source.height),
  }
}

fn desktop_window_source(source: DesktopCaptureSource) -> Option<ScreenShareSource> {
  if source.width < MIN_WINDOW_SOURCE_WIDTH || source.height < MIN_WINDOW_SOURCE_HEIGHT {
    crate::log_once!(debug, target: "stream::sources", "[stream/sources] ignoring tiny window source: id={} app=\"{}\" title=\"{}\" size={}x{}",
      source.id,
      source.description,
      source.name,
      source.width,
      source.height
    );
    return None;
  }
  Some(desktop_source(source))
}

fn capture_source_preview(key: ScreenSharePreviewKey) -> ScreenSharePreview {
  match key.kind {
    ScreenShareSourceKind::Screen => capture_screen_preview(key),
    ScreenShareSourceKind::Window => capture_window_preview(key),
    ScreenShareSourceKind::Webcam => capture_webcam_preview(key),
  }
}

fn capture_webcam_preview(key: ScreenSharePreviewKey) -> ScreenSharePreview {
  crate::log_once!(debug, target: "stream::thumbnails", "[stream/thumbnails] webcam preview is not available yet; using fallback icon for source id={}",
    key.id
  );
  ScreenSharePreview::empty()
}

fn capture_screen_preview(key: ScreenSharePreviewKey) -> ScreenSharePreview {
  let capture = || DesktopCaptureSource::find(DesktopCaptureSourceKind::Screen, key.id)?.capture_snapshot_frame();
  captured_preview(key, capture)
}

fn capture_window_preview(key: ScreenSharePreviewKey) -> ScreenSharePreview {
  let capture = || DesktopCaptureSource::find(DesktopCaptureSourceKind::Window, key.id)?.capture_snapshot_frame();
  captured_preview(key, capture)
}

fn captured_preview(
  key: ScreenSharePreviewKey,
  capture: impl FnOnce() -> Result<DesktopFrame, String>,
) -> ScreenSharePreview {
  let _span = profiler::span("stream.thumbnail.create");
  let started_at = Instant::now();
  let capture_started_at = Instant::now();
  let captured = capture();
  let capture_elapsed = capture_started_at.elapsed();
  let preview = match captured {
    Ok(frame) => {
      let captured_width = frame.width;
      let captured_height = frame.height;
      let thumbnail_started_at = Instant::now();
      let preview = thumbnail_image(frame);
      let thumbnail_elapsed = thumbnail_started_at.elapsed();
      match preview.as_ref() {
        Some(preview) => {
          crate::log_once!(debug, target: "stream::thumbnails", "[stream/thumbnails] captured {} preview for source id={} source={}x{} raw={}x{} thumbnail={}x{} capture={} thumbnail={} total={}",
            source_kind_label(key.kind),
            key.id,
            key.width,
            key.height,
            captured_width,
            captured_height,
            preview.width(),
            preview.height(),
            format_duration(capture_elapsed),
            format_duration(thumbnail_elapsed),
            format_duration(started_at.elapsed())
          )
        }
        None => {
          crate::log_once!(warn, target: "stream::thumbnails", "[stream/thumbnails] invalid {} preview dimensions for source id={} source={}x{} raw={}x{} capture={} thumbnail={} total={}",
            source_kind_label(key.kind),
            key.id,
            key.width,
            key.height,
            captured_width,
            captured_height,
            format_duration(capture_elapsed),
            format_duration(thumbnail_elapsed),
            format_duration(started_at.elapsed())
          )
        }
      }
      preview
    }
    Err(error) => {
      crate::log_once!(warn, target: "stream::thumbnails", "[stream/thumbnails] failed to capture {} preview for source id={} size={}x{} capture={} total={}: {}",
        source_kind_label(key.kind),
        key.id,
        key.width,
        key.height,
        format_duration(capture_elapsed),
        format_duration(started_at.elapsed()),
        error
      );
      None
    }
  };
  ScreenSharePreview { image: preview }
}

fn format_duration(duration: Duration) -> String {
  format!("{:.3}ms", duration.as_secs_f64() * 1000.0)
}

fn source_kind_label(kind: ScreenShareSourceKind) -> &'static str {
  match kind {
    ScreenShareSourceKind::Screen => "screen",
    ScreenShareSourceKind::Window => "window",
    ScreenShareSourceKind::Webcam => "webcam",
  }
}

fn thumbnail_image(frame: DesktopFrame) -> Option<ImageData> {
  let width = frame.width;
  let height = frame.height;
  if width == 0 || height == 0 {
    return None;
  }

  let scale = (PREVIEW_MAX_WIDTH as f32 / width as f32)
    .min(PREVIEW_MAX_HEIGHT as f32 / height as f32)
    .min(1.0);
  let thumbnail_width = ((width as f32 * scale).round() as u32).max(1);
  let thumbnail_height = ((height as f32 * scale).round() as u32).max(1);
  let thumbnail = if thumbnail_width == width && thumbnail_height == height {
    frame.rgba
  } else {
    resize_rgba_nearest(&frame.rgba, width, height, thumbnail_width, thumbnail_height)?
  };

  Some(ImageData::from_rgba(thumbnail, thumbnail_width, thumbnail_height))
}

fn resize_rgba_nearest(
  source: &[u8],
  width: u32,
  height: u32,
  target_width: u32,
  target_height: u32,
) -> Option<Vec<u8>> {
  if source.len() != (width as usize).checked_mul(height as usize)?.checked_mul(4)? {
    return None;
  }

  let mut target = vec![
    0;
    (target_width as usize)
      .checked_mul(target_height as usize)?
      .checked_mul(4)?
  ];
  for y in 0..target_height {
    let source_y = (u64::from(y) * u64::from(height) / u64::from(target_height)) as u32;
    for x in 0..target_width {
      let source_x = (u64::from(x) * u64::from(width) / u64::from(target_width)) as u32;
      let source_index = ((source_y * width + source_x) * 4) as usize;
      let target_index = ((y * target_width + x) * 4) as usize;
      target[target_index..target_index + 4].copy_from_slice(&source[source_index..source_index + 4]);
    }
  }
  Some(target)
}

fn source_resolution(width: u32, height: u32) -> Option<String> {
  if width > 0 && height > 0 {
    Some(format!("{width}x{height}"))
  } else {
    None
  }
}

fn screen_share_source_kind(kind: DesktopCaptureSourceKind) -> ScreenShareSourceKind {
  match kind {
    DesktopCaptureSourceKind::Screen => ScreenShareSourceKind::Screen,
    DesktopCaptureSourceKind::Window => ScreenShareSourceKind::Window,
  }
}
