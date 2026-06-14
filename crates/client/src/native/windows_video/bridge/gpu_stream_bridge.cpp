#include "capture/windows_screen_capture.h"
#include "amd/amf_encoder.h"
#include "common/video_types.h"
#include "nvidia/nvenc_encoder.h"

#include <d3d11.h>
#include <winrt/base.h>
#include <wrl/client.h>

#include <cstdint>
#include <condition_variable>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <mutex>
#include <vector>

namespace {

using parties_rs::video::VideoCodecId;
using parties_rs::video::CaptureTarget;
using parties_rs::video::ScreenCapture;
using parties_rs::video::native_log_error;
using parties_rs::video::native_log_info;
using parties_rs::video::native_log_warn;
using parties_rs::video::amd::AmfEncoder;
using parties_rs::video::nvidia::NvencEncoder;
using Microsoft::WRL::ComPtr;

struct InputViewCacheEntry {
    ID3D11Texture2D* texture = nullptr;
    ComPtr<ID3D11VideoProcessorInputView> view;
};

struct RegisteredInputCacheEntry {
    ID3D11Texture2D* texture = nullptr;
    int slot = -1;
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
    NvencEncoder nvenc;
    AmfEncoder amf;
    std::vector<InputViewCacheEntry> input_view_cache;
    std::vector<RegisteredInputCacheEntry> registered_input_cache;
    std::mutex mutex;
    std::vector<uint8_t> pending;
    std::vector<uint8_t> readable;
    bool pending_keyframe = false;
    bool readable_keyframe = false;
    bool encoder_ready = false;
    bool use_amf = false;
    bool apartment_initialized = false;
    bool scale_required = false;
    int scaled_input_slot = -1;
    uint32_t source_width = 0;
    uint32_t source_height = 0;
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

void clear_scaler(GpuStreamBridge& bridge) {
    if (!bridge.use_amf) {
        bridge.nvenc.unregister_inputs();
    }
    bridge.scaled_input_slot = -1;
    bridge.scale_required = false;
    bridge.input_view_cache.clear();
    bridge.registered_input_cache.clear();
    bridge.scaled_output_view.Reset();
    bridge.scaled_texture.Reset();
    bridge.video_processor.Reset();
    bridge.video_enumerator.Reset();
}

bool configure_scaler(GpuStreamBridge& bridge, uint32_t source_width, uint32_t source_height,
                      uint32_t output_width, uint32_t output_height, uint32_t fps) {
    clear_scaler(bridge);

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

bool register_scaled_input(GpuStreamBridge& bridge) {
    if (!bridge.scale_required) {
        return true;
    }
    if (bridge.use_amf) {
        return true;
    }
    bridge.scaled_input_slot = bridge.nvenc.register_input(bridge.scaled_texture.Get());
    return bridge.scaled_input_slot >= 0;
}

int registered_input_slot_for_frame(GpuStreamBridge& bridge, ID3D11Texture2D* texture) {
    if (!texture || bridge.scale_required || bridge.use_amf) {
        return -1;
    }

    for (const auto& entry : bridge.registered_input_cache) {
        if (entry.texture == texture) {
            return entry.slot;
        }
    }

    int slot = bridge.nvenc.register_input(texture);
    if (slot < 0) {
        return -1;
    }

    bridge.registered_input_cache.push_back({texture, slot});
    return slot;
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

bool copy_texture_to_rgba(ScreenCapture& capture, ID3D11Texture2D* texture, uint32_t width, uint32_t height, std::vector<uint8_t>& rgba) {
    if (!texture || width == 0 || height == 0) {
        return false;
    }

    ID3D11Device* device = capture.device();
    ID3D11DeviceContext* context = capture.context();
    if (!device || !context) {
        return false;
    }

    D3D11_TEXTURE2D_DESC desc{};
    texture->GetDesc(&desc);
    desc.Width = width;
    desc.Height = height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.SampleDesc.Quality = 0;
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    desc.MiscFlags = 0;

    ComPtr<ID3D11Texture2D> staging;
    HRESULT hr = device->CreateTexture2D(&desc, nullptr, &staging);
    if (FAILED(hr) || !staging) {
        native_log_error("WGC snapshot staging texture failed: {:#010x}", static_cast<unsigned>(hr));
        return false;
    }

    context->CopyResource(staging.Get(), texture);

    D3D11_MAPPED_SUBRESOURCE mapped{};
    hr = context->Map(staging.Get(), 0, D3D11_MAP_READ, 0, &mapped);
    if (FAILED(hr)) {
        native_log_error("WGC snapshot map failed: {:#010x}", static_cast<unsigned>(hr));
        return false;
    }

    rgba.resize(static_cast<size_t>(width) * static_cast<size_t>(height) * 4);
    const auto* source = static_cast<const uint8_t*>(mapped.pData);
    for (uint32_t y = 0; y < height; ++y) {
        const uint8_t* source_row = source + static_cast<size_t>(mapped.RowPitch) * y;
        uint8_t* target_row = rgba.data() + static_cast<size_t>(width) * y * 4;
        for (uint32_t x = 0; x < width; ++x) {
            const uint8_t* bgra = source_row + static_cast<size_t>(x) * 4;
            uint8_t* out = target_row + static_cast<size_t>(x) * 4;
            out[0] = bgra[2];
            out[1] = bgra[1];
            out[2] = bgra[0];
            out[3] = bgra[3];
        }
    }

    context->Unmap(staging.Get(), 0);
    return true;
}

} // namespace

extern "C" {

uint8_t* parties_wgc_snapshot_capture(
    uint8_t source_kind,
    uintptr_t source_handle,
    uint32_t timeout_ms,
    uint32_t* out_width,
    uint32_t* out_height,
    uintptr_t* out_len) {
    if (!source_handle || !out_width || !out_height || !out_len) {
        native_log_error("WGC snapshot rejected invalid arguments");
        return nullptr;
    }

    *out_width = 0;
    *out_height = 0;
    *out_len = 0;

    bool apartment_initialized = false;
    try {
        winrt::init_apartment(winrt::apartment_type::multi_threaded);
        apartment_initialized = true;
    } catch (...) {
        // The worker may already be in a compatible apartment.
    }

    ScreenCapture capture;
    if (!capture.init()) {
        native_log_error("WGC snapshot capture init failed");
        if (apartment_initialized) {
            winrt::uninit_apartment();
        }
        return nullptr;
    }

    std::mutex mutex;
    std::condition_variable cv;
    std::vector<uint8_t> rgba;
    uint32_t frame_width = 0;
    uint32_t frame_height = 0;
    bool completed = false;
    bool failed = false;

    capture.on_frame = [&](ID3D11Texture2D* texture, uint32_t width, uint32_t height) {
        std::lock_guard<std::mutex> lock(mutex);
        if (completed) {
            return;
        }
        if (copy_texture_to_rgba(capture, texture, width, height, rgba)) {
            frame_width = width;
            frame_height = height;
        } else {
            failed = true;
        }
        completed = true;
        cv.notify_one();
    };

    capture.on_closed = [&] {
        std::lock_guard<std::mutex> lock(mutex);
        failed = true;
        completed = true;
        cv.notify_one();
    };

    CaptureTarget target{};
    target.type = source_kind == 0 ? CaptureTarget::Type::Monitor : CaptureTarget::Type::Window;
    target.handle = reinterpret_cast<void*>(source_handle);
    if (!capture.start(target, 30)) {
        native_log_error("WGC snapshot capture start failed");
        capture.shutdown();
        if (apartment_initialized) {
            winrt::uninit_apartment();
        }
        return nullptr;
    }

    {
        std::unique_lock<std::mutex> lock(mutex);
        const auto timeout = std::chrono::milliseconds(timeout_ms ? timeout_ms : 1000);
        cv.wait_for(lock, timeout, [&] { return completed; });
    }

    capture.shutdown();

    if (apartment_initialized) {
        winrt::uninit_apartment();
    }

    if (!completed || failed || rgba.empty() || frame_width == 0 || frame_height == 0) {
        native_log_error("WGC snapshot failed or timed out");
        return nullptr;
    }

    uint8_t* bytes = static_cast<uint8_t*>(std::malloc(rgba.size()));
    if (!bytes) {
        native_log_error("WGC snapshot allocation failed");
        return nullptr;
    }
    std::memcpy(bytes, rgba.data(), rgba.size());
    *out_width = frame_width;
    *out_height = frame_height;
    *out_len = static_cast<uintptr_t>(rgba.size());
    return bytes;
}

void parties_wgc_snapshot_free(uint8_t* bytes) {
    std::free(bytes);
}

GpuStreamBridge* create_gpu_stream(
    uint8_t source_kind,
    uintptr_t source_handle,
    uint8_t codec,
    uint16_t width,
    uint16_t height,
    uint32_t fps,
    uint32_t bitrate,
    bool use_amf) {
    native_log_info("{} GPU stream create requested: source_kind={} source_handle={} codec={} output={}x{} fps={} bitrate={}",
        use_amf ? "AMF" : "NVENC", source_kind, source_handle, codec, width, height, fps, bitrate);
    if (!source_handle || width == 0 || height == 0 || fps == 0 || bitrate == 0) {
        native_log_error("GPU stream create rejected invalid arguments");
        return nullptr;
    }

    auto bridge = std::make_unique<GpuStreamBridge>();
    bridge->use_amf = use_amf;
    try {
        winrt::init_apartment(winrt::apartment_type::multi_threaded);
        bridge->apartment_initialized = true;
    } catch (...) {
        // The thread may already have a compatible apartment. Let WGC calls below decide.
    }

    if (!bridge->capture.init()) {
        native_log_error("GPU stream capture init failed");
        return nullptr;
    }
    bridge->frame_duration_100ns = static_cast<int64_t>(10'000'000ull / fps);

    CaptureTarget target{};
    target.type = source_kind == 0 ? CaptureTarget::Type::Monitor : CaptureTarget::Type::Window;
    target.handle = reinterpret_cast<void*>(source_handle);
    if (!bridge->capture.start(target, fps)) {
        native_log_error("GPU stream capture start failed");
        bridge->capture.shutdown();
        return nullptr;
    }

    bridge->source_width = bridge->capture.width();
    bridge->source_height = bridge->capture.height();
    if (bridge->source_width == 0 || bridge->source_height == 0) {
        native_log_error("GPU stream capture returned invalid source size: {}x{}", bridge->source_width, bridge->source_height);
        bridge->capture.shutdown();
        return nullptr;
    }

    const VideoCodecId requested_codec = codec_from_u8(codec);
    const bool encoder_ready = use_amf
        ? bridge->amf.init(bridge->capture.device(), width, height, fps, bitrate, requested_codec)
        : bridge->nvenc.init(bridge->capture.device(), width, height, fps, bitrate, requested_codec);
    if (!encoder_ready) {
        native_log_error("GPU stream encoder init failed");
        bridge->capture.shutdown();
        return nullptr;
    }
    const auto encoder_info = use_amf ? bridge->amf.info() : bridge->nvenc.info();
    if (encoder_info.codec != requested_codec) {
        native_log_error("GPU stream encoder selected unexpected codec: requested={} actual={}",
            static_cast<int>(requested_codec), static_cast<int>(encoder_info.codec));
        bridge->capture.shutdown();
        return nullptr;
    }
    if (use_amf) {
        bridge->amf.force_keyframe();
    } else {
        bridge->nvenc.force_keyframe();
    }

    if (!configure_scaler(*bridge, bridge->source_width, bridge->source_height, width, height, fps)) {
        native_log_error("GPU stream scaler init failed: source={}x{} output={}x{}", bridge->source_width, bridge->source_height, width, height);
        bridge->capture.shutdown();
        return nullptr;
    }
    if (!register_scaled_input(*bridge)) {
        native_log_error("GPU stream scaled input registration failed");
        bridge->capture.shutdown();
        return nullptr;
    }

    auto on_encoded = [ptr = bridge.get()](const uint8_t* data, size_t len, bool keyframe) {
        std::lock_guard<std::mutex> lock(ptr->mutex);
        ptr->pending.assign(data, data + len);
        ptr->pending_keyframe = keyframe;
    };
    if (use_amf) {
        bridge->amf.on_encoded = on_encoded;
    } else {
        bridge->nvenc.on_encoded = on_encoded;
    }

    bridge->capture.on_frame = [ptr = bridge.get(), width, height, fps](ID3D11Texture2D* texture, uint32_t frame_width, uint32_t frame_height) {
        const int64_t timestamp = static_cast<int64_t>(ptr->frame_number++ * ptr->frame_duration_100ns);
        if (frame_width == 0 || frame_height == 0) {
            native_log_warn("GPU stream skipped zero-sized frame: {}x{}", frame_width, frame_height);
            return;
        }
        if (frame_width != ptr->source_width || frame_height != ptr->source_height) {
            native_log_info("GPU stream source resized: {}x{} -> {}x{}",
                ptr->source_width, ptr->source_height, frame_width, frame_height);
            if (!configure_scaler(*ptr, frame_width, frame_height, width, height, fps)) {
                native_log_error("GPU stream scaler reconfigure failed: source={}x{} output={}x{}",
                    frame_width, frame_height, width, height);
                return;
            }
            ptr->source_width = frame_width;
            ptr->source_height = frame_height;
            if (!register_scaled_input(*ptr)) {
                native_log_error("GPU stream scaled input re-registration failed");
                return;
            }
            if (ptr->use_amf) {
                ptr->amf.force_keyframe();
            } else {
                ptr->nvenc.force_keyframe();
            }
        }
        ID3D11Texture2D* encoder_texture = scale_frame(*ptr, texture);
        if (!encoder_texture) {
            return;
        }
        if (ptr->use_amf) {
            ptr->amf.encode(encoder_texture, timestamp);
        } else if (ptr->scale_required && ptr->scaled_input_slot >= 0) {
            ptr->nvenc.encode_registered(ptr->scaled_input_slot, timestamp);
        } else {
            int slot = registered_input_slot_for_frame(*ptr, encoder_texture);
            if (slot >= 0) {
                ptr->nvenc.encode_registered(slot, timestamp);
            } else {
                ptr->nvenc.encode(encoder_texture, timestamp);
            }
        }
    };

    bridge->encoder_ready = true;
    native_log_info("{} GPU stream ready: source={}x{} output={}x{} scale_required={}", use_amf ? "AMF" : "NVENC", bridge->source_width, bridge->source_height, width, height, bridge->scale_required);
    return bridge.release();
}

GpuStreamBridge* parties_gpu_stream_create(
    uint8_t source_kind,
    uintptr_t source_handle,
    uint8_t codec,
    uint16_t width,
    uint16_t height,
    uint32_t fps,
    uint32_t bitrate) {
    return create_gpu_stream(source_kind, source_handle, codec, width, height, fps, bitrate, false);
}

GpuStreamBridge* parties_amf_gpu_stream_create(
    uint8_t source_kind,
    uintptr_t source_handle,
    uint8_t codec,
    uint16_t width,
    uint16_t height,
    uint32_t fps,
    uint32_t bitrate) {
    return create_gpu_stream(source_kind, source_handle, codec, width, height, fps, bitrate, true);
}

void parties_gpu_stream_destroy(GpuStreamBridge* bridge) {
    delete bridge;
}

void parties_gpu_stream_force_keyframe(GpuStreamBridge* bridge) {
    if (bridge && bridge->encoder_ready) {
        if (bridge->use_amf) {
            bridge->amf.force_keyframe();
        } else {
            bridge->nvenc.force_keyframe();
        }
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
