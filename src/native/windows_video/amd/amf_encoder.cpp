#include "amf_encoder.h"
#include "common/native_profile.h"

#include <d3d11.h>

#include <algorithm>

namespace parties_rs::video::amd {

namespace {

constexpr uint32_t AMF_QUERY_RETRIES = 1;

const wchar_t* component_id(VideoCodecId codec) {
    switch (codec) {
    case VideoCodecId::AV1:
        return AMFVideoEncoder_AV1;
    case VideoCodecId::H265:
        return AMFVideoEncoder_HEVC;
    case VideoCodecId::H264:
        return AMFVideoEncoderVCE_AVC;
    default:
        return AMFVideoEncoderVCE_AVC;
    }
}

bool set_i64(amf::AMFComponent* encoder, const wchar_t* property, amf_int64 value) {
    const AMF_RESULT result = encoder->SetProperty(property, value);
    if (result != AMF_OK) {
        native_log_error("AMF SetProperty failed: result={}", static_cast<int>(result));
        return false;
    }
    return true;
}

bool set_bool(amf::AMFComponent* encoder, const wchar_t* property, bool value) {
    const AMF_RESULT result = encoder->SetProperty(property, value);
    if (result != AMF_OK) {
        native_log_error("AMF SetProperty failed: result={}", static_cast<int>(result));
        return false;
    }
    return true;
}

bool set_size(amf::AMFComponent* encoder, const wchar_t* property, uint32_t width, uint32_t height) {
    const AMF_RESULT result = encoder->SetProperty(
        property,
        ::AMFConstructSize(static_cast<amf_int32>(width), static_cast<amf_int32>(height)));
    if (result != AMF_OK) {
        native_log_error("AMF SetProperty(size) failed: result={}", static_cast<int>(result));
        return false;
    }
    return true;
}

bool set_rate(amf::AMFComponent* encoder, const wchar_t* property, uint32_t fps) {
    const AMF_RESULT result = encoder->SetProperty(
        property,
        ::AMFConstructRate(static_cast<amf_int32>((std::max)(fps, 1u)), 1));
    if (result != AMF_OK) {
        native_log_error("AMF SetProperty(rate) failed: result={}", static_cast<int>(result));
        return false;
    }
    return true;
}

} // namespace

AmfEncoder::AmfEncoder() = default;

AmfEncoder::~AmfEncoder() {
    if (encoder_) {
        encoder_->Drain();
        encoder_->Terminate();
        encoder_ = nullptr;
    }
    if (amf_context_) {
        amf_context_->Terminate();
        amf_context_ = nullptr;
    }
    staging_texture_.Reset();
    context_.Reset();
    device_.Reset();
    if (factory_initialized_) {
        g_AMFFactory.Terminate();
        factory_initialized_ = false;
    }
    initialized_ = false;
}

bool AmfEncoder::init(ID3D11Device* device, uint32_t width, uint32_t height,
                      uint32_t fps, uint32_t bitrate, VideoCodecId preferred_codec) {
    if (initialized_ || !device || width == 0 || height == 0 || fps == 0 || bitrate == 0) {
        return false;
    }

    const AMF_RESULT init_result = g_AMFFactory.Init();
    if (init_result != AMF_OK) {
        native_log_error("AMF runtime init failed: result={}", static_cast<int>(init_result));
        return false;
    }
    factory_initialized_ = true;

    amf::AMFFactory* factory = g_AMFFactory.GetFactory();
    if (!factory) {
        native_log_error("AMF factory unavailable after runtime init");
        return false;
    }

    AMF_RESULT result = factory->CreateContext(&amf_context_);
    if (result != AMF_OK || !amf_context_) {
        native_log_error("AMF CreateContext failed: result={}", static_cast<int>(result));
        return false;
    }

    result = amf_context_->InitDX11(device, amf::AMF_DX11_0);
    if (result != AMF_OK) {
        native_log_error("AMF InitDX11 failed: result={}", static_cast<int>(result));
        return false;
    }

    device_ = device;
    device_->GetImmediateContext(&context_);
    width_ = width;
    height_ = height;
    fps_ = fps;

    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width;
    desc.Height = height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;

    HRESULT hr = device_->CreateTexture2D(&desc, nullptr, &staging_texture_);
    if (FAILED(hr)) {
        native_log_error("AMF CreateTexture2D staging failed: {}", static_cast<int>(hr));
        return false;
    }

    if (try_init_codec(preferred_codec, bitrate)) {
        initialized_ = true;
        return true;
    }

    native_log_error("AMF requested codec is unavailable: {}", codec_name(preferred_codec));
    return false;
}

bool AmfEncoder::try_init_codec(VideoCodecId codec, uint32_t bitrate) {
    amf::AMFComponentPtr candidate;
    AMF_RESULT result = g_AMFFactory.GetFactory()->CreateComponent(amf_context_, component_id(codec), &candidate);
    if (result != AMF_OK || !candidate) {
        native_log_error("AMF CreateComponent failed: codec={} result={}", codec_name(codec), static_cast<int>(result));
        return false;
    }

    encoder_ = candidate;
    if (!set_common_properties(codec, bitrate)) {
        encoder_ = nullptr;
        return false;
    }

    result = encoder_->Init(amf::AMF_SURFACE_BGRA, static_cast<amf_int32>(width_), static_cast<amf_int32>(height_));
    if (result != AMF_OK) {
        native_log_error("AMF encoder Init failed: codec={} result={}", codec_name(codec), static_cast<int>(result));
        encoder_ = nullptr;
        return false;
    }

    codec_ = codec;
    force_keyframe();
    native_log_info("AMF selected codec: {} ({}x{} @ {} fps), bitrate: {} bps",
                    codec_name(codec_), width_, height_, fps_, bitrate);
    return true;
}

bool AmfEncoder::set_common_properties(VideoCodecId codec, uint32_t bitrate) {
    if (!encoder_) {
        return false;
    }

    const uint32_t vbv_bits = (std::max)(bitrate / (std::max)(fps_, 1u), 64'000u);
    if (codec == VideoCodecId::AV1) {
        return set_i64(encoder_, AMF_VIDEO_ENCODER_AV1_USAGE, AMF_VIDEO_ENCODER_AV1_USAGE_LOW_LATENCY) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_AV1_QUALITY_PRESET, AMF_VIDEO_ENCODER_AV1_QUALITY_PRESET_SPEED) &&
               set_size(encoder_, AMF_VIDEO_ENCODER_AV1_FRAMESIZE, width_, height_) &&
               set_rate(encoder_, AMF_VIDEO_ENCODER_AV1_FRAMERATE, fps_) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_AV1_RATE_CONTROL_METHOD, AMF_VIDEO_ENCODER_AV1_RATE_CONTROL_METHOD_CBR) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_AV1_TARGET_BITRATE, bitrate) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_AV1_PEAK_BITRATE, bitrate) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_AV1_VBV_BUFFER_SIZE, vbv_bits) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_AV1_ENCODING_LATENCY_MODE, AMF_VIDEO_ENCODER_AV1_ENCODING_LATENCY_MODE_LOWEST_LATENCY) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_AV1_QUERY_TIMEOUT, 0) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_AV1_OUTPUT_MODE, AMF_VIDEO_ENCODER_AV1_OUTPUT_MODE_FRAME);
    }
    if (codec == VideoCodecId::H265) {
        return set_i64(encoder_, AMF_VIDEO_ENCODER_HEVC_USAGE, AMF_VIDEO_ENCODER_HEVC_USAGE_LOW_LATENCY) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_HEVC_QUALITY_PRESET, AMF_VIDEO_ENCODER_HEVC_QUALITY_PRESET_SPEED) &&
               set_size(encoder_, AMF_VIDEO_ENCODER_HEVC_FRAMESIZE, width_, height_) &&
               set_rate(encoder_, AMF_VIDEO_ENCODER_HEVC_FRAMERATE, fps_) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_HEVC_RATE_CONTROL_METHOD, AMF_VIDEO_ENCODER_HEVC_RATE_CONTROL_METHOD_CBR) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_HEVC_TARGET_BITRATE, bitrate) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_HEVC_PEAK_BITRATE, bitrate) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_HEVC_VBV_BUFFER_SIZE, vbv_bits) &&
               set_bool(encoder_, AMF_VIDEO_ENCODER_HEVC_LOWLATENCY_MODE, true) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_HEVC_QUERY_TIMEOUT, 0) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_HEVC_HEADER_INSERTION_MODE, AMF_VIDEO_ENCODER_HEVC_HEADER_INSERTION_MODE_IDR_ALIGNED) &&
               set_i64(encoder_, AMF_VIDEO_ENCODER_HEVC_OUTPUT_MODE, AMF_VIDEO_ENCODER_HEVC_OUTPUT_MODE_FRAME);
    }

    return set_i64(encoder_, AMF_VIDEO_ENCODER_USAGE, AMF_VIDEO_ENCODER_USAGE_LOW_LATENCY) &&
           set_i64(encoder_, AMF_VIDEO_ENCODER_QUALITY_PRESET, AMF_VIDEO_ENCODER_QUALITY_PRESET_SPEED) &&
           set_size(encoder_, AMF_VIDEO_ENCODER_FRAMESIZE, width_, height_) &&
           set_rate(encoder_, AMF_VIDEO_ENCODER_FRAMERATE, fps_) &&
           set_i64(encoder_, AMF_VIDEO_ENCODER_RATE_CONTROL_METHOD, AMF_VIDEO_ENCODER_RATE_CONTROL_METHOD_CBR) &&
           set_i64(encoder_, AMF_VIDEO_ENCODER_TARGET_BITRATE, bitrate) &&
           set_i64(encoder_, AMF_VIDEO_ENCODER_PEAK_BITRATE, bitrate) &&
           set_i64(encoder_, AMF_VIDEO_ENCODER_VBV_BUFFER_SIZE, vbv_bits) &&
           set_bool(encoder_, AMF_VIDEO_ENCODER_LOWLATENCY_MODE, true) &&
           set_i64(encoder_, AMF_VIDEO_ENCODER_QUERY_TIMEOUT, 0) &&
           set_i64(encoder_, AMF_VIDEO_ENCODER_OUTPUT_MODE, AMF_VIDEO_ENCODER_OUTPUT_MODE_FRAME);
}

