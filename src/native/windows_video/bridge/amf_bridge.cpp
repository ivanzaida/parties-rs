#include "amd/amf_decoder.h"
#include "amd/amf_encoder.h"
#include "common/native_profile.h"
#include "common/video_types.h"

#include <d3d11.h>
#include <d3d11_1.h>
#include <d3d11_3.h>
#include <d3dcompiler.h>
#include <dxgi1_2.h>
#include <wrl/client.h>

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <memory>
#include <string>
#include <type_traits>
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

struct SharedNv12PlanesTarget {
    HANDLE y_handle = nullptr;
    HANDLE uv_handle = nullptr;
    ComPtr<ID3D11Texture2D> y_texture;
    ComPtr<ID3D11Texture2D> uv_texture;
    ComPtr<IDXGIKeyedMutex> y_mutex;
    ComPtr<IDXGIKeyedMutex> uv_mutex;
    uint32_t width = 0;
    uint32_t height = 0;
};

struct PlaneBlitConstants {
    float uv_scale[2];
    float padding[2];
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
        reset_shared_targets();
    }

    void reset_shared_targets() {
        for (auto& target : shared_nv12_targets) {
            if (target.handle) {
                CloseHandle(target.handle);
                target.handle = nullptr;
            }
            target.texture.Reset();
            target.mutex.Reset();
            target.width = 0;
            target.height = 0;
        }
        for (auto& target : shared_nv12_planes_targets) {
            if (target.y_handle) {
                CloseHandle(target.y_handle);
                target.y_handle = nullptr;
            }
            if (target.uv_handle) {
                CloseHandle(target.uv_handle);
                target.uv_handle = nullptr;
            }
            target.y_texture.Reset();
            target.uv_texture.Reset();
            target.y_mutex.Reset();
            target.uv_mutex.Reset();
            target.width = 0;
            target.height = 0;
        }
        y_texture.Reset();
        uv_texture.Reset();
        y_handle = nullptr;
        uv_handle = nullptr;
        shared_nv12_target_index = 0;
        shared_nv12_planes_target_index = 0;
    }

    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11Device1> device1;
    ComPtr<ID3D11Device3> device3;
    ComPtr<ID3D11DeviceContext> context;
    AmfDecoder decoder;
    HANDLE y_handle = nullptr;
    HANDLE uv_handle = nullptr;
    ComPtr<ID3D11Texture2D> y_texture;
    ComPtr<ID3D11Texture2D> uv_texture;
    ComPtr<ID3D11VertexShader> plane_blit_vs;
    ComPtr<ID3D11PixelShader> plane_blit_y_ps;
    ComPtr<ID3D11PixelShader> plane_blit_uv_ps;
    ComPtr<ID3D11SamplerState> plane_blit_sampler;
    ComPtr<ID3D11Buffer> plane_blit_constants;
    bool plane_blit_srv_mode_logged = false;
    uint64_t shared_nv12_copy_count = 0;
    SharedNv12Target shared_nv12_targets[SHARED_NV12_TARGET_COUNT];
    size_t shared_nv12_target_index = 0;
    uint64_t shared_nv12_planes_copy_count = 0;
    SharedNv12PlanesTarget shared_nv12_planes_targets[SHARED_NV12_TARGET_COUNT];
    size_t shared_nv12_planes_target_index = 0;
    uint8_t* nv12 = nullptr;
    uintptr_t nv12_len = 0;
    bool decoded = false;
    VideoCodecId codec = VideoCodecId::H264;
    uint32_t width = 0;
    uint32_t height = 0;
    LUID adapter_luid{};
    bool adapter_luid_valid = false;
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

bool get_adapter_luid_from_device(ID3D11Device* device, LUID& luid) {
    if (!device) {
        return false;
    }
    ComPtr<IDXGIDevice> dxgi_device;
    HRESULT hr = device->QueryInterface(IID_PPV_ARGS(&dxgi_device));
    if (FAILED(hr) || !dxgi_device) {
        return false;
    }
    ComPtr<IDXGIAdapter> adapter;
    hr = dxgi_device->GetAdapter(&adapter);
    if (FAILED(hr) || !adapter) {
        return false;
    }
    DXGI_ADAPTER_DESC desc{};
    hr = adapter->GetDesc(&desc);
    if (FAILED(hr)) {
        return false;
    }
    luid = desc.AdapterLuid;
    return true;
}

