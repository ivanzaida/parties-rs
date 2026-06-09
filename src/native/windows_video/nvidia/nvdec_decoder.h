#pragma once

#include "common/video_types.h"
#include "nvidia_loader.h"

#include <cstdint>
#include <vector>

namespace parties_rs::video::nvidia {

class NvdecDecoder final : public NvdecDecoderBase {
public:
    NvdecDecoder();
    ~NvdecDecoder() override;

    bool init(VideoCodecId codec, uint32_t width, uint32_t height);

    bool decode(const uint8_t* data, size_t len, int64_t timestamp) override;
    void flush() override;
    bool context_lost() const override { return context_lost_; }
    DecoderInfo info() const override;
    void set_output_buffer(uint8_t* nv12, size_t nv12_size);
    void set_output_arrays(CUarray y_array, CUarray uv_array, uint32_t width, uint32_t height);
    bool set_output_d3d12_textures(
        void* y_handle, unsigned long long y_size,
        void* uv_handle, unsigned long long uv_size,
        uint32_t width, uint32_t height);
    void clear_output_d3d12_textures();

private:
    static int CUDAAPI handle_sequence(void* user, CUVIDEOFORMAT* fmt);
    static int CUDAAPI handle_decode(void* user, CUVIDPICPARAMS* pic);
    static int CUDAAPI handle_display(void* user, CUVIDPARSERDISPINFO* info);

    int on_sequence(CUVIDEOFORMAT* fmt);
    int on_decode(CUVIDPICPARAMS* pic);
    int on_display(CUVIDPARSERDISPINFO* info);

    struct ExternalArray {
        void* handle = nullptr;
        unsigned long long size = 0;
        uint32_t width = 0;
        uint32_t height = 0;
        uint32_t channels = 0;
        CUexternalMemory memory = nullptr;
        CUmipmappedArray mipmapped = nullptr;
        CUarray array = nullptr;
    };

    bool ensure_external_array(
        ExternalArray& target, void* handle, unsigned long long size,
        uint32_t width, uint32_t height, uint32_t channels);
    void release_external_array(ExternalArray& target);

    CudaApi cuda_{};
    CuvidApi cuvid_{};

    CUcontext cu_ctx_ = nullptr;
    CUvideoparser parser_ = nullptr;
    CUvideodecoder decoder_ = nullptr;

    VideoCodecId codec_ = VideoCodecId::AV1;
    uint32_t width_ = 0;
    uint32_t height_ = 0;
    uint32_t bit_depth_ = 8;
    uint32_t num_decode_surfaces_ = 0;

    uint8_t* pinned_nv12_ = nullptr;
    size_t pinned_nv12_size_ = 0;
    uint8_t* external_nv12_ = nullptr;
    size_t external_nv12_size_ = 0;
    CUarray external_y_array_ = nullptr;
    CUarray external_uv_array_ = nullptr;
    uint32_t external_array_width_ = 0;
    uint32_t external_array_height_ = 0;
    ExternalArray external_y_texture_;
    ExternalArray external_uv_texture_;

    bool initialized_ = false;
    bool context_lost_ = false;
};

} // namespace parties_rs::video::nvidia
