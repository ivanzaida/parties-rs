#include <client/screen_capture.h>
#include "nvidia/nvenc_encoder.h"

#include <d3d11.h>
#include <winrt/base.h>
#include <wrl/client.h>

#include <cstdint>
#include <memory>
#include <mutex>
#include <vector>

namespace {

using parties::VideoCodecId;
using parties::client::CaptureTarget;
using parties::client::ScreenCapture;
using parties::encdec::nvidia::NvencEncoder;
using Microsoft::WRL::ComPtr;

struct InputViewCacheEntry {
    ID3D11Texture2D* texture = nullptr;
    ComPtr<ID3D11VideoProcessorInputView> view;
};

struct GpuStreamBridge {
    ~GpuStreamBridge() {
        capture.shutdown();
        if (apartment_initialized) {
            winrt::uninit_apartment();
        }
    }

    ScreenCapture capture;
    ComPtr<ID3D11DeviceContext> d3d_context;
    ComPtr<ID3D11VideoDevice> video_device;
    ComPtr<ID3D11VideoContext> video_context;
    ComPtr<ID3D11VideoProcessorEnumerator> video_enumerator;
    ComPtr<ID3D11VideoProcessor> video_processor;
    ComPtr<ID3D11VideoProcessorOutputView> scaled_output_view;
    ComPtr<ID3D11Texture2D> scaled_texture;
    NvencEncoder encoder;
    std::vector<InputViewCacheEntry> input_view_cache;
    std::mutex mutex;
    std::vector<uint8_t> pending;
    std::vector<uint8_t> readable;
    bool pending_keyframe = false;
    bool readable_keyframe = false;
    bool encoder_ready = false;
    bool apartment_initialized = false;
    bool scale_required = false;
    int scaled_input_slot = -1;
    int64_t frame_duration_100ns = 333333;
    uint64_t frame_number = 0;
};

VideoCodecId codec_from_u8(uint8_t codec) {
    switch (codec) {
    case 1: return VideoCodecId::AV1;
    case 2: return VideoCodecId::H265;
    case 3: return VideoCodecId::H264;
    default: return VideoCodecId::H264;
    }
}

bool create_scaler(GpuStreamBridge& bridge, uint32_t source_width, uint32_t source_height,
                   uint32_t output_width, uint32_t output_height, uint32_t fps) {
    if (source_width == output_width && source_height == output_height) {
        return true;
    }

    ID3D11Device* device = bridge.capture.device();
    if (!device) {
        return false;
    }

    device->GetImmediateContext(&bridge.d3d_context);
    if (!bridge.d3d_context) {
        return false;
    }

    HRESULT hr = device->QueryInterface(__uuidof(ID3D11VideoDevice), reinterpret_cast<void**>(bridge.video_device.GetAddressOf()));
    if (FAILED(hr)) {
        return false;
    }
    hr = bridge.d3d_context.As(&bridge.video_context);
    if (FAILED(hr)) {
        return false;
    }

    D3D11_TEXTURE2D_DESC texture_desc{};
    texture_desc.Width = output_width;
    texture_desc.Height = output_height;
    texture_desc.MipLevels = 1;
    texture_desc.ArraySize = 1;
    texture_desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    texture_desc.SampleDesc.Count = 1;
    texture_desc.Usage = D3D11_USAGE_DEFAULT;
    texture_desc.BindFlags = D3D11_BIND_RENDER_TARGET;

    hr = device->CreateTexture2D(&texture_desc, nullptr, &bridge.scaled_texture);
    if (FAILED(hr)) {
        return false;
    }

    D3D11_VIDEO_PROCESSOR_CONTENT_DESC content_desc{};
    content_desc.InputFrameFormat = D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE;
    content_desc.InputWidth = source_width;
    content_desc.InputHeight = source_height;
    content_desc.OutputWidth = output_width;
    content_desc.OutputHeight = output_height;
    content_desc.Usage = D3D11_VIDEO_USAGE_PLAYBACK_NORMAL;
    content_desc.InputFrameRate.Numerator = fps;
    content_desc.InputFrameRate.Denominator = 1;
    content_desc.OutputFrameRate.Numerator = fps;
    content_desc.OutputFrameRate.Denominator = 1;

    hr = bridge.video_device->CreateVideoProcessorEnumerator(&content_desc, &bridge.video_enumerator);
    if (FAILED(hr)) {
        return false;
    }
    hr = bridge.video_device->CreateVideoProcessor(bridge.video_enumerator.Get(), 0, &bridge.video_processor);
    if (FAILED(hr)) {
        return false;
    }

    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC output_view_desc{};
    output_view_desc.ViewDimension = D3D11_VPOV_DIMENSION_TEXTURE2D;
    output_view_desc.Texture2D.MipSlice = 0;
    hr = bridge.video_device->CreateVideoProcessorOutputView(
        bridge.scaled_texture.Get(),
        bridge.video_enumerator.Get(),
        &output_view_desc,
        &bridge.scaled_output_view);
    if (FAILED(hr)) {
        return false;
    }

    RECT source_rect{0, 0, static_cast<LONG>(source_width), static_cast<LONG>(source_height)};
    RECT output_rect{0, 0, static_cast<LONG>(output_width), static_cast<LONG>(output_height)};
    bridge.video_context->VideoProcessorSetStreamSourceRect(bridge.video_processor.Get(), 0, TRUE, &source_rect);
    bridge.video_context->VideoProcessorSetStreamDestRect(bridge.video_processor.Get(), 0, TRUE, &output_rect);
    bridge.video_context->VideoProcessorSetOutputTargetRect(bridge.video_processor.Get(), TRUE, &output_rect);
    bridge.scale_required = true;
    return true;
}

ID3D11VideoProcessorInputView* input_view_for_frame(GpuStreamBridge& bridge, ID3D11Texture2D* texture) {
    for (auto& entry : bridge.input_view_cache) {
        if (entry.texture == texture) {
            return entry.view.Get();
        }
    }

    if (!bridge.video_device) {
        return nullptr;
    }

    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC input_view_desc{};
    input_view_desc.FourCC = 0;
    input_view_desc.ViewDimension = D3D11_VPIV_DIMENSION_TEXTURE2D;
    input_view_desc.Texture2D.MipSlice = 0;
    input_view_desc.Texture2D.ArraySlice = 0;

    InputViewCacheEntry entry{};
    entry.texture = texture;
    HRESULT hr = bridge.video_device->CreateVideoProcessorInputView(
        texture,
        bridge.video_enumerator.Get(),
        &input_view_desc,
        &entry.view);
    if (FAILED(hr) || !entry.view) {
        return nullptr;
    }

    bridge.input_view_cache.push_back(std::move(entry));
    return bridge.input_view_cache.back().view.Get();
}

ID3D11Texture2D* scale_frame(GpuStreamBridge& bridge, ID3D11Texture2D* texture) {
    if (!bridge.scale_required) {
        return texture;
    }
    if (!texture || !bridge.video_context || !bridge.video_processor || !bridge.video_enumerator || !bridge.scaled_output_view) {
        return nullptr;
    }

    ID3D11VideoProcessorInputView* input_view = input_view_for_frame(bridge, texture);
    if (!input_view) {
        return nullptr;
    }

    D3D11_VIDEO_PROCESSOR_STREAM stream{};
    stream.Enable = TRUE;
    stream.OutputIndex = 0;
    stream.InputFrameOrField = 0;
    stream.PastFrames = 0;
    stream.FutureFrames = 0;
    stream.ppPastSurfaces = nullptr;
    stream.ppFutureSurfaces = nullptr;
    stream.pInputSurface = input_view;

    HRESULT hr = bridge.video_context->VideoProcessorBlt(
        bridge.video_processor.Get(),
        bridge.scaled_output_view.Get(),
        static_cast<UINT>(bridge.frame_number),
        1,
        &stream);
    if (FAILED(hr)) {
        return nullptr;
    }

    return bridge.scaled_texture.Get();
}

} // namespace

