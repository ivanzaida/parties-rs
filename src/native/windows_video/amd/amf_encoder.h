#pragma once

#include "common/video_types.h"

#include "amf/public/common/AMFFactory.h"
#include "amf/public/include/components/VideoEncoderAV1.h"
#include "amf/public/include/components/VideoEncoderHEVC.h"
#include "amf/public/include/components/VideoEncoderVCE.h"

#include <atomic>
#include <wrl/client.h>

struct ID3D11Device;
struct ID3D11DeviceContext;
struct ID3D11Texture2D;

namespace parties_rs::video::amd {

class AmfEncoder final {
public:
    AmfEncoder();
    ~AmfEncoder();

    bool init(ID3D11Device* device, uint32_t width, uint32_t height,
              uint32_t fps, uint32_t bitrate, VideoCodecId preferred_codec);
    bool encode(ID3D11Texture2D* bgra_texture, int64_t timestamp_100ns);
    void force_keyframe();
    EncoderInfo info() const;

    std::function<void(const uint8_t* data, size_t len, bool keyframe)> on_encoded;

private:
    bool try_init_codec(VideoCodecId codec, uint32_t bitrate);
    bool set_common_properties(VideoCodecId codec, uint32_t bitrate);
    bool collect_output(bool* produced);
    bool output_is_keyframe(amf::AMFData* data) const;

    Microsoft::WRL::ComPtr<ID3D11Device> device_;
    Microsoft::WRL::ComPtr<ID3D11DeviceContext> context_;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> staging_texture_;

    amf::AMFContextPtr amf_context_;
    amf::AMFComponentPtr encoder_;

    VideoCodecId codec_ = VideoCodecId::H264;
    uint32_t width_ = 0;
    uint32_t height_ = 0;
    uint32_t fps_ = 30;
    bool initialized_ = false;
    bool factory_initialized_ = false;
    std::atomic<bool> force_keyframe_{false};
};

} // namespace parties_rs::video::amd
