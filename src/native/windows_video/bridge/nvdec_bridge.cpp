#include "common/video_types.h"
#include "nvidia/nvdec_decoder.h"

#include <cstdint>
#include <cstring>
#include <memory>

namespace {

using parties_rs::video::VideoCodecId;
using parties_rs::video::DecodedFrame;
using parties_rs::video::native_log_error;
using parties_rs::video::native_log_info;
using parties_rs::video::nvidia::NvdecDecoder;

struct NvdecBridge {
    NvdecDecoder decoder;
    uint8_t* nv12 = nullptr;
    uintptr_t nv12_len = 0;
    bool decoded = false;
    bool output_enabled = true;
    bool d3d12_output = false;
};

VideoCodecId codec_from_u8(uint8_t codec) {
    switch (codec) {
    case 1: return VideoCodecId::AV1;
    case 2: return VideoCodecId::H265;
    case 3: return VideoCodecId::H264;
    default: return VideoCodecId::AV1;
    }
}

bool valid_nv12_target(const DecodedFrame& frame, uint8_t* nv12, uintptr_t nv12_len) {
    const uint32_t width = frame.width;
    const uint32_t height = frame.height;
    return nv12 && frame.y_plane && frame.u_plane && width > 0 && height > 0 && (width & 1) == 0 && (height & 1) == 0 &&
        nv12_len >= static_cast<uintptr_t>(width) * height + static_cast<uintptr_t>(width) * height / 2;
}

bool copy_nv12(const DecodedFrame& frame, uint8_t* nv12, uintptr_t nv12_len) {
    if (!valid_nv12_target(frame, nv12, nv12_len)) {
        return false;
    }

    const uint32_t width = frame.width;
    const uint32_t height = frame.height;
    uint8_t* y_out = nv12;
    uint8_t* uv_out = nv12 + static_cast<size_t>(width) * height;

    for (uint32_t y = 0; y < height; ++y) {
        std::memcpy(
            y_out + static_cast<size_t>(y) * width,
            frame.y_plane + static_cast<size_t>(y) * frame.y_stride,
            width);
    }

    for (uint32_t y = 0; y < height / 2; ++y) {
        std::memcpy(
            uv_out + static_cast<size_t>(y) * width,
            frame.u_plane + static_cast<size_t>(y) * frame.uv_stride,
            width);
    }

    return true;
}

bool i420_to_nv12(const DecodedFrame& frame, uint8_t* nv12, uintptr_t nv12_len) {
    if (!valid_nv12_target(frame, nv12, nv12_len) || !frame.v_plane) {
        return false;
    }

    const uint32_t width = frame.width;
    const uint32_t height = frame.height;
    uint8_t* y_out = nv12;
    uint8_t* uv_out = nv12 + static_cast<size_t>(width) * height;

    for (uint32_t y = 0; y < height; ++y) {
        std::memcpy(
            y_out + static_cast<size_t>(y) * width,
            frame.y_plane + static_cast<size_t>(y) * frame.y_stride,
            width);
    }

    for (uint32_t y = 0; y < height / 2; ++y) {
        const uint8_t* u_row = frame.u_plane + static_cast<size_t>(y) * frame.uv_stride;
        const uint8_t* v_row = frame.v_plane + static_cast<size_t>(y) * frame.uv_stride;
        uint8_t* out_row = uv_out + static_cast<size_t>(y) * width;
        for (uint32_t x = 0; x < width / 2; ++x) {
            out_row[x * 2] = u_row[x];
            out_row[x * 2 + 1] = v_row[x];
        }
    }

    return true;
}

void on_decoded(NvdecBridge* bridge, const DecodedFrame& frame) {
    if (!bridge) {
        return;
    }

    const uint32_t width = frame.width;
    const uint32_t height = frame.height;
    if (bridge->d3d12_output && frame.nv12 && width > 0 && height > 0 && (width & 1) == 0 && (height & 1) == 0) {
        bridge->decoded = true;
        return;
    }

    const uintptr_t y_size = static_cast<uintptr_t>(width) * height;
    if (bridge->nv12 && frame.nv12 && frame.y_plane == bridge->nv12 && frame.u_plane == bridge->nv12 + y_size) {
        bridge->decoded = valid_nv12_target(frame, bridge->nv12, bridge->nv12_len);
        return;
    }

    if (frame.nv12 || frame.v_plane == nullptr) {
        bridge->decoded = copy_nv12(frame, bridge->nv12, bridge->nv12_len);
    } else {
        bridge->decoded = i420_to_nv12(frame, bridge->nv12, bridge->nv12_len);
    }
}

void set_output_enabled(NvdecBridge* bridge, bool enabled) {
    if (!bridge || bridge->output_enabled == enabled) {
        return;
    }
    bridge->output_enabled = enabled;
    if (enabled) {
        bridge->decoder.on_decoded = [bridge](const DecodedFrame& frame) { on_decoded(bridge, frame); };
    } else {
        bridge->decoder.on_decoded = nullptr;
    }
}

} // namespace

