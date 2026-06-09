#include "nvidia/nvenc_encoder.h"

#include <d3d11.h>
#include <wrl/client.h>

#include <cstdint>
#include <cstring>
#include <memory>
#include <vector>

namespace {

using Microsoft::WRL::ComPtr;
using parties_rs::video::VideoCodecId;
using parties_rs::video::nvidia::NvencEncoder;

struct NvencBridge {
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    ComPtr<ID3D11Texture2D> texture;
    NvencEncoder encoder;
    std::vector<uint8_t> encoded;
    bool keyframe = false;
};

VideoCodecId codec_from_u8(uint8_t codec) {
    switch (codec) {
    case 1: return VideoCodecId::AV1;
    case 2: return VideoCodecId::H265;
    case 3: return VideoCodecId::H264;
    default: return VideoCodecId::H264;
    }
}

bool create_device(NvencBridge& bridge) {
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
        nullptr,
        D3D_DRIVER_TYPE_HARDWARE,
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
            nullptr,
            D3D_DRIVER_TYPE_HARDWARE,
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

bool create_texture(NvencBridge& bridge, uint16_t width, uint16_t height) {
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

} // namespace

extern "C" {

NvencBridge* parties_nvenc_create(uint8_t codec, uint16_t width, uint16_t height, uint32_t fps, uint32_t bitrate) {
    if (width == 0 || height == 0 || fps == 0 || bitrate == 0) {
        return nullptr;
    }

    auto bridge = std::make_unique<NvencBridge>();
    if (!create_device(*bridge) || !create_texture(*bridge, width, height)) {
        return nullptr;
    }

    const VideoCodecId requested_codec = codec_from_u8(codec);
    if (!bridge->encoder.init(bridge->device.Get(), width, height, fps, bitrate, requested_codec)) {
        return nullptr;
    }
    if (bridge->encoder.info().codec != requested_codec) {
        return nullptr;
    }
    bridge->encoder.force_keyframe();

    bridge->encoder.on_encoded = [ptr = bridge.get()](const uint8_t* data, size_t len, bool keyframe) {
        ptr->encoded.assign(data, data + len);
        ptr->keyframe = keyframe;
    };

    return bridge.release();
}

void parties_nvenc_destroy(NvencBridge* bridge) {
    delete bridge;
}

void parties_nvenc_force_keyframe(NvencBridge* bridge) {
    if (bridge) {
        bridge->encoder.force_keyframe();
    }
}

int parties_nvenc_encode_rgba(NvencBridge* bridge, const uint8_t* rgba, uintptr_t rgba_len, int64_t timestamp) {
    if (!bridge || !rgba || rgba_len == 0 || !bridge->context || !bridge->texture) {
        return -1;
    }

    D3D11_TEXTURE2D_DESC desc{};
    bridge->texture->GetDesc(&desc);
    const uintptr_t required = static_cast<uintptr_t>(desc.Width) * desc.Height * 4;
    if (rgba_len < required) {
        return -1;
    }

    bridge->encoded.clear();
    bridge->keyframe = false;
    bridge->context->UpdateSubresource(bridge->texture.Get(), 0, nullptr, rgba, desc.Width * 4, 0);
    if (!bridge->encoder.encode(bridge->texture.Get(), timestamp)) {
        return -1;
    }
    return bridge->encoded.empty() ? 0 : 1;
}

const uint8_t* parties_nvenc_encoded_ptr(NvencBridge* bridge) {
    if (!bridge || bridge->encoded.empty()) {
        return nullptr;
    }
    return bridge->encoded.data();
}

uintptr_t parties_nvenc_encoded_len(NvencBridge* bridge) {
    if (!bridge) {
        return 0;
    }
    return bridge->encoded.size();
}

int parties_nvenc_encoded_keyframe(NvencBridge* bridge) {
    if (!bridge) {
        return 0;
    }
    return bridge->keyframe ? 1 : 0;
}

uint8_t parties_nvenc_codec(NvencBridge* bridge) {
    if (!bridge) {
        return 0;
    }
    return static_cast<uint8_t>(bridge->encoder.info().codec);
}

}
