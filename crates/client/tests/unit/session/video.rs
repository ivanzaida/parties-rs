use bytes::Bytes;

use super::*;

fn video_packet(sender_id: UserId, frame_number: u32) -> ForwardedVideoFrame {
  ForwardedVideoFrame {
    sender_id,
    frame: crate::network::protocol::data::VideoFrame {
      frame_number,
      timestamp: frame_number,
      keyframe: frame_number == 0,
      width: 1280,
      height: 720,
      codec: VideoCodecId::H264,
      encoded: Bytes::from_static(&[1, 2, 3]),
    },
  }
}

fn queued_video_packet(sender_id: UserId, frame_number: u32) -> QueuedVideoPacket {
  QueuedVideoPacket::from_stream(video_packet(sender_id, frame_number))
}

#[test]
fn video_packet_queue_drops_oldest_packets_when_full() {
  let queue = VideoPacketQueue::new();
  let stop = AtomicBool::new(false);
  let mut batch = Vec::new();
  let mut dropped_senders = HashMap::new();

  for frame_number in 0..(MAX_QUEUED_VIDEO_PACKETS as u32 + 2) {
    queue.push(queued_video_packet(7, frame_number));
  }

  let dropped = queue.pop_batch_into(&stop, &mut batch, &mut dropped_senders, WATCHED_STREAM_QUEUE_IDLE_WAIT);

  assert!(matches!(dropped, VideoPacketPopResult::Batch(2)));
  assert_eq!(dropped_senders.get(&7), Some(&2));
  assert_eq!(batch.len(), MAX_QUEUED_VIDEO_PACKETS);
  assert_eq!(batch.first().map(|packet| packet.frame.frame_number), Some(2));
  assert_eq!(
    batch.last().map(|packet| packet.frame.frame_number),
    Some(MAX_QUEUED_VIDEO_PACKETS as u32 + 1)
  );
}

#[test]
fn video_packet_queue_ignores_push_after_close() {
  let queue = VideoPacketQueue::new();
  let stop = AtomicBool::new(false);
  let mut batch = Vec::new();
  let mut dropped_senders = HashMap::new();

  queue.close();
  queue.push(queued_video_packet(7, 1));

  let result = queue.pop_batch_into(&stop, &mut batch, &mut dropped_senders, WATCHED_STREAM_QUEUE_IDLE_WAIT);

  assert!(matches!(result, VideoPacketPopResult::Closed));
  assert!(batch.is_empty());
  assert!(dropped_senders.is_empty());
}

#[test]
fn video_packet_queue_returns_none_when_stopped_without_packets() {
  let queue = VideoPacketQueue::new();
  let stop = AtomicBool::new(true);
  let mut batch = Vec::new();
  let mut dropped_senders = HashMap::new();

  let result = queue.pop_batch_into(&stop, &mut batch, &mut dropped_senders, WATCHED_STREAM_QUEUE_IDLE_WAIT);

  assert!(matches!(result, VideoPacketPopResult::Closed));
  assert!(batch.is_empty());
  assert!(dropped_senders.is_empty());
}

#[test]
fn latest_watched_frame_number_uses_frame_order_not_queue_order() {
  let batch = vec![
    queued_video_packet(7, 12),
    queued_video_packet(7, 10),
    queued_video_packet(9, 99),
    queued_video_packet(7, 11),
  ];

  assert_eq!(latest_watched_frame_number(&batch, Some(7)), Some(12));
}

#[test]
fn watched_video_batch_orders_frames_from_expected_number() {
  let mut batch = vec![
    queued_video_packet(7, 13),
    queued_video_packet(7, 10),
    queued_video_packet(9, 1),
    queued_video_packet(7, 12),
    queued_video_packet(7, 11),
  ];

  order_watched_video_batch(&mut batch, 7, Some(11));

  let frames = batch
    .iter()
    .filter(|packet| packet.sender_id == 7)
    .map(|packet| packet.frame.frame_number)
    .collect::<Vec<_>>();
  assert_eq!(frames, vec![11, 12, 13, 10]);
}

#[test]
fn video_frame_number_comparison_handles_wraparound() {
  assert!(frame_number_before(u32::MAX, 1));
  assert!(frame_number_after(1, u32::MAX));
  assert!(!frame_number_before(20, 10));
  assert!(!frame_number_after(10, 20));
}
