#include "amf_decoder.h"
#include "common/native_profile.h"

#include <d3d11.h>
#include <dxgi.h>

#include <algorithm>
#include <cstring>

namespace parties_rs::video::amd {

namespace {

constexpr uint32_t AMF_QUERY_RETRIES = 1;

const wchar_t* component_id(VideoCodecId codec) {
    switch (codec) {
    case VideoCodecId::AV1:
        return AMFVideoDecoderHW_AV1;
    case VideoCodecId::H265:
        return AMFVideoDecoderHW_H265_HEVC;
    case VideoCodecId::H264:
        return AMFVideoDecoderUVD_H264_AVC;
    default:
        return AMFVideoDecoderUVD_H264_AVC;
    }
}

bool set_i64(amf::AMFComponent* decoder, const wchar_t* property, amf_int64 value) {
    const AMF_RESULT result = decoder->SetProperty(property, value);
    if (result != AMF_OK) {
        native_log_error("AMF decoder SetProperty failed: result={}", static_cast<int>(result));
        return false;
    }
    return true;
}

bool set_bool(amf::AMFComponent* decoder, const wchar_t* property, bool value) {
    const AMF_RESULT result = decoder->SetProperty(property, value);
    if (result != AMF_OK) {
        native_log_error("AMF decoder SetProperty failed: result={}", static_cast<int>(result));
        return false;
    }
    return true;
}

bool set_size(amf::AMFComponent* decoder, const wchar_t* property, uint32_t width, uint32_t height) {
    const AMF_RESULT result = decoder->SetProperty(
        property,
        ::AMFConstructSize(static_cast<amf_int32>(width), static_cast<amf_int32>(height)));
    if (result != AMF_OK) {
        native_log_error("AMF decoder SetProperty(size) failed: result={}", static_cast<int>(result));
        return false;
    }
    return true;
}

} // namespace

AmfDecoder::AmfDecoder() = default;

AmfDecoder::~AmfDecoder() {
    if (decoder_) {
        decoder_->Drain();
        decoder_->Terminate();
        decoder_ = nullptr;
    }
    if (amf_context_) {
        amf_context_->Terminate();
        amf_context_ = nullptr;
    }
    for (auto& slot : readback_slots_) {
        slot = ReadbackSlot{};
    }
    context_.Reset();
    device_.Reset();
    if (factory_initialized_) {
        g_AMFFactory.Terminate();
        factory_initialized_ = false;
    }
    initialized_ = false;
}

bool AmfDecoder::init(ID3D11Device* device, VideoCodecId codec, uint32_t width, uint32_t height) {
    if (initialized_ || !device || width == 0 || height == 0) {
        return false;
    }

    const AMF_RESULT init_result = g_AMFFactory.Init();
    if (init_result != AMF_OK) {
        native_log_error("AMF decoder runtime init failed: result={}", static_cast<int>(init_result));
        return false;
    }
    factory_initialized_ = true;

    amf::AMFFactory* factory = g_AMFFactory.GetFactory();
    if (!factory) {
        native_log_error("AMF decoder factory unavailable after runtime init");
        return false;
    }

    AMF_RESULT result = factory->CreateContext(&amf_context_);
    if (result != AMF_OK || !amf_context_) {
        native_log_error("AMF decoder CreateContext failed: result={}", static_cast<int>(result));
        return false;
    }

    result = amf_context_->InitDX11(device, amf::AMF_DX11_0);
    if (result != AMF_OK) {
        native_log_error("AMF decoder InitDX11 failed: result={}", static_cast<int>(result));
        return false;
    }
    device_ = device;
    device_->GetImmediateContext(&context_);

    amf::AMFComponentPtr candidate;
    result = factory->CreateComponent(amf_context_, component_id(codec), &candidate);
    if (result != AMF_OK || !candidate) {
        native_log_error("AMF decoder CreateComponent failed: codec={} result={}", codec_name(codec), static_cast<int>(result));
        return false;
    }

    decoder_ = candidate;
    if (!set_bool(decoder_, AMF_VIDEO_DECODER_LOW_LATENCY, true) ||
        !set_i64(decoder_, AMF_VIDEO_DECODER_REORDER_MODE, AMF_VIDEO_DECODER_MODE_LOW_LATENCY) ||
        !set_bool(decoder_, AMF_VIDEO_DECODER_SURFACE_COPY, false) ||
        !set_bool(decoder_, AMF_VIDEO_DECODER_SURFACE_CPU, false) ||
        !set_i64(decoder_, AMF_VIDEO_DECODER_OUTPUT_FORMAT, amf::AMF_SURFACE_NV12) ||
        !set_size(decoder_, AMF_VIDEO_DECODER_ALLOC_SIZE, width, height)) {
        decoder_ = nullptr;
        return false;
    }

    result = decoder_->Init(amf::AMF_SURFACE_NV12, static_cast<amf_int32>(width), static_cast<amf_int32>(height));
    if (result != AMF_OK) {
        native_log_error("AMF decoder Init failed: codec={} result={}", codec_name(codec), static_cast<int>(result));
        decoder_ = nullptr;
        return false;
    }

    codec_ = codec;
    width_ = width;
    height_ = height;
    initialized_ = true;
    native_log_info("AMF decoder selected codec: {} ({}x{})", codec_name(codec_), width_, height_);
    return true;
}

bool AmfDecoder::decode(const uint8_t* data, size_t len, int64_t timestamp) {
    NativeProfileSpan span("native.amf.decode.total");
    if (!initialized_ || !decoder_ || !amf_context_ || !data || len == 0) {
        return false;
    }

    amf::AMFBufferPtr buffer;
    AMF_RESULT result = AMF_OK;
    {
        NativeProfileSpan alloc_span("native.amf.decode.alloc_input");
        result = amf_context_->AllocBuffer(amf::AMF_MEMORY_HOST, len, &buffer);
    }
    if (result != AMF_OK || !buffer || !buffer->GetNative()) {
        native_log_error("AMF decoder AllocBuffer failed: result={}", static_cast<int>(result));
        return false;
    }
    {
        NativeProfileSpan copy_span("native.amf.decode.copy_input");
        std::memcpy(buffer->GetNative(), data, len);
    }
    buffer->SetPts(timestamp);

    {
        NativeProfileSpan submit_span("native.amf.decode.submit_input");
        result = decoder_->SubmitInput(buffer);
    }
    if (result == AMF_INPUT_FULL) {
        bool ignored = false;
        collect_output(&ignored);
        NativeProfileSpan submit_span("native.amf.decode.submit_input_retry");
        result = decoder_->SubmitInput(buffer);
    }
    if (result != AMF_OK && result != AMF_NEED_MORE_INPUT) {
        native_log_error("AMF decoder SubmitInput failed: result={}", static_cast<int>(result));
        return false;
    }

    bool produced = false;
    return collect_output(&produced);
}

void AmfDecoder::flush() {
    if (decoder_) {
        decoder_->Drain();
    }
}

DecoderInfo AmfDecoder::info() const {
    return {VideoBackend::AMF, codec_};
}

bool AmfDecoder::collect_output(bool* produced) {
    NativeProfileSpan span("native.amf.decode.collect_output");
    if (produced) {
        *produced = false;
    }

    for (uint32_t attempt = 0; attempt < AMF_QUERY_RETRIES; ++attempt) {
        amf::AMFDataPtr data;
        AMF_RESULT result = AMF_OK;
        {
            NativeProfileSpan query_span("native.amf.decode.query_output");
            result = decoder_->QueryOutput(&data);
        }
        if (result == AMF_REPEAT || result == AMF_NEED_MORE_INPUT) {
            return true;
        }
        if (result != AMF_OK) {
            native_log_error("AMF decoder QueryOutput failed: result={}", static_cast<int>(result));
            return false;
        }
        if (!data) {
            return true;
        }

        amf::AMFSurfacePtr surface(data);
        if (!surface) {
            continue;
        }
        {
            NativeProfileSpan emit_span("native.amf.decode.emit_surface");
            if (!emit_surface(surface)) {
                return false;
            }
        }
        if (produced) {
            *produced = true;
        }
        return true;
    }
    return true;
}

bool AmfDecoder::emit_surface(amf::AMFSurface* surface) {
    if (!surface) {
        return false;
    }
    if (on_decoded_surface) {
        return on_decoded_surface(surface);
    }
    if (!on_decoded) {
        return true;
    }
    if (surface->GetMemoryType() == amf::AMF_MEMORY_DX11 && emit_dx11_surface(surface)) {
        return true;
    }
    AMF_RESULT result = AMF_OK;
    {
        NativeProfileSpan convert_span("native.amf.decode.surface_to_host");
        result = surface->Convert(amf::AMF_MEMORY_HOST);
    }
    if (result != AMF_OK) {
        native_log_error("AMF decoder surface Convert(HOST) failed: result={}", static_cast<int>(result));
        return false;
    }

    amf::AMFPlane* y = surface->GetPlane(amf::AMF_PLANE_Y);
    amf::AMFPlane* uv = surface->GetPlane(amf::AMF_PLANE_UV);
    if (!y || !uv || !y->GetNative() || !uv->GetNative()) {
        native_log_error("AMF decoder output surface missing NV12 planes");
        return false;
    }

    const uint32_t width = static_cast<uint32_t>((std::max)(0, y->GetWidth()));
    const uint32_t height = static_cast<uint32_t>((std::max)(0, y->GetHeight()));
    if (width == 0 || height == 0 || (width & 1) != 0 || (height & 1) != 0) {
        native_log_error("AMF decoder output surface has invalid size {}x{}", width, height);
        return false;
    }

    DecodedFrame frame{};
    frame.y_plane = static_cast<const uint8_t*>(y->GetNative());
    frame.u_plane = static_cast<const uint8_t*>(uv->GetNative());
    frame.v_plane = nullptr;
    frame.y_stride = static_cast<uint32_t>(y->GetHPitch());
    frame.uv_stride = static_cast<uint32_t>(uv->GetHPitch());
    frame.width = width;
    frame.height = height;
    frame.timestamp = surface->GetPts();
    frame.nv12 = true;

    if (on_decoded) {
        on_decoded(frame);
    }
    return true;
}

bool AmfDecoder::ensure_readback_texture(ReadbackSlot& slot, ID3D11Texture2D* texture) {
    if (!device_ || !texture) {
        return false;
    }

    D3D11_TEXTURE2D_DESC source_desc{};
    texture->GetDesc(&source_desc);
    if (source_desc.Width == 0 || source_desc.Height == 0 || source_desc.Format != DXGI_FORMAT_NV12) {
        return false;
    }

    if (slot.texture) {
        D3D11_TEXTURE2D_DESC current_desc{};
        slot.texture->GetDesc(&current_desc);
        if (current_desc.Width == source_desc.Width &&
            current_desc.Height == source_desc.Height &&
            current_desc.Format == source_desc.Format) {
            return true;
        }
        slot = ReadbackSlot{};
    }

    D3D11_TEXTURE2D_DESC readback_desc = source_desc;
    readback_desc.MipLevels = 1;
    readback_desc.ArraySize = 1;
    readback_desc.SampleDesc.Count = 1;
    readback_desc.SampleDesc.Quality = 0;
    readback_desc.Usage = D3D11_USAGE_STAGING;
    readback_desc.BindFlags = 0;
    readback_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    readback_desc.MiscFlags = 0;

    HRESULT hr = device_->CreateTexture2D(&readback_desc, nullptr, &slot.texture);
    if (FAILED(hr)) {
        native_log_error("AMF decoder readback texture create failed: {}", static_cast<int>(hr));
        return false;
    }
    slot.width = source_desc.Width;
    slot.height = source_desc.Height;
    return true;
}

bool AmfDecoder::emit_dx11_surface(amf::AMFSurface* surface) {
    if (!surface || !context_ || !on_decoded) {
        return false;
    }

    amf::AMFPlane* plane = surface->GetPlaneAt(0);
    if (!plane || !plane->GetNative()) {
        return false;
    }

    auto* texture = static_cast<ID3D11Texture2D*>(plane->GetNative());
    auto& write_slot = readback_slots_[readback_write_index_];
    if (!ensure_readback_texture(write_slot, texture)) {
        return false;
    }

    {
        NativeProfileSpan copy_span("native.amf.decode.dx11_copy_to_readback");
        context_->CopyResource(write_slot.texture.Get(), texture);
    }
    write_slot.timestamp = surface->GetPts();
    write_slot.pending = true;
    readback_write_index_ = (readback_write_index_ + 1) % readback_slots_.size();

    auto& read_slot = readback_slots_[readback_write_index_];
    if (!read_slot.pending) {
        return true;
    }

    return map_readback_slot(read_slot);
}

bool AmfDecoder::map_readback_slot(ReadbackSlot& slot) {
    if (!context_ || !slot.texture || !slot.pending) {
        return true;
    }

    D3D11_MAPPED_SUBRESOURCE mapped{};
    HRESULT hr = S_OK;
    {
        NativeProfileSpan map_span("native.amf.decode.dx11_map_readback");
        hr = context_->Map(slot.texture.Get(), 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT, &mapped);
    }
    if (hr == DXGI_ERROR_WAS_STILL_DRAWING) {
        return true;
    }
    if (FAILED(hr) || !mapped.pData || mapped.RowPitch == 0) {
        native_log_error("AMF decoder readback texture map failed: {}", static_cast<int>(hr));
        slot.pending = false;
        return false;
    }

    const uint32_t width = slot.width;
    const uint32_t height = slot.height;
    if (width == 0 || height == 0 || (width & 1) != 0 || (height & 1) != 0) {
        context_->Unmap(slot.texture.Get(), 0);
        slot.pending = false;
        native_log_error("AMF decoder DX11 output surface has invalid size {}x{}", width, height);
        return false;
    }

    auto* bytes = static_cast<const uint8_t*>(mapped.pData);
    DecodedFrame frame{};
    frame.y_plane = bytes;
    frame.u_plane = bytes + static_cast<size_t>(mapped.RowPitch) * height;
    frame.v_plane = nullptr;
    frame.y_stride = mapped.RowPitch;
    frame.uv_stride = mapped.RowPitch;
    frame.width = width;
    frame.height = height;
    frame.timestamp = slot.timestamp;
    frame.nv12 = true;

    on_decoded(frame);
    context_->Unmap(slot.texture.Get(), 0);
    slot.pending = false;
    return true;
}

} // namespace parties_rs::video::amd