extern "C" {

NvdecBridge* parties_nvdec_create(uint8_t codec, uint16_t width, uint16_t height) {
    native_log_info("NVDEC bridge create requested: codec={} size={}x{}", codec, width, height);
    auto bridge = std::make_unique<NvdecBridge>();
    bridge->decoder.on_decoded = [ptr = bridge.get()](const DecodedFrame& frame) { on_decoded(ptr, frame); };

    if (!bridge->decoder.init(codec_from_u8(codec), width, height)) {
        native_log_error("NVDEC bridge decoder init failed");
        return nullptr;
    }

    native_log_info("NVDEC bridge ready: codec={} size={}x{}", codec, width, height);
    return bridge.release();
}

void parties_nvdec_destroy(NvdecBridge* bridge) {
    delete bridge;
}

int parties_nvdec_decode(
    NvdecBridge* bridge,
    const uint8_t* data,
    uintptr_t len,
    int64_t timestamp,
    uint8_t* nv12,
    uintptr_t nv12_len) {
    if (!bridge || !data || len == 0) {
        native_log_error("NVDEC bridge decode rejected invalid input");
        return -1;
    }

    bridge->nv12 = nv12;
    bridge->nv12_len = nv12_len;
    bridge->decoded = false;
    bridge->d3d12_output = false;
    set_output_enabled(bridge, nv12 && nv12_len > 0);
    bridge->decoder.set_output_buffer(nv12, nv12_len);
    const bool ok = bridge->decoder.decode(data, static_cast<size_t>(len), timestamp);
    bridge->decoder.set_output_buffer(nullptr, 0);
    bridge->nv12 = nullptr;
    bridge->nv12_len = 0;
    if (!ok) {
        native_log_error("NVDEC bridge decoder rejected frame");
        return -1;
    }
    return bridge->decoded ? 1 : 0;
}

int parties_nvdec_decode_to_d3d12(
    NvdecBridge* bridge,
    const uint8_t* data,
    uintptr_t len,
    int64_t timestamp,
    uintptr_t y_handle,
    uint64_t y_size,
    uintptr_t uv_handle,
    uint64_t uv_size,
    uint16_t width,
    uint16_t height) {
    if (!bridge || !data || len == 0 || !y_handle || !uv_handle || width == 0 || height == 0) {
        native_log_error("NVDEC bridge D3D12 decode rejected invalid input");
        return -1;
    }

    bridge->nv12 = nullptr;
    bridge->nv12_len = 0;
    bridge->decoded = false;
    bridge->d3d12_output = true;
    set_output_enabled(bridge, true);
    if (!bridge->decoder.set_output_d3d12_textures(
            reinterpret_cast<void*>(y_handle),
            y_size,
            reinterpret_cast<void*>(uv_handle),
            uv_size,
            width,
            height)) {
        native_log_error("NVDEC bridge failed to set D3D12 output textures");
        bridge->d3d12_output = false;
        bridge->decoder.clear_output_d3d12_textures();
        return -1;
    }

    const bool ok = bridge->decoder.decode(data, static_cast<size_t>(len), timestamp);
    bridge->decoder.clear_output_d3d12_textures();
    bridge->d3d12_output = false;
    if (!ok) {
        native_log_error("NVDEC bridge decoder rejected D3D12 frame");
        return -1;
    }
    return bridge->decoded ? 1 : 0;
}

}
