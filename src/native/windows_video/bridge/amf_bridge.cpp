#include "amd/amf_decoder.h"
#include "amd/amf_encoder.h"
#include "common/native_profile.h"
#include "common/video_types.h"

#include <d3d11.h>
#include <d3d11_1.h>
#include <dxgi1_2.h>
#include <wrl/client.h>

#include <cstdint>
#include <cstring>
#include <memory>
#include <vector>

namespace {

using Microsoft::WRL::ComPtr;
using parties_rs::video::DecodedFrame;
using parties_rs::video::VideoCodecId;
using parties_rs::video::amd::AmfDecoder;
using parties_rs::video::amd::AmfEncoder;
using parties_rs::video::native_log_error;
using parties_rs::video::native_log_info;
using parties_rs::video::native_log_warn;

constexpr UINT AMD_VENDOR_ID = 0x1002;
constexpr size_t SHARED_NV12_TARGET_COUNT = 8;
constexpr DWORD SHARED_NV12_MUTEX_TIMEOUT_MS = 16;

struct SharedNv12Target {
    HANDLE handle = nullptr;
    ComPtr<ID3D11Texture2D> texture;
    ComPtr<IDXGIKeyedMutex> mutex;
    uint32_t width = 0;
    uint32_t height = 0;
};

enum class SharedNv12CopyResult {
    Fatal,
    Dropped,
    Copied,
};

struct AmfBridge {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<ID3D11Texture2D> texture;
    AmfEncoder encoder;
    std::vector<uint8_t> encoded;
    bool keyframe = false;
};

struct AmfDecoderBridge {
    ~AmfDecoderBridge() {
        for (auto& target : shared_nv12_targets) {
            if (target.handle) {
                CloseHandle(target.handle);
                target.handle = nullptr;
            }
        }
    }

    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11Device1> device1;
    ComPtr<ID3D11DeviceContext> context;
    AmfDecoder decoder;
    HANDLE y_handle = nullptr;
    HANDLE uv_handle = nullptr;
    ComPtr<ID3D11Texture2D> y_texture;
    ComPtr<ID3D11Texture2D> uv_texture;
    uint64_t shared_nv12_copy_count = 0;
    SharedNv12Target shared_nv12_targets[SHARED_NV12_TARGET_COUNT];
    size_t shared_nv12_target_index = 0;
    uint8_t* nv12 = nullptr;
    uintptr_t nv12_len = 0;
    bool decoded = false;
};

VideoCodecId codec_from_u8(uint8_t codec) {
    switch (codec) {
    case 1: return VideoCodecId::AV1;
    case 2: return VideoCodecId::H265;
    case 3: return VideoCodecId::H264;
    default: return VideoCodecId::H264;
    }
}

template <typename Bridge>
bool create_device_on_adapter(IDXGIAdapter1* adapter, Bridge& bridge) {
    UINT flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
#if defined(_DEBUG)
    flags |= D3D11_CREATE_DEVICE_DEBUG;
#endif
    D3D_FEATURE_LEVEL levels[] = {
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
    };
    D3D_FEATURE_LEVEL selected{};
    HRESULT hr = D3D11CreateDevice(
        adapter,
        D3D_DRIVER_TYPE_UNKNOWN,
        nullptr,
        flags,
        levels,
        static_cast<UINT>(sizeof(levels) / sizeof(levels[0])),
        D3D11_SDK_VERSION,
        &bridge.device,
        &selected,
        &bridge.context);
    if (FAILED(hr) && (flags & D3D11_CREATE_DEVICE_DEBUG)) {
        flags &= ~D3D11_CREATE_DEVICE_DEBUG;
        hr = D3D11CreateDevice(
            adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            nullptr,
            flags,
            levels,
            static_cast<UINT>(sizeof(levels) / sizeof(levels[0])),
            D3D11_SDK_VERSION,
            &bridge.device,
            &selected,
            &bridge.context);
    }
    return SUCCEEDED(hr);
}

template <typename Bridge>
bool create_amd_device(Bridge& bridge) {
    ComPtr<IDXGIFactory1> factory;
    HRESULT hr = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
    if (FAILED(hr)) {
        native_log_error("AMF bridge failed to create DXGI factory: {}", static_cast<int>(hr));
        return false;
    }

    for (UINT index = 0;; ++index) {
        ComPtr<IDXGIAdapter1> adapter;
        hr = factory->EnumAdapters1(index, &adapter);
        if (hr == DXGI_ERROR_NOT_FOUND) {
            break;
        }
        if (FAILED(hr)) {
            continue;
        }

        DXGI_ADAPTER_DESC1 desc{};
        if (FAILED(adapter->GetDesc1(&desc)) || desc.VendorId != AMD_VENDOR_ID) {
            continue;
        }
        if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) {
            continue;
        }

        if (create_device_on_adapter(adapter.Get(), bridge)) {
            native_log_info("AMF bridge selected AMD adapter: index={} vendor_id={} device_id={}",
                index, desc.VendorId, desc.DeviceId);
            return true;
        }
    }

