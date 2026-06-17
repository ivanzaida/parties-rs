# Screen Sharing

Screen sharing supports screen, window, webcam, and stream audio workflows.

## Stream Setup

The stream modal gathers source, codec, bitrate, FPS, scaling, webcam, and audio settings. The session layer starts `VideoBroadcast` with the selected `VideoBroadcastConfig`.

## Viewing Streams

Remote stream metadata arrives through control messages. Video packets are decoded by a per-user decode pool and presented through the video sink.

## Codecs

Supported protocol codecs are AV1, H.265, and H.264. Platform support depends on the operating system and hardware. See [Video](../architecture/video.md).

