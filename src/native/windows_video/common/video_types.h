#pragma once

#include <cstddef>
#include <cstdint>
#include <functional>
#include <sstream>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>

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

enum class NativeLogLevel : uint8_t {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
};

void native_log_emit(NativeLogLevel level, const char* message);

inline void native_log_format_into(std::vector<std::string>&) {}

template <typename T, typename... Rest>
inline void native_log_format_into(std::vector<std::string>& out, T&& value, Rest&&... rest) {
    std::ostringstream stream;
    using Value = std::decay_t<T>;
    if constexpr (std::is_same_v<Value, uint8_t> || std::is_same_v<Value, int8_t>) {
        stream << static_cast<int>(value);
    } else {
        stream << std::forward<T>(value);
    }
    out.push_back(stream.str());
    native_log_format_into(out, std::forward<Rest>(rest)...);
}

inline std::string native_log_format(const char* format) {
    return format ? std::string(format) : std::string();
}

template <typename... Args>
inline std::string native_log_format(const char* format, Args&&... args) {
    std::vector<std::string> values;
    values.reserve(sizeof...(Args));
    native_log_format_into(values, std::forward<Args>(args)...);

    std::string message;
    const std::string pattern = format ? std::string(format) : std::string();
    size_t value_index = 0;
    for (size_t index = 0; index < pattern.size(); ++index) {
        if (pattern[index] == '{') {
            const size_t end = pattern.find('}', index + 1);
            if (end != std::string::npos && value_index < values.size()) {
                message += values[value_index++];
                index = end;
                continue;
            }
        }
        message += pattern[index];
    }
    return message;
}

template <typename... Args>
inline void native_log_debug(const char* format, Args&&... args) {
    const std::string message = native_log_format(format, std::forward<Args>(args)...);
    native_log_emit(NativeLogLevel::Debug, message.c_str());
}

template <typename... Args>
inline void native_log_info(const char* format, Args&&... args) {
    const std::string message = native_log_format(format, std::forward<Args>(args)...);
    native_log_emit(NativeLogLevel::Info, message.c_str());
}

template <typename... Args>
inline void native_log_warn(const char* format, Args&&... args) {
    const std::string message = native_log_format(format, std::forward<Args>(args)...);
    native_log_emit(NativeLogLevel::Warn, message.c_str());
}

template <typename... Args>
inline void native_log_error(const char* format, Args&&... args) {
    const std::string message = native_log_format(format, std::forward<Args>(args)...);
    native_log_emit(NativeLogLevel::Error, message.c_str());
}

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