    native_log_error("AMF bridge did not find a usable AMD D3D11 adapter");
    return false;
}

bool create_texture(AmfBridge& bridge, uint16_t width, uint16_t height) {
    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width;
    desc.Height = height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    return SUCCEEDED(bridge.device->CreateTexture2D(&desc, nullptr, &bridge.texture));
}

bool valid_nv12_target(const DecodedFrame& frame, uint8_t* nv12, uintptr_t nv12_len) {
    const uint32_t width = frame.width;
    const uint32_t height = frame.height;
    return nv12 && frame.y_plane && frame.u_plane && width > 0 && height > 0 && (width & 1) == 0 && (height & 1) == 0 &&
        nv12_len >= static_cast<uintptr_t>(width) * height + static_cast<uintptr_t>(width) * height / 2;
}

bool copy_nv12(const DecodedFrame& frame, uint8_t* nv12, uintptr_t nv12_len) {
    parties_rs::video::NativeProfileSpan span("native.amf.decode.copy_nv12");
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

void on_decoded(AmfDecoderBridge* bridge, const DecodedFrame& frame) {
    if (!bridge) {
        return;
    }
    bridge->decoded = copy_nv12(frame, bridge->nv12, bridge->nv12_len);
}

bool ensure_shared_texture(
    AmfDecoderBridge* bridge,
    HANDLE handle,
    HANDLE& cached_handle,
    ComPtr<ID3D11Texture2D>& texture) {
    if (!bridge || !bridge->device1 || !handle) {
        return false;
    }
    if (texture && cached_handle == handle) {
        return true;
    }

    texture.Reset();
    cached_handle = nullptr;
    HRESULT hr = bridge->device1->OpenSharedResource1(handle, IID_PPV_ARGS(&texture));
    if (FAILED(hr) || !texture) {
        native_log_error("AMF decoder failed to open shared DX12 texture in D3D11: {}", static_cast<int>(hr));
        return false;
    }
    cached_handle = handle;
    return true;
}

bool copy_decoded_surface_to_shared_textures(
    AmfDecoderBridge* bridge,
    amf::AMFSurface* surface,
    HANDLE y_handle,
    HANDLE uv_handle,
    uint32_t width,
    uint32_t height) {
    parties_rs::video::NativeProfileSpan span("native.amf.decode.dx12_surface_copy");
    if (!bridge || !bridge->context || !surface || !y_handle || !uv_handle || width == 0 || height == 0) {
        return false;
    }
    if (surface->GetMemoryType() != amf::AMF_MEMORY_DX11) {
        return false;
    }
    if (!ensure_shared_texture(bridge, y_handle, bridge->y_handle, bridge->y_texture) ||
        !ensure_shared_texture(bridge, uv_handle, bridge->uv_handle, bridge->uv_texture)) {
        return false;
    }

    amf::AMFPlane* plane = surface->GetPlaneAt(0);
    if (!plane || !plane->GetNative()) {
        return false;
    }
    auto* source = static_cast<ID3D11Texture2D*>(plane->GetNative());

    D3D11_TEXTURE2D_DESC source_desc{};
    source->GetDesc(&source_desc);
    if (source_desc.Format != DXGI_FORMAT_NV12 || source_desc.Width != width || source_desc.Height != height) {
        native_log_error(
            "AMF decoder DX12 copy rejected source format/size: format={} size={}x{} expected={}x{}",
            static_cast<int>(source_desc.Format),
            source_desc.Width,
            source_desc.Height,
            width,
            height);
        return false;
    }

    D3D11_TEXTURE2D_DESC y_desc{};
    D3D11_TEXTURE2D_DESC uv_desc{};
    bridge->y_texture->GetDesc(&y_desc);
    bridge->uv_texture->GetDesc(&uv_desc);
    if (y_desc.Format != DXGI_FORMAT_R8_UNORM || y_desc.Width != width || y_desc.Height != height ||
        uv_desc.Format != DXGI_FORMAT_R8G8_UNORM || uv_desc.Width != width / 2 || uv_desc.Height != height / 2) {
        native_log_error(
            "AMF decoder DX12 copy rejected target textures: y_format={} y={}x{} uv_format={} uv={}x{} expected={}x{}",
            static_cast<int>(y_desc.Format),
            y_desc.Width,
            y_desc.Height,
            static_cast<int>(uv_desc.Format),
            uv_desc.Width,
            uv_desc.Height,
            width,
            height);
        return false;
    }

    {
        parties_rs::video::NativeProfileSpan y_span("native.amf.decode.dx12_copy_y");
        bridge->context->CopySubresourceRegion(bridge->y_texture.Get(), 0, 0, 0, 0, source, 0, nullptr);
    }
    {
        parties_rs::video::NativeProfileSpan uv_span("native.amf.decode.dx12_copy_uv");
        bridge->context->CopySubresourceRegion(bridge->uv_texture.Get(), 0, 0, 0, 0, source, 1, nullptr);
    }
    bridge->context->Flush();
    bridge->decoded = true;
    return true;
}

bool ensure_shared_nv12_target(AmfDecoderBridge* bridge, SharedNv12Target& target, size_t slot, uint32_t width, uint32_t height) {
    if (!bridge || !bridge->device || width == 0 || height == 0) {
        return false;
    }
    if (target.texture && target.width == width && target.height == height && target.handle) {
        return true;
    }

    target.texture.Reset();
    target.mutex.Reset();
    target.width = 0;
    target.height = 0;
    if (target.handle) {
        CloseHandle(target.handle);
        target.handle = nullptr;
    }

    D3D11_TEXTURE2D_DESC desc{};
    desc.Width = width;
    desc.Height = height;
    desc.MipLevels = 1;
    desc.ArraySize = 1;
    desc.Format = DXGI_FORMAT_NV12;
    desc.SampleDesc.Count = 1;
    desc.Usage = D3D11_USAGE_DEFAULT;
    desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    desc.MiscFlags = D3D11_RESOURCE_MISC_SHARED_NTHANDLE | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX;

    HRESULT hr = bridge->device->CreateTexture2D(&desc, nullptr, &target.texture);
    if (FAILED(hr) || !target.texture) {
        native_log_error("AMF decoder failed to create shared NV12 D3D11 texture: {}", static_cast<int>(hr));
        return false;
    }

    hr = target.texture.As(&target.mutex);
    if (FAILED(hr) || !target.mutex) {
        native_log_error("AMF decoder failed to query shared NV12 keyed mutex: {}", static_cast<int>(hr));
        target.texture.Reset();
        return false;
    }

    ComPtr<IDXGIResource1> resource;
    hr = target.texture.As(&resource);
    if (FAILED(hr) || !resource) {
        native_log_error("AMF decoder failed to query shared NV12 IDXGIResource1: {}", static_cast<int>(hr));
        target.texture.Reset();
        target.mutex.Reset();
        return false;
    }

    hr = resource->CreateSharedHandle(
        nullptr,
        DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE,
        nullptr,
        &target.handle);
    if (FAILED(hr) || !target.handle) {
        native_log_error("AMF decoder failed to create shared NV12 handle: {}", static_cast<int>(hr));
        target.texture.Reset();
        target.mutex.Reset();
        target.handle = nullptr;
        return false;
    }

    target.width = width;
    target.height = height;
    native_log_info(
        "AMF decoder shared NV12 ring target ready: slot={} handle={} size={}x{}",
        slot,
        reinterpret_cast<uintptr_t>(target.handle),
        width,
        height);
    return true;
}

void reset_shared_nv12_target(SharedNv12Target& target) {
    target.texture.Reset();
    target.mutex.Reset();
    target.width = 0;
    target.height = 0;
    if (target.handle) {
        CloseHandle(target.handle);
        target.handle = nullptr;
    }
}

SharedNv12CopyResult acquire_shared_nv12_target(SharedNv12Target& target, size_t slot) {
    HRESULT hr = target.mutex->AcquireSync(0, 0);
    if (hr == S_OK) {
        return SharedNv12CopyResult::Copied;
    }

    hr = target.mutex->AcquireSync(0, SHARED_NV12_MUTEX_TIMEOUT_MS);
    if (hr == S_OK) {
        return SharedNv12CopyResult::Copied;
    }

    return SharedNv12CopyResult::Dropped;
}

SharedNv12CopyResult copy_decoded_surface_to_shared_nv12(
    AmfDecoderBridge* bridge,
    amf::AMFSurface* surface,
    uint32_t width,
    uint32_t height,
    HANDLE* shared_handle_out) {
    parties_rs::video::NativeProfileSpan span("native.amf.decode.shared_nv12_copy");
    if (!bridge || !bridge->context || !surface || !shared_handle_out || width == 0 || height == 0) {
        return SharedNv12CopyResult::Fatal;
    }
    if (surface->GetMemoryType() != amf::AMF_MEMORY_DX11) {
        return SharedNv12CopyResult::Fatal;
    }

    amf::AMFPlane* plane = surface->GetPlaneAt(0);
    if (!plane || !plane->GetNative()) {
        return SharedNv12CopyResult::Fatal;
    }
    auto* source = static_cast<ID3D11Texture2D*>(plane->GetNative());

    D3D11_TEXTURE2D_DESC source_desc{};
    source->GetDesc(&source_desc);
    if (source_desc.Format != DXGI_FORMAT_NV12 || source_desc.Width != width || source_desc.Height != height) {
        native_log_error(
            "AMF decoder shared NV12 copy rejected source format/size: format={} size={}x{} expected={}x{}",
            static_cast<int>(source_desc.Format),
            source_desc.Width,
            source_desc.Height,
            width,
            height);
        return SharedNv12CopyResult::Fatal;
    }

    const size_t slot = bridge->shared_nv12_target_index % SHARED_NV12_TARGET_COUNT;
    bridge->shared_nv12_target_index += 1;
    SharedNv12Target& target = bridge->shared_nv12_targets[slot];
    if (!ensure_shared_nv12_target(bridge, target, slot, width, height)) {
        return SharedNv12CopyResult::Fatal;
    }

    const SharedNv12CopyResult acquire_result = acquire_shared_nv12_target(target, slot);
    if (acquire_result == SharedNv12CopyResult::Dropped) {
        reset_shared_nv12_target(target);
        if (!ensure_shared_nv12_target(bridge, target, slot, width, height)) {
            return SharedNv12CopyResult::Fatal;
        }
    } else if (acquire_result != SharedNv12CopyResult::Copied) {
        return SharedNv12CopyResult::Fatal;
    }

    if (acquire_result == SharedNv12CopyResult::Dropped) {
        const SharedNv12CopyResult retry_acquire_result = acquire_shared_nv12_target(target, slot);
        if (retry_acquire_result != SharedNv12CopyResult::Copied) {
            native_log_warn("AMF decoder dropped shared NV12 frame: fresh keyed mutex unavailable slot={}", slot);
            return retry_acquire_result;
        }
    }

    {
        parties_rs::video::NativeProfileSpan copy_span("native.amf.decode.shared_nv12_copy_resource");
        bridge->context->CopyResource(target.texture.Get(), source);
    }
    bridge->context->Flush();
    HRESULT hr = target.mutex->ReleaseSync(1);
    if (FAILED(hr)) {
        native_log_warn(
            "AMF decoder dropped shared NV12 frame: failed to release keyed mutex slot={} result={}",
            slot,
            static_cast<int>(hr));
        reset_shared_nv12_target(target);
        return SharedNv12CopyResult::Dropped;
    }

    *shared_handle_out = target.handle;
    bridge->decoded = true;
    bridge->shared_nv12_copy_count += 1;
    if (bridge->shared_nv12_copy_count == 1 || bridge->shared_nv12_copy_count % 120 == 0) {
        native_log_info(
            "AMF decoder copied shared NV12 frame #{}: slot={} handle={} size={}x{}",
            bridge->shared_nv12_copy_count,
            slot,
            reinterpret_cast<uintptr_t>(target.handle),
            width,
            height);
    }
    return SharedNv12CopyResult::Copied;
}

} // namespace