extern "C" {

GpuStreamBridge* parties_gpu_stream_create(
    uint8_t source_kind,
    uintptr_t source_handle,
    uint8_t codec,
    uint16_t width,
    uint16_t height,
    uint32_t fps,
    uint32_t bitrate) {
    if (!source_handle || width == 0 || height == 0 || fps == 0 || bitrate == 0) {
        return nullptr;
    }

    auto bridge = std::make_unique<GpuStreamBridge>();
    try {
        winrt::init_apartment(winrt::apartment_type::multi_threaded);
        bridge->apartment_initialized = true;
    } catch (...) {
        // The thread may already have a compatible apartment. Let WGC calls below decide.
    }

    if (!bridge->capture.init()) {
        return nullptr;
    }
    bridge->frame_duration_100ns = static_cast<int64_t>(10'000'000ull / fps);

    CaptureTarget target{};
    target.type = source_kind == 0 ? CaptureTarget::Type::Monitor : CaptureTarget::Type::Window;
    target.handle = reinterpret_cast<void*>(source_handle);
    if (!bridge->capture.start(target, fps)) {
        bridge->capture.shutdown();
        return nullptr;
    }

    const uint32_t source_width = bridge->capture.width();
    const uint32_t source_height = bridge->capture.height();
    if (source_width == 0 || source_height == 0) {
        bridge->capture.shutdown();
        return nullptr;
    }

    const VideoCodecId requested_codec = codec_from_u8(codec);
    if (!bridge->encoder.init(bridge->capture.device(), width, height, fps, bitrate, requested_codec)) {
        bridge->capture.shutdown();
        return nullptr;
    }
    if (bridge->encoder.codec() != requested_codec) {
        bridge->capture.shutdown();
        return nullptr;
    }
    bridge->encoder.force_keyframe();

    if (!create_scaler(*bridge, source_width, source_height, width, height, fps)) {
        bridge->capture.shutdown();
        return nullptr;
    }
    if (bridge->scale_required) {
        bridge->scaled_input_slot = bridge->encoder.register_input(bridge->scaled_texture.Get());
    }

    bridge->encoder.on_encoded = [ptr = bridge.get()](const uint8_t* data, size_t len, bool keyframe) {
        std::lock_guard<std::mutex> lock(ptr->mutex);
        ptr->pending.assign(data, data + len);
        ptr->pending_keyframe = keyframe;
    };

    bridge->capture.on_frame = [ptr = bridge.get()](ID3D11Texture2D* texture, uint32_t, uint32_t) {
        const int64_t timestamp = static_cast<int64_t>(ptr->frame_number++ * ptr->frame_duration_100ns);
        ID3D11Texture2D* encoder_texture = scale_frame(*ptr, texture);
        if (!encoder_texture) {
            return;
        }
        if (ptr->scale_required && ptr->scaled_input_slot >= 0) {
            ptr->encoder.encode_registered(ptr->scaled_input_slot, timestamp);
        } else {
            ptr->encoder.encode(encoder_texture, timestamp);
        }
    };

    bridge->encoder_ready = true;
    return bridge.release();
}

void parties_gpu_stream_destroy(GpuStreamBridge* bridge) {
    delete bridge;
}

void parties_gpu_stream_force_keyframe(GpuStreamBridge* bridge) {
    if (bridge && bridge->encoder_ready) {
        bridge->encoder.force_keyframe();
    }
}

int parties_gpu_stream_poll(GpuStreamBridge* bridge) {
    if (!bridge || !bridge->encoder_ready) {
        return -1;
    }
    std::lock_guard<std::mutex> lock(bridge->mutex);
    if (bridge->pending.empty()) {
        return 0;
    }
    bridge->readable.swap(bridge->pending);
    bridge->pending.clear();
    bridge->readable_keyframe = bridge->pending_keyframe;
    bridge->pending_keyframe = false;
    return 1;
}

const uint8_t* parties_gpu_stream_encoded_ptr(GpuStreamBridge* bridge) {
    if (!bridge || bridge->readable.empty()) {
        return nullptr;
    }
    return bridge->readable.data();
}

uintptr_t parties_gpu_stream_encoded_len(GpuStreamBridge* bridge) {
    if (!bridge) {
        return 0;
    }
    return bridge->readable.size();
}

int parties_gpu_stream_encoded_keyframe(GpuStreamBridge* bridge) {
    if (!bridge) {
        return 0;
    }
    return bridge->readable_keyframe ? 1 : 0;
}

}