bool same_luid(const LUID& left, const LUID& right) {
    return left.LowPart == right.LowPart && left.HighPart == right.HighPart;
}

std::string luid_text(const LUID& luid) {
    char buffer[64]{};
    std::snprintf(buffer, sizeof(buffer), "%08x:%08x", static_cast<unsigned>(luid.HighPart), luid.LowPart);
    return buffer;
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
            if constexpr (std::is_same_v<Bridge, AmfDecoderBridge>) {
                bridge.adapter_luid = desc.AdapterLuid;
                bridge.adapter_luid_valid = true;
            }
            return true;
        }
    }

    native_log_error("AMF bridge did not find a usable AMD D3D11 adapter");
    return false;
}

bool create_amd_device_on_luid(AmfDecoderBridge& bridge, const LUID& target_luid) {
    ComPtr<IDXGIFactory1> factory;
    HRESULT hr = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
    if (FAILED(hr)) {
        native_log_error("AMF bridge failed to create DXGI factory for LUID match: {}", static_cast<int>(hr));
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
        if (FAILED(adapter->GetDesc1(&desc)) || desc.VendorId != AMD_VENDOR_ID || (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) {
            continue;
        }
        if (!same_luid(desc.AdapterLuid, target_luid)) {
            continue;
        }

        if (create_device_on_adapter(adapter.Get(), bridge)) {
            bridge.adapter_luid = desc.AdapterLuid;
            bridge.adapter_luid_valid = true;
            native_log_info(
                "AMF bridge selected AMD adapter by renderer LUID: index={} vendor_id={} device_id={} luid={}",
                index,
                desc.VendorId,
                desc.DeviceId,
                luid_text(desc.AdapterLuid));
            return true;
        }
    }

    native_log_error("AMF bridge did not find AMD D3D11 adapter matching renderer LUID {}", luid_text(target_luid));
    return false;
}

bool ensure_decoder_adapter_for_d3d12_target(AmfDecoderBridge* bridge, const LUID& target_luid) {
    if (!bridge || bridge->width == 0 || bridge->height == 0) {
        return false;
    }

    LUID current_luid{};
    const bool has_current_luid = get_adapter_luid_from_device(bridge->device.Get(), current_luid);
    if (has_current_luid && same_luid(current_luid, target_luid)) {
        return true;
    }

    native_log_warn(
        "AMF decoder D3D12 target adapter differs from current AMF device: current_luid={} target_luid={}; recreating decoder",
        has_current_luid ? luid_text(current_luid) : "unknown",
        luid_text(target_luid));

    bridge->decoder.shutdown();
    bridge->reset_shared_targets();
    bridge->device.Reset();
    bridge->device1.Reset();
    bridge->device3.Reset();
    bridge->context.Reset();
    bridge->plane_blit_vs.Reset();
    bridge->plane_blit_y_ps.Reset();
    bridge->plane_blit_uv_ps.Reset();
    bridge->plane_blit_sampler.Reset();
    bridge->plane_blit_constants.Reset();
    bridge->plane_blit_srv_mode_logged = false;
    bridge->adapter_luid_valid = false;

    if (!create_amd_device_on_luid(*bridge, target_luid) ||
        FAILED(bridge->device.As(&bridge->device1)) ||
        FAILED(bridge->device.As(&bridge->device3)) ||
        !bridge->decoder.init(bridge->device.Get(), bridge->codec, bridge->width, bridge->height)) {
        native_log_error("AMF decoder failed to recreate on renderer adapter LUID {}", luid_text(target_luid));
        return false;
    }

    return true;
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

bool compile_shader(const char* source, const char* entry, const char* target, ComPtr<ID3DBlob>& blob) {
    UINT flags = 0;
#if defined(_DEBUG)
    flags = D3DCOMPILE_DEBUG | D3DCOMPILE_SKIP_OPTIMIZATION;
#endif
    ComPtr<ID3DBlob> errors;
    HRESULT hr = D3DCompile(
        source,
        std::strlen(source),
        nullptr,
        nullptr,
        nullptr,
        entry,
        target,
        flags,
        0,
        &blob,
        &errors);
    if (FAILED(hr)) {
        native_log_error("AMF decoder plane blit shader compile failed: entry={} result={}", entry, static_cast<int>(hr));
        return false;
    }
    return true;
}

bool ensure_plane_blit_resources(AmfDecoderBridge* bridge) {
    if (!bridge || !bridge->device) {
        return false;
    }
    if (bridge->plane_blit_vs && bridge->plane_blit_y_ps && bridge->plane_blit_uv_ps && bridge->plane_blit_sampler) {
        return true;
    }

    static constexpr const char* kPlaneBlitShader = R"(
      struct VSOut {
        float4 position : SV_Position;
        float2 uv : TEXCOORD0;
      };

      VSOut vs_main(uint vertex_id : SV_VertexID) {
        float2 positions[3] = {
          float2(-1.0, -1.0),
          float2(-1.0,  3.0),
          float2( 3.0, -1.0)
        };
        float2 uvs[3] = {
          float2(0.0, 1.0),
          float2(0.0, -1.0),
          float2(2.0, 1.0)
        };
        VSOut output;
        output.position = float4(positions[vertex_id], 0.0, 1.0);
        output.uv = uvs[vertex_id];
        return output;
      }

      Texture2D plane_texture : register(t0);
      SamplerState plane_sampler : register(s0);
      cbuffer PlaneBlitConstants : register(b0) {
        float2 uv_scale;
        float2 unused_padding;
      };

      float4 ps_y_main(VSOut input) : SV_Target {
        float y = plane_texture.SampleLevel(plane_sampler, input.uv * uv_scale, 0.0).r;
        return float4(y, y, y, 1.0);
      }

      float4 ps_uv_main(VSOut input) : SV_Target {
        float2 uv = plane_texture.SampleLevel(plane_sampler, input.uv * uv_scale, 0.0).rg;
        return float4(uv.x, uv.y, 0.0, 1.0);
      }
    )";

    ComPtr<ID3DBlob> vs_blob;
    ComPtr<ID3DBlob> y_ps_blob;
    ComPtr<ID3DBlob> uv_ps_blob;
    if (!compile_shader(kPlaneBlitShader, "vs_main", "vs_5_0", vs_blob) ||
        !compile_shader(kPlaneBlitShader, "ps_y_main", "ps_5_0", y_ps_blob) ||
        !compile_shader(kPlaneBlitShader, "ps_uv_main", "ps_5_0", uv_ps_blob)) {
        return false;
    }

    HRESULT hr = bridge->device->CreateVertexShader(
        vs_blob->GetBufferPointer(), vs_blob->GetBufferSize(), nullptr, &bridge->plane_blit_vs);
    if (FAILED(hr)) {
        native_log_error("AMF decoder plane blit vertex shader create failed: {}", static_cast<int>(hr));
        return false;
    }
    hr = bridge->device->CreatePixelShader(
        y_ps_blob->GetBufferPointer(), y_ps_blob->GetBufferSize(), nullptr, &bridge->plane_blit_y_ps);
    if (FAILED(hr)) {
        native_log_error("AMF decoder plane blit Y pixel shader create failed: {}", static_cast<int>(hr));
        return false;
    }
    hr = bridge->device->CreatePixelShader(
        uv_ps_blob->GetBufferPointer(), uv_ps_blob->GetBufferSize(), nullptr, &bridge->plane_blit_uv_ps);
    if (FAILED(hr)) {
        native_log_error("AMF decoder plane blit UV pixel shader create failed: {}", static_cast<int>(hr));
        return false;
    }

    D3D11_SAMPLER_DESC sampler_desc{};
    sampler_desc.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT;
    sampler_desc.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler_desc.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler_desc.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    sampler_desc.MaxLOD = D3D11_FLOAT32_MAX;
    hr = bridge->device->CreateSamplerState(&sampler_desc, &bridge->plane_blit_sampler);
    if (FAILED(hr)) {
        native_log_error("AMF decoder plane blit sampler create failed: {}", static_cast<int>(hr));
        return false;
    }

    D3D11_BUFFER_DESC constants_desc{};
    constants_desc.ByteWidth = sizeof(PlaneBlitConstants);
    constants_desc.Usage = D3D11_USAGE_DYNAMIC;
    constants_desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
    constants_desc.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    hr = bridge->device->CreateBuffer(&constants_desc, nullptr, &bridge->plane_blit_constants);
    if (FAILED(hr)) {
        native_log_error("AMF decoder plane blit constants create failed: {}", static_cast<int>(hr));
        return false;
    }

    native_log_info("AMF decoder plane blit shaders ready");
    return true;
}

bool blit_nv12_plane_to_texture(
    AmfDecoderBridge* bridge,
    ID3D11Texture2D* source,
    ID3D11Texture2D* target,
    DXGI_FORMAT view_format,
    UINT source_plane_slice,
    ID3D11PixelShader* pixel_shader,
    uint32_t width,
    uint32_t height,
    float uv_scale_x,
    float uv_scale_y) {
    if (!bridge || !bridge->device || !bridge->context || !source || !target || !pixel_shader ||
        !bridge->plane_blit_constants || width == 0 || height == 0 || uv_scale_x <= 0.0f || uv_scale_y <= 0.0f) {
        return false;
    }

    ComPtr<ID3D11ShaderResourceView> source_view;
    HRESULT hr = E_FAIL;
    if (bridge->device3) {
        if (!bridge->plane_blit_srv_mode_logged) {
            native_log_info("AMF decoder plane blit using D3D11.3 plane-slice SRVs");
            bridge->plane_blit_srv_mode_logged = true;
        }
        ComPtr<ID3D11ShaderResourceView1> source_view1;
        D3D11_SHADER_RESOURCE_VIEW_DESC1 srv_desc{};
        srv_desc.Format = view_format;
        srv_desc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
        srv_desc.Texture2D.MostDetailedMip = 0;
        srv_desc.Texture2D.MipLevels = 1;
        srv_desc.Texture2D.PlaneSlice = source_plane_slice;
        hr = bridge->device3->CreateShaderResourceView1(source, &srv_desc, &source_view1);
        if (SUCCEEDED(hr) && source_view1) {
            source_view1.As(&source_view);
        }
    } else {
        D3D11_SHADER_RESOURCE_VIEW_DESC srv_desc{};
        srv_desc.Format = view_format;
        srv_desc.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
        srv_desc.Texture2D.MostDetailedMip = 0;
        srv_desc.Texture2D.MipLevels = 1;
        hr = bridge->device->CreateShaderResourceView(source, &srv_desc, &source_view);
        if (!bridge->plane_blit_srv_mode_logged) {
            native_log_warn("AMF decoder plane blit using legacy SRVs without explicit NV12 plane slices");
            bridge->plane_blit_srv_mode_logged = true;
        }
    }
    if (FAILED(hr) || !source_view) {
        native_log_error(
            "AMF decoder plane blit SRV create failed: format={} plane={} result={}",
            static_cast<int>(view_format),
            source_plane_slice,
            static_cast<int>(hr));
        return false;
    }

    D3D11_RENDER_TARGET_VIEW_DESC rtv_desc{};
    rtv_desc.Format = view_format;
    rtv_desc.ViewDimension = D3D11_RTV_DIMENSION_TEXTURE2D;
    rtv_desc.Texture2D.MipSlice = 0;
    ComPtr<ID3D11RenderTargetView> target_view;
    hr = bridge->device->CreateRenderTargetView(target, &rtv_desc, &target_view);
    if (FAILED(hr) || !target_view) {
        native_log_error("AMF decoder plane blit RTV create failed: format={} result={}", static_cast<int>(view_format), static_cast<int>(hr));
        return false;
    }

    D3D11_VIEWPORT viewport{};
    viewport.TopLeftX = 0.0f;
    viewport.TopLeftY = 0.0f;
    viewport.Width = static_cast<float>(width);
    viewport.Height = static_cast<float>(height);
    viewport.MinDepth = 0.0f;
    viewport.MaxDepth = 1.0f;

    D3D11_MAPPED_SUBRESOURCE mapped{};
    hr = bridge->context->Map(bridge->plane_blit_constants.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped);
    if (FAILED(hr)) {
        native_log_error("AMF decoder plane blit constants map failed: {}", static_cast<int>(hr));
        return false;
    }
    auto* constants = static_cast<PlaneBlitConstants*>(mapped.pData);
    constants->uv_scale[0] = uv_scale_x;
    constants->uv_scale[1] = uv_scale_y;
    constants->padding[0] = 0.0f;
    constants->padding[1] = 0.0f;
    bridge->context->Unmap(bridge->plane_blit_constants.Get(), 0);

    ID3D11ShaderResourceView* srv = source_view.Get();
    ID3D11SamplerState* sampler = bridge->plane_blit_sampler.Get();
    ID3D11RenderTargetView* rtv = target_view.Get();
    ID3D11Buffer* constants_buffer = bridge->plane_blit_constants.Get();
    bridge->context->IASetInputLayout(nullptr);
    bridge->context->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    bridge->context->VSSetShader(bridge->plane_blit_vs.Get(), nullptr, 0);
    bridge->context->PSSetShader(pixel_shader, nullptr, 0);
    bridge->context->PSSetShaderResources(0, 1, &srv);
    bridge->context->PSSetSamplers(0, 1, &sampler);
    bridge->context->PSSetConstantBuffers(0, 1, &constants_buffer);
    bridge->context->RSSetViewports(1, &viewport);
    bridge->context->OMSetRenderTargets(1, &rtv, nullptr);
    bridge->context->Draw(3, 0);

    ID3D11ShaderResourceView* null_srv = nullptr;
    ID3D11RenderTargetView* null_rtv = nullptr;
    ID3D11Buffer* null_cb = nullptr;
    bridge->context->PSSetShaderResources(0, 1, &null_srv);
    bridge->context->PSSetConstantBuffers(0, 1, &null_cb);
    bridge->context->OMSetRenderTargets(1, &null_rtv, nullptr);
    return true;
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
    if (source_desc.Format != DXGI_FORMAT_NV12 || source_desc.Width < width || source_desc.Height < height) {
        native_log_error(
            "AMF decoder DX12 copy rejected source format/size: format={} size={}x{} expected_at_least={}x{}",
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

    D3D11_BOX y_box{};
    y_box.left = 0;
    y_box.top = 0;
    y_box.front = 0;
    y_box.right = width;
    y_box.bottom = height;
    y_box.back = 1;

    D3D11_BOX uv_box{};
    uv_box.left = 0;
    uv_box.top = 0;
    uv_box.front = 0;
    uv_box.right = width / 2;
    uv_box.bottom = height / 2;
    uv_box.back = 1;

    {
        parties_rs::video::NativeProfileSpan y_span("native.amf.decode.dx12_copy_y");
        bridge->context->CopySubresourceRegion(bridge->y_texture.Get(), 0, 0, 0, 0, source, 0, &y_box);
    }
    {
        parties_rs::video::NativeProfileSpan uv_span("native.amf.decode.dx12_copy_uv");
        bridge->context->CopySubresourceRegion(bridge->uv_texture.Get(), 0, 0, 0, 0, source, 1, &uv_box);
    }
    bridge->context->Flush();
    bridge->decoded = true;
    return true;
}

void reset_shared_nv12_planes_target(SharedNv12PlanesTarget& target) {
    target.y_mutex.Reset();
    target.uv_mutex.Reset();
    target.y_texture.Reset();
    target.uv_texture.Reset();
    target.width = 0;
    target.height = 0;
    if (target.y_handle) {
        CloseHandle(target.y_handle);
        target.y_handle = nullptr;
    }
    if (target.uv_handle) {
        CloseHandle(target.uv_handle);
        target.uv_handle = nullptr;
    }
}

bool create_shared_texture_handle(ID3D11Texture2D* texture, HANDLE* handle_out) {
    if (!texture || !handle_out) {
        return false;
    }
    constexpr DWORD GENERIC_ALL_ACCESS = 0x10000000;
    ComPtr<IDXGIResource1> resource;
    HRESULT hr = texture->QueryInterface(IID_PPV_ARGS(&resource));
    if (FAILED(hr) || !resource) {
        native_log_error("AMF decoder failed to query shared planes IDXGIResource1: {}", static_cast<int>(hr));
        return false;
    }
    hr = resource->CreateSharedHandle(nullptr, GENERIC_ALL_ACCESS, nullptr, handle_out);
    if (FAILED(hr) || !*handle_out) {
        native_log_error("AMF decoder failed to create shared planes handle: {}", static_cast<int>(hr));
        return false;
    }
    return true;
}

bool ensure_shared_nv12_planes_target(
    AmfDecoderBridge* bridge,
    SharedNv12PlanesTarget& target,
    size_t slot,
    uint32_t width,
    uint32_t height) {
    if (!bridge || !bridge->device || width == 0 || height == 0 || (width & 1) != 0 || (height & 1) != 0) {
        return false;
    }
    if (target.y_texture && target.uv_texture && target.y_handle && target.uv_handle && target.width == width &&
        target.height == height) {
        return true;
    }

    reset_shared_nv12_planes_target(target);

    D3D11_TEXTURE2D_DESC y_desc{};
    y_desc.Width = width;
    y_desc.Height = height;
    y_desc.MipLevels = 1;
    y_desc.ArraySize = 1;
    y_desc.Format = DXGI_FORMAT_R8_UNORM;
    y_desc.SampleDesc.Count = 1;
    y_desc.Usage = D3D11_USAGE_DEFAULT;
    y_desc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET;
    y_desc.MiscFlags = D3D11_RESOURCE_MISC_SHARED_NTHANDLE | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX;

    HRESULT hr = bridge->device->CreateTexture2D(&y_desc, nullptr, &target.y_texture);
    if (FAILED(hr) || !target.y_texture) {
        native_log_error("AMF decoder failed to create shared NV12 Y texture: {}", static_cast<int>(hr));
        reset_shared_nv12_planes_target(target);
        return false;
    }
    hr = target.y_texture.As(&target.y_mutex);
    if (FAILED(hr) || !target.y_mutex) {
        native_log_error("AMF decoder failed to query shared NV12 Y keyed mutex: {}", static_cast<int>(hr));
        reset_shared_nv12_planes_target(target);
        return false;
    }

    D3D11_TEXTURE2D_DESC uv_desc = y_desc;
    uv_desc.Width = width / 2;
    uv_desc.Height = height / 2;
    uv_desc.Format = DXGI_FORMAT_R8G8_UNORM;
    hr = bridge->device->CreateTexture2D(&uv_desc, nullptr, &target.uv_texture);
    if (FAILED(hr) || !target.uv_texture) {
        native_log_error("AMF decoder failed to create shared NV12 UV texture: {}", static_cast<int>(hr));
        reset_shared_nv12_planes_target(target);
        return false;
    }
    hr = target.uv_texture.As(&target.uv_mutex);
    if (FAILED(hr) || !target.uv_mutex) {
        native_log_error("AMF decoder failed to query shared NV12 UV keyed mutex: {}", static_cast<int>(hr));
        reset_shared_nv12_planes_target(target);
        return false;
    }

    if (!create_shared_texture_handle(target.y_texture.Get(), &target.y_handle) ||
        !create_shared_texture_handle(target.uv_texture.Get(), &target.uv_handle)) {
        reset_shared_nv12_planes_target(target);
        return false;
    }

    target.width = width;
    target.height = height;
    native_log_info(
        "AMF decoder shared NV12 planes target ready: slot={} y_handle={} uv_handle={} size={}x{}",
        slot,
        reinterpret_cast<uintptr_t>(target.y_handle),
        reinterpret_cast<uintptr_t>(target.uv_handle),
        width,
        height);
    return true;
}

SharedNv12CopyResult copy_decoded_surface_to_shared_nv12_planes(
    AmfDecoderBridge* bridge,
    amf::AMFSurface* surface,
    uint32_t width,
    uint32_t height,
    HANDLE* y_handle_out,
    HANDLE* uv_handle_out) {
    parties_rs::video::NativeProfileSpan span("native.amf.decode.shared_nv12_planes_copy");
    if (!bridge || !bridge->context || !surface || !y_handle_out || !uv_handle_out || width == 0 || height == 0) {
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
    if (source_desc.Format != DXGI_FORMAT_NV12 || source_desc.Width < width || source_desc.Height < height) {
        native_log_error(
            "AMF decoder shared planes copy rejected source format/size: format={} size={}x{} expected_at_least={}x{}",
            static_cast<int>(source_desc.Format),
            source_desc.Width,
            source_desc.Height,
            width,
            height);
        return SharedNv12CopyResult::Fatal;
    }
    const float uv_scale_x = static_cast<float>(width) / static_cast<float>(source_desc.Width);
    const float uv_scale_y = static_cast<float>(height) / static_cast<float>(source_desc.Height);

    const size_t slot = bridge->shared_nv12_planes_target_index % SHARED_NV12_TARGET_COUNT;
    bridge->shared_nv12_planes_target_index += 1;
    SharedNv12PlanesTarget& target = bridge->shared_nv12_planes_targets[slot];
    if (!ensure_shared_nv12_planes_target(bridge, target, slot, width, height)) {
        return SharedNv12CopyResult::Fatal;
    }
    if (!ensure_plane_blit_resources(bridge)) {
        return SharedNv12CopyResult::Fatal;
    }

    HRESULT hr = target.y_mutex->AcquireSync(0, SHARED_NV12_MUTEX_TIMEOUT_MS);
    if (hr != S_OK) {
        native_log_warn(
            "AMF decoder dropped shared NV12 planes frame: Y producer keyed mutex unavailable slot={} result={}",
            slot,
            static_cast<int>(hr));
        return SharedNv12CopyResult::Dropped;
    }
    hr = target.uv_mutex->AcquireSync(0, SHARED_NV12_MUTEX_TIMEOUT_MS);
    if (hr != S_OK) {
        target.y_mutex->ReleaseSync(0);
        native_log_warn(
            "AMF decoder dropped shared NV12 planes frame: UV producer keyed mutex unavailable slot={} result={}",
            slot,
            static_cast<int>(hr));
        return SharedNv12CopyResult::Dropped;
    }

    {
        parties_rs::video::NativeProfileSpan y_span("native.amf.decode.shared_nv12_planes_copy_y");
        if (!blit_nv12_plane_to_texture(
                bridge,
                source,
                target.y_texture.Get(),
                DXGI_FORMAT_R8_UNORM,
                0,
                bridge->plane_blit_y_ps.Get(),
                width,
                height,
                uv_scale_x,
                uv_scale_y)) {
            target.uv_mutex->ReleaseSync(0);
            target.y_mutex->ReleaseSync(0);
            return SharedNv12CopyResult::Fatal;
        }
    }
    {
        parties_rs::video::NativeProfileSpan uv_span("native.amf.decode.shared_nv12_planes_copy_uv");
        if (!blit_nv12_plane_to_texture(
                bridge,
                source,
                target.uv_texture.Get(),
                DXGI_FORMAT_R8G8_UNORM,
                1,
                bridge->plane_blit_uv_ps.Get(),
                width / 2,
                height / 2,
                uv_scale_x,
                uv_scale_y)) {
            target.uv_mutex->ReleaseSync(0);
            target.y_mutex->ReleaseSync(0);
            return SharedNv12CopyResult::Fatal;
        }
    }
    bridge->context->Flush();
    const HRESULT uv_release = target.uv_mutex->ReleaseSync(0);
    const HRESULT y_release = target.y_mutex->ReleaseSync(0);
    if (FAILED(y_release) || FAILED(uv_release)) {
        native_log_warn(
            "AMF decoder dropped shared NV12 planes frame: failed to release producer keyed mutex slot={} y_result={} uv_result={}",
            slot,
            static_cast<int>(y_release),
            static_cast<int>(uv_release));
        return SharedNv12CopyResult::Dropped;
    }

    *y_handle_out = target.y_handle;
    *uv_handle_out = target.uv_handle;
    bridge->decoded = true;
    bridge->shared_nv12_planes_copy_count += 1;
    if (bridge->shared_nv12_planes_copy_count == 1 || bridge->shared_nv12_planes_copy_count % 120 == 0) {
        native_log_info(
            "AMF decoder copied shared NV12 planes frame #{}: slot={} y_handle={} uv_handle={} size={}x{}",
            bridge->shared_nv12_planes_copy_count,
            slot,
            reinterpret_cast<uintptr_t>(target.y_handle),
            reinterpret_cast<uintptr_t>(target.uv_handle),
            width,
            height);
    }
    return SharedNv12CopyResult::Copied;
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
    bridge->device.As(&bridge->device3);
    bridge->device->GetImmediateContext(&bridge->context);
    bridge->decoder.on_decoded = [ptr = bridge.get()](const DecodedFrame& frame) { on_decoded(ptr, frame); };

    const VideoCodecId requested_codec = codec_from_u8(codec);
    bridge->codec = requested_codec;
    bridge->width = width;
    bridge->height = height;
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
    uint32_t adapter_luid_low,
    int32_t adapter_luid_high,
    uint16_t width,
    uint16_t height) {
    if (!bridge || !data || len == 0 || !y_handle || !uv_handle || width == 0 || height == 0) {
        native_log_error("AMF decoder bridge DX12 decode rejected invalid input");
        return -1;
    }

    LUID target_luid{};
    target_luid.LowPart = adapter_luid_low;
    target_luid.HighPart = adapter_luid_high;
    if (!ensure_decoder_adapter_for_d3d12_target(bridge, target_luid)) {
        native_log_error(
            "AMF decoder bridge DX12 decode rejected renderer adapter LUID {}",
            luid_text(target_luid));
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

int parties_amf_decode_to_shared_nv12_planes(
    AmfDecoderBridge* bridge,
    const uint8_t* data,
    uintptr_t len,
    int64_t timestamp,
    uint16_t width,
    uint16_t height,
    uintptr_t* y_shared_handle_out,
    uintptr_t* uv_shared_handle_out) {
    if (!bridge || !data || len == 0 || width == 0 || height == 0 || !y_shared_handle_out || !uv_shared_handle_out) {
        native_log_error("AMF decoder bridge shared NV12 planes decode rejected invalid input");
        return -1;
    }

    *y_shared_handle_out = 0;
    *uv_shared_handle_out = 0;
    bridge->decoded = false;
    bridge->nv12 = nullptr;
    bridge->nv12_len = 0;
    bridge->decoder.on_decoded = nullptr;
    bridge->decoder.on_decoded_surface =
        [bridge, width, height, y_shared_handle_out, uv_shared_handle_out](amf::AMFSurface* surface) {
            HANDLE y_handle = nullptr;
            HANDLE uv_handle = nullptr;
            const SharedNv12CopyResult result =
                copy_decoded_surface_to_shared_nv12_planes(bridge, surface, width, height, &y_handle, &uv_handle);
            if (result == SharedNv12CopyResult::Copied) {
                *y_shared_handle_out = reinterpret_cast<uintptr_t>(y_handle);
                *uv_shared_handle_out = reinterpret_cast<uintptr_t>(uv_handle);
                return true;
            }
            return result == SharedNv12CopyResult::Dropped;
        };
    const bool ok = bridge->decoder.decode(data, static_cast<size_t>(len), timestamp);
    bridge->decoder.on_decoded_surface = nullptr;
    if (!ok) {
        native_log_error("AMF decoder bridge rejected shared NV12 planes frame");
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