extern "C" {

AmfBridge* parties_amf_create(uint8_t codec, uint16_t width, uint16_t height, uint32_t fps, uint32_t bitrate) {
    native_log_info("AMF bridge create requested: codec={} size={}x{} fps={} bitrate={}", codec, width, height, fps, bitrate);
    if (width == 0 || height == 0 || fps == 0 || bitrate == 0) {
        native_log_error("AMF bridge create rejected invalid arguments");
        return nullptr;
    }

    auto bridge = std::make_unique<AmfBridge>();
    if (!create_amd_device(*bridge) || !create_texture(*bridge, width, height)) {
        native_log_error("AMF bridge failed to create D3D11 device or texture");
        return nullptr;
    }

    const VideoCodecId requested_codec = codec_from_u8(codec);
    if (!bridge->encoder.init(bridge->device.Get(), width, height, fps, bitrate, requested_codec)) {
        native_log_error("AMF bridge encoder init failed");
        return nullptr;
    }
    if (bridge->encoder.info().codec != requested_codec) {
        native_log_error("AMF bridge selected unexpected codec: requested={} actual={}",
            static_cast<int>(requested_codec), static_cast<int>(bridge->encoder.info().codec));
        return nullptr;
    }
    bridge->encoder.force_keyframe();

    bridge->encoder.on_encoded = [ptr = bridge.get()](const uint8_t* data, size_t len, bool keyframe) {
        parties_rs::video::NativeProfileSpan span("native.amf.encode.copy_encoded");
        ptr->encoded.assign(data, data + len);
        ptr->keyframe = keyframe;
    };

    native_log_info("AMF bridge ready: codec={} size={}x{} fps={} bitrate={}", codec, width, height, fps, bitrate);
    return bridge.release();
}

void parties_amf_destroy(AmfBridge* bridge) {
    delete bridge;
}

void parties_amf_force_keyframe(AmfBridge* bridge) {
    if (bridge) {
        bridge->encoder.force_keyframe();
    }
}

int parties_amf_encode_bgra(AmfBridge* bridge, const uint8_t* bgra, uintptr_t bgra_len, int64_t timestamp) {
    if (!bridge || !bgra || bgra_len == 0 || !bridge->context || !bridge->texture) {
        native_log_error("AMF bridge encode rejected invalid input");
        return -1;
    }

    D3D11_TEXTURE2D_DESC desc{};
    bridge->texture->GetDesc(&desc);
    const uintptr_t required = static_cast<uintptr_t>(desc.Width) * desc.Height * 4;
    if (bgra_len < required) {
        native_log_error("AMF bridge encode BGRA buffer too small: len={} required={}", bgra_len, required);
        return -1;
    }

    bridge->encoded.clear();
    bridge->keyframe = false;
    bridge->context->UpdateSubresource(bridge->texture.Get(), 0, nullptr, bgra, desc.Width * 4, 0);
    if (!bridge->encoder.encode(bridge->texture.Get(), timestamp)) {
        native_log_error("AMF bridge encoder rejected frame");
        return -1;
    }
    return bridge->encoded.empty() ? 0 : 1;
}

const uint8_t* parties_amf_encoded_ptr(AmfBridge* bridge) {
    if (!bridge || bridge->encoded.empty()) {
        return nullptr;
    }
    return bridge->encoded.data();
}

uintptr_t parties_amf_encoded_len(AmfBridge* bridge) {
    if (!bridge) {
        return 0;
    }
    return bridge->encoded.size();
}

int parties_amf_encoded_keyframe(AmfBridge* bridge) {
    if (!bridge) {
        return 0;
    }
    return bridge->keyframe ? 1 : 0;
}

uint8_t parties_amf_codec(AmfBridge* bridge) {
    if (!bridge) {
        return 0;
    }
    return static_cast<uint8_t>(bridge->encoder.info().codec);
}

AmfDecoderBridge* parties_amf_decoder_create(uint8_t codec, uint16_t width, uint16_t height) {
    native_log_info("AMF decoder bridge create requested: codec={} size={}x{}", codec, width, height);
    if (width == 0 || height == 0) {
        native_log_error("AMF decoder bridge create rejected invalid arguments");
        return nullptr;
    }

    auto bridge = std::make_unique<AmfDecoderBridge>();
    if (!create_amd_device(*bridge)) {
        native_log_error("AMF decoder bridge failed to create AMD D3D11 device");
        return nullptr;
    }
    bridge->device.As(&bridge->device1);
    bridge->device->GetImmediateContext(&bridge->context);
    bridge->decoder.on_decoded = [ptr = bridge.get()](const DecodedFrame& frame) { on_decoded(ptr, frame); };

    const VideoCodecId requested_codec = codec_from_u8(codec);
    if (!bridge->decoder.init(bridge->device.Get(), requested_codec, width, height)) {
        native_log_error("AMF decoder bridge init failed");
        return nullptr;
    }

    native_log_info("AMF decoder bridge ready: codec={} size={}x{}", codec, width, height);
    return bridge.release();
}

void parties_amf_decoder_destroy(AmfDecoderBridge* bridge) {
    delete bridge;
}

int parties_amf_decode(
    AmfDecoderBridge* bridge,
    const uint8_t* data,
    uintptr_t len,
    int64_t timestamp,
    uint8_t* nv12,
    uintptr_t nv12_len) {
    if (!bridge || !data || len == 0) {
        native_log_error("AMF decoder bridge decode rejected invalid input");
        return -1;
    }

    const bool output_enabled = nv12 && nv12_len > 0;
    bridge->nv12 = output_enabled ? nv12 : nullptr;
    bridge->nv12_len = output_enabled ? nv12_len : 0;
    bridge->decoded = false;
    if (output_enabled) {
        bridge->decoder.on_decoded = [bridge](const DecodedFrame& frame) { on_decoded(bridge, frame); };
    } else {
        bridge->decoder.on_decoded = nullptr;
    }
    const bool ok = bridge->decoder.decode(data, static_cast<size_t>(len), timestamp);
    bridge->nv12 = nullptr;
    bridge->nv12_len = 0;
    if (!ok) {
        native_log_error("AMF decoder bridge rejected frame");
        return -1;
    }
    return bridge->decoded ? 1 : 0;
}

int parties_amf_decode_to_d3d12(
    AmfDecoderBridge* bridge,
    const uint8_t* data,
    uintptr_t len,
    int64_t timestamp,
    void* y_handle,
    uintptr_t,
    void* uv_handle,
    uintptr_t,
    uint16_t width,
    uint16_t height) {
    if (!bridge || !data || len == 0 || !y_handle || !uv_handle || width == 0 || height == 0) {
        native_log_error("AMF decoder bridge DX12 decode rejected invalid input");
        return -1;
    }

    bridge->decoded = false;
    bridge->nv12 = nullptr;
    bridge->nv12_len = 0;
    bridge->decoder.on_decoded = nullptr;
    bridge->decoder.on_decoded_surface =
        [bridge, y_handle, uv_handle, width, height](amf::AMFSurface* surface) {
            return copy_decoded_surface_to_shared_textures(
                bridge,
                surface,
                static_cast<HANDLE>(y_handle),
                static_cast<HANDLE>(uv_handle),
                width,
                height);
        };
    const bool ok = bridge->decoder.decode(data, static_cast<size_t>(len), timestamp);
    bridge->decoder.on_decoded_surface = nullptr;
    if (!ok) {
        native_log_error("AMF decoder bridge rejected DX12 frame");
        return -1;
    }
    return bridge->decoded ? 1 : 0;
}

int parties_amf_decode_to_shared_nv12(
    AmfDecoderBridge* bridge,
    const uint8_t* data,
    uintptr_t len,
    int64_t timestamp,
    uint16_t width,
    uint16_t height,
    uintptr_t* shared_handle_out) {
    if (!bridge || !data || len == 0 || width == 0 || height == 0 || !shared_handle_out) {
        native_log_error("AMF decoder bridge shared NV12 decode rejected invalid input");
        return -1;
    }

    *shared_handle_out = 0;
    bridge->decoded = false;
    bridge->nv12 = nullptr;
    bridge->nv12_len = 0;
    bridge->decoder.on_decoded = nullptr;
    bridge->decoder.on_decoded_surface =
        [bridge, width, height, shared_handle_out](amf::AMFSurface* surface) {
            HANDLE handle = nullptr;
            const SharedNv12CopyResult result = copy_decoded_surface_to_shared_nv12(bridge, surface, width, height, &handle);
            if (result == SharedNv12CopyResult::Copied) {
                *shared_handle_out = reinterpret_cast<uintptr_t>(handle);
                return true;
            }
            return result == SharedNv12CopyResult::Dropped;
        };
    const bool ok = bridge->decoder.decode(data, static_cast<size_t>(len), timestamp);
    bridge->decoder.on_decoded_surface = nullptr;
    if (!ok) {
        native_log_error("AMF decoder bridge rejected shared NV12 frame");
        return -1;
    }
    return bridge->decoded ? 1 : 0;
}

}
