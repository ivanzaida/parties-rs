#pragma once

#include "common/video_types.h"

#include "amf/public/common/AMFFactory.h"
#include "amf/public/include/components/VideoDecoderUVD.h"

#include <array>
#include <cstdint>
#include <functional>
#include <wrl/client.h>

struct ID3D11Device;
struct ID3D11DeviceContext;
struct ID3D11Texture2D;

namespace parties_rs::video::amd {

class AmfDecoder final : public NvdecDecoderBase {
public:
    AmfDecoder();
    ~AmfDecoder() override;

    bool init(ID3D11Device* device, VideoCodecId codec, uint32_t width, uint32_t height);
    void shutdown();

    bool decode(const uint8_t* data, size_t len, int64_t timestamp) override;
    void flush() override;
    DecoderInfo info() const override;

private:
    bool collect_output(bool* produced);
    bool emit_surface(amf::AMFSurface* surface);
    bool emit_dx11_surface(amf::AMFSurface* surface);
    struct ReadbackSlot {
        Microsoft::WRL::ComPtr<ID3D11Texture2D> texture;
        uint32_t width = 0;
        uint32_t height = 0;
        int64_t timestamp = 0;
        bool pending = false;
    };
    bool ensure_readback_texture(ReadbackSlot& slot, ID3D11Texture2D* texture);
    bool map_readback_slot(ReadbackSlot& slot);

    Microsoft::WRL::ComPtr<ID3D11Device> device_;
    Microsoft::WRL::ComPtr<ID3D11DeviceContext> context_;
    std::array<ReadbackSlot, 4> readback_slots_{};
    size_t readback_write_index_ = 0;
    amf::AMFContextPtr amf_context_;
    amf::AMFComponentPtr decoder_;

    VideoCodecId codec_ = VideoCodecId::H264;
    uint32_t width_ = 0;
    uint32_t height_ = 0;
    bool initialized_ = false;
    bool factory_initialized_ = false;

public:
    std::function<bool(amf::AMFSurface* surface)> on_decoded_surface;
};

} // namespace parties_rs::video::amd