bool AmfEncoder::encode(ID3D11Texture2D* bgra_texture, int64_t timestamp_100ns) {
    NativeProfileSpan span("native.amf.encode.total");
    if (!initialized_ || !bgra_texture || !context_ || !encoder_) {
        return false;
    }

    {
        NativeProfileSpan copy_span("native.amf.encode.copy_input_texture");
        context_->CopyResource(staging_texture_.Get(), bgra_texture);
    }

    amf::AMFSurfacePtr surface;
    AMF_RESULT result = AMF_OK;
    {
        NativeProfileSpan surface_span("native.amf.encode.create_surface");
        result = amf_context_->CreateSurfaceFromDX11Native(staging_texture_.Get(), &surface, nullptr);
    }
    if (result != AMF_OK || !surface) {
        native_log_error("AMF CreateSurfaceFromDX11Native failed: result={}", static_cast<int>(result));
        return false;
    }
    surface->SetPts(timestamp_100ns);
    surface->SetDuration(10'000'000 / (std::max)(fps_, 1u));

    if (force_keyframe_.exchange(false, std::memory_order_acq_rel)) {
        if (codec_ == VideoCodecId::AV1) {
            surface->SetProperty(AMF_VIDEO_ENCODER_AV1_FORCE_FRAME_TYPE, AMF_VIDEO_ENCODER_AV1_FORCE_FRAME_TYPE_KEY);
            surface->SetProperty(AMF_VIDEO_ENCODER_AV1_FORCE_INSERT_SEQUENCE_HEADER, true);
        } else if (codec_ == VideoCodecId::H265) {
            surface->SetProperty(AMF_VIDEO_ENCODER_HEVC_FORCE_PICTURE_TYPE, AMF_VIDEO_ENCODER_HEVC_PICTURE_TYPE_IDR);
            surface->SetProperty(AMF_VIDEO_ENCODER_HEVC_INSERT_HEADER, true);
        } else {
            surface->SetProperty(AMF_VIDEO_ENCODER_FORCE_PICTURE_TYPE, AMF_VIDEO_ENCODER_PICTURE_TYPE_IDR);
        }
    }

    {
        NativeProfileSpan submit_span("native.amf.encode.submit_input");
        result = encoder_->SubmitInput(surface);
    }
    if (result == AMF_INPUT_FULL) {
        bool ignored = false;
        collect_output(&ignored);
        NativeProfileSpan submit_span("native.amf.encode.submit_input_retry");
        result = encoder_->SubmitInput(surface);
    }
    if (result != AMF_OK && result != AMF_NEED_MORE_INPUT) {
        native_log_error("AMF SubmitInput failed: result={}", static_cast<int>(result));
        return false;
    }

    bool produced = false;
    return collect_output(&produced);
}

