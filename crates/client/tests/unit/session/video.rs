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

#[test]
fn video_packet_queue_drops_oldest_packets_when_full() {
  let queue = VideoPacketQueue::new();
  let stop = AtomicBool::new(false);
  let mut batch = Vec::new();
  let mut dropped_senders = HashMap::new();

  for frame_number in 0..(MAX_QUEUED_VIDEO_PACKETS as u32 + 2) {
    queue.push(video_packet(7, frame_number));
  }

  let dropped = queue.pop_batch_into(&stop, &mut batch, &mut dropped_senders);

  assert_eq!(dropped, Some(2));
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
  queue.push(video_packet(7, 1));

  assert_eq!(queue.pop_batch_into(&stop, &mut batch, &mut dropped_senders), None);
  assert!(batch.is_empty());
  assert!(dropped_senders.is_empty());
}

#[test]
fn video_packet_queue_returns_none_when_stopped_without_packets() {
  let queue = VideoPacketQueue::new();
  let stop = AtomicBool::new(true);
  let mut batch = Vec::new();
  let mut dropped_senders = HashMap::new();

  assert_eq!(queue.pop_batch_into(&stop, &mut batch, &mut dropped_senders), None);
  assert!(batch.is_empty());
  assert!(dropped_senders.is_empty());
}
