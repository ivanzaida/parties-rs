#pragma once

#include <cstddef>
#include <cstdint>
#include <functional>

struct ID3D11Texture2D;

namespace parties_rs::video {

enum class VideoCodecId : uint8_t {
    Unknown = 0,
    AV1 = 1,
    H265 = 2,
    H264 = 3,
};

inline const char* codec_name(VideoCodecId codec) {
    switch (codec) {
    case VideoCodecId::AV1:
        return "AV1";
    case VideoCodecId::H265:
        return "H265";
    case VideoCodecId::H264:
        return "H264";
    default:
        return "unknown";
    }
}

enum class VideoBackend : uint8_t {
    NVENC,
    NVDEC,
    MFT,
};

constexpr uint32_t VIDEO_KEYFRAME_INTERVAL_MS = 5000;

template <typename... Args>
inline void native_log_debug(const char*, Args&&...) {}

template <typename... Args>
inline void native_log_info(const char*, Args&&...) {}

template <typename... Args>
inline void native_log_warn(const char*, Args&&...) {}

template <typename... Args>
inline void native_log_error(const char*, Args&&...) {}

struct EncoderInfo {
    VideoBackend backend;
    VideoCodecId codec;
    uint32_t width;
    uint32_t height;
};

struct DecodedFrame {
    const uint8_t* y_plane;
    const uint8_t* u_plane;
    const uint8_t* v_plane;
    uint32_t y_stride;
    uint32_t uv_stride;
    uint32_t width;
    uint32_t height;
    int64_t timestamp;
    bool nv12 = false;
};

struct DecoderInfo {
    VideoBackend backend;
    VideoCodecId codec;
};

class NvencEncoderBase {
public:
    virtual ~NvencEncoderBase() = default;
    virtual bool encode(ID3D11Texture2D* bgra_texture, int64_t timestamp_100ns) = 0;
    virtual bool supports_registered_input() const { return false; }
    virtual int register_input(ID3D11Texture2D*) { return -1; }
    virtual void unregister_inputs() {}
    virtual bool encode_registered(int, int64_t) { return false; }
    virtual void force_keyframe() = 0;
    virtual void set_bitrate(uint32_t bitrate) = 0;
    virtual EncoderInfo info() const = 0;

    std::function<void(const uint8_t* data, size_t len, bool keyframe)> on_encoded;
};

class NvdecDecoderBase {
public:
    virtual ~NvdecDecoderBase() = default;
    virtual bool decode(const uint8_t* data, size_t len, int64_t timestamp) = 0;
    virtual void flush() = 0;
    virtual bool context_lost() const { return false; }
    virtual DecoderInfo info() const = 0;

    std::function<void(const DecodedFrame& frame)> on_decoded;
};

} // namespace parties_rs::video