bool AmfEncoder::collect_output(bool* produced) {
    NativeProfileSpan span("native.amf.encode.collect_output");
    if (produced) {
        *produced = false;
    }

    for (uint32_t attempt = 0; attempt < AMF_QUERY_RETRIES; ++attempt) {
        amf::AMFDataPtr data;
        AMF_RESULT result = AMF_OK;
        {
            NativeProfileSpan query_span("native.amf.encode.query_output");
            result = encoder_->QueryOutput(&data);
        }
        if (result == AMF_REPEAT || result == AMF_NEED_MORE_INPUT) {
            continue;
        }
        if (result != AMF_OK) {
            native_log_error("AMF QueryOutput failed: result={}", static_cast<int>(result));
            return false;
        }
        if (!data) {
            continue;
        }

        amf::AMFBufferPtr buffer(data);
        if (!buffer || !buffer->GetNative() || buffer->GetSize() == 0) {
            continue;
        }
        if (on_encoded) {
            on_encoded(static_cast<const uint8_t*>(buffer->GetNative()), buffer->GetSize(), output_is_keyframe(data));
        }
        if (produced) {
            *produced = true;
        }
        return true;
    }
    return true;
}

bool AmfEncoder::output_is_keyframe(amf::AMFData* data) const {
    amf_int64 frame_type = 0;
    if (codec_ == VideoCodecId::AV1) {
        if (data->GetProperty(AMF_VIDEO_ENCODER_AV1_OUTPUT_FRAME_TYPE, &frame_type) == AMF_OK) {
            return frame_type == AMF_VIDEO_ENCODER_AV1_OUTPUT_FRAME_TYPE_KEY ||
                   frame_type == AMF_VIDEO_ENCODER_AV1_OUTPUT_FRAME_TYPE_INTRA_ONLY;
        }
    } else if (codec_ == VideoCodecId::H265) {
        if (data->GetProperty(AMF_VIDEO_ENCODER_HEVC_OUTPUT_DATA_TYPE, &frame_type) == AMF_OK) {
            return frame_type == AMF_VIDEO_ENCODER_HEVC_OUTPUT_DATA_TYPE_IDR ||
                   frame_type == AMF_VIDEO_ENCODER_HEVC_OUTPUT_DATA_TYPE_I;
        }
    } else {
        if (data->GetProperty(AMF_VIDEO_ENCODER_OUTPUT_DATA_TYPE, &frame_type) == AMF_OK) {
            return frame_type == AMF_VIDEO_ENCODER_OUTPUT_DATA_TYPE_IDR ||
                   frame_type == AMF_VIDEO_ENCODER_OUTPUT_DATA_TYPE_I;
        }
    }
    return false;
}

void AmfEncoder::force_keyframe() {
    force_keyframe_.store(true, std::memory_order_release);
}

EncoderInfo AmfEncoder::info() const {
    return {VideoBackend::AMF, codec_, width_, height_};
}

} // namespace parties_rs::video::amd
