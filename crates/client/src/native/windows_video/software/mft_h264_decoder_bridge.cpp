#include <algorithm>
#include <cstdint>
#include <cstring>
#include <limits>
#include <vector>

#include <codecapi.h>
#include <mfapi.h>
#include <mferror.h>
#include <mfidl.h>
#include <mftransform.h>
#include <objbase.h>
#include <wmcodecdsp.h>
#include <wrl/client.h>

#include "video_types.h"

using Microsoft::WRL::ComPtr;
using parties_rs::video::native_log_error;
using parties_rs::video::native_log_info;
using parties_rs::video::native_log_warn;

namespace {

constexpr int kNoOutput = 0;
constexpr int kCopiedOutput = 1;
constexpr int kDrainedOutputWithoutCopy = 2;

struct PartiesMftH264Decoder {
    ComPtr<IMFTransform> transform;
    uint32_t width = 0;
    uint32_t height = 0;
    bool com_initialized = false;
    bool mf_started = false;
    std::vector<uint8_t> scratch;
};

bool ok(HRESULT hr, const char* label) {
    if (SUCCEEDED(hr)) {
        return true;
    }
    native_log_error("{} failed: {:#010x}", label, static_cast<unsigned>(hr));
    return false;
}

bool set_input_type(PartiesMftH264Decoder* decoder) {
    ComPtr<IMFMediaType> type;
    HRESULT hr = MFCreateMediaType(&type);
    if (!ok(hr, "MFCreateMediaType input")) return false;
    type->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    type->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_H264);
    MFSetAttributeSize(type.Get(), MF_MT_FRAME_SIZE, decoder->width, decoder->height);
    type->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    hr = decoder->transform->SetInputType(0, type.Get(), 0);
    return ok(hr, "MFT SetInputType H264");
}

bool set_output_type(PartiesMftH264Decoder* decoder) {
    for (DWORD index = 0;; ++index) {
        ComPtr<IMFMediaType> type;
        HRESULT hr = decoder->transform->GetOutputAvailableType(0, index, &type);
        if (hr == MF_E_NO_MORE_TYPES) {
            break;
        }
        if (FAILED(hr)) {
            continue;
        }
        GUID subtype{};
        if (FAILED(type->GetGUID(MF_MT_SUBTYPE, &subtype)) || subtype != MFVideoFormat_NV12) {
            continue;
        }
        MFSetAttributeSize(type.Get(), MF_MT_FRAME_SIZE, decoder->width, decoder->height);
        type->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
        hr = decoder->transform->SetOutputType(0, type.Get(), 0);
        if (SUCCEEDED(hr)) {
            return true;
        }
    }

    ComPtr<IMFMediaType> type;
    HRESULT hr = MFCreateMediaType(&type);
    if (!ok(hr, "MFCreateMediaType output")) return false;
    type->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video);
    type->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_NV12);
    MFSetAttributeSize(type.Get(), MF_MT_FRAME_SIZE, decoder->width, decoder->height);
    type->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive);
    hr = decoder->transform->SetOutputType(0, type.Get(), 0);
    return ok(hr, "MFT SetOutputType NV12");
}

bool set_codecapi_u32(ICodecAPI* codec_api, const GUID& key, uint32_t value, const char* label, bool warn_on_failure) {
    VARIANT variant;
    VariantInit(&variant);
    variant.vt = VT_UI4;
    variant.ulVal = value;
    const HRESULT hr = codec_api->SetValue(&key, &variant);
    if (SUCCEEDED(hr)) {
        native_log_info("MFT H264 {}={}", label, value);
        return true;
    }
    if (warn_on_failure) {
        native_log_warn("MFT H264 {} rejected: {}", label, static_cast<int>(hr));
    }
    return false;
}

void configure_low_latency_decode(PartiesMftH264Decoder* decoder) {
    bool low_latency_enabled = false;

    ComPtr<IMFAttributes> attributes;
    HRESULT hr = decoder->transform.As(&attributes);
    if (SUCCEEDED(hr) && attributes) {
        hr = attributes->SetUINT32(MF_LOW_LATENCY, TRUE);
        if (SUCCEEDED(hr)) {
            low_latency_enabled = true;
        } else {
            native_log_warn("MFT H264 MF_LOW_LATENCY rejected: {}", static_cast<int>(hr));
        }
    }

    ComPtr<ICodecAPI> codec_api;
    hr = decoder->transform.As(&codec_api);
    if (SUCCEEDED(hr) && codec_api) {
        low_latency_enabled |= set_codecapi_u32(codec_api.Get(), CODECAPI_AVLowLatencyMode, 1, "CODECAPI_AVLowLatencyMode", true);
        set_codecapi_u32(codec_api.Get(), CODECAPI_AVDecVideoDXVAMode, eAVDecVideoDXVAMode_SW, "CODECAPI_AVDecVideoDXVAMode", false);
        set_codecapi_u32(codec_api.Get(), CODECAPI_AVDecVideoAcceleration_H264, 0, "CODECAPI_AVDecVideoAcceleration_H264", false);
        set_codecapi_u32(codec_api.Get(), CODECAPI_AVDecVideoFastDecodeMode, eVideoDecodeFastest, "CODECAPI_AVDecVideoFastDecodeMode", false);
        set_codecapi_u32(codec_api.Get(), CODECAPI_AVDecDisableVideoPostProcessing, 1, "CODECAPI_AVDecDisableVideoPostProcessing", false);
        set_codecapi_u32(codec_api.Get(), CODECAPI_AVDecVideoDropPicWithMissingRef, 1, "CODECAPI_AVDecVideoDropPicWithMissingRef", false);
    }

    native_log_info("MFT H264 low-latency mode {}", low_latency_enabled ? "enabled" : "unavailable");
}

bool create_sample_from_bytes(const uint8_t* data, size_t len, int64_t timestamp, IMFTransform* transform, IMFSample** sample_out) {
    if (len > static_cast<size_t>((std::numeric_limits<DWORD>::max)())) {
        native_log_error("MFT H264 input packet too large: {}", len);
        return false;
    }
    ComPtr<IMFMediaBuffer> buffer;
    HRESULT hr = MFCreateMemoryBuffer(static_cast<DWORD>(len), &buffer);
    if (!ok(hr, "MFCreateMemoryBuffer input")) return false;

    BYTE* dst = nullptr;
    DWORD max_len = 0;
    hr = buffer->Lock(&dst, &max_len, nullptr);
    if (!ok(hr, "MFT input buffer Lock")) return false;
    if (len > 0) {
        std::memcpy(dst, data, len);
    }
    buffer->Unlock();
    buffer->SetCurrentLength(static_cast<DWORD>(len));

    ComPtr<IMFSample> sample;
    hr = MFCreateSample(&sample);
    if (!ok(hr, "MFCreateSample input")) return false;
    sample->AddBuffer(buffer.Get());
    sample->SetSampleTime(timestamp);
    *sample_out = sample.Detach();
    return true;
}

bool output_sample_to_nv12(IMFSample* sample, uint32_t width, uint32_t height, uint8_t* output, size_t output_len) {
    const size_t required = static_cast<size_t>(width) * height * 3 / 2;
    if (!output || output_len < required) {
        native_log_error("MFT H264 output buffer too small: len={} required={}", output_len, required);
        return false;
    }
    ComPtr<IMFMediaBuffer> buffer;
    HRESULT hr = sample->ConvertToContiguousBuffer(&buffer);
    if (!ok(hr, "MFT ConvertToContiguousBuffer")) return false;

    BYTE* src = nullptr;
    DWORD max_len = 0;
    DWORD current_len = 0;
    hr = buffer->Lock(&src, &max_len, &current_len);
    if (!ok(hr, "MFT output buffer Lock")) return false;
    const size_t copy_len = (std::min)(required, static_cast<size_t>(current_len));
    if (copy_len < required) {
        buffer->Unlock();
        native_log_error("MFT H264 output too small: len={} required={}", copy_len, required);
        return false;
    }
    std::memcpy(output, src, required);
    buffer->Unlock();
    return true;
}

int drain_output(PartiesMftH264Decoder* decoder, int output_requested, uint8_t* output, size_t output_len, uint32_t* error_out) {
    MFT_OUTPUT_STREAM_INFO info{};
    HRESULT hr = decoder->transform->GetOutputStreamInfo(0, &info);
    if (!ok(hr, "MFT GetOutputStreamInfo")) {
        if (error_out) *error_out = static_cast<uint32_t>(hr);
        return -1;
    }

    const size_t required = static_cast<size_t>(decoder->width) * decoder->height * 3 / 2;
    const size_t buffer_len = (std::max)(required, static_cast<size_t>(info.cbSize));
    ComPtr<IMFMediaBuffer> out_buffer;
    hr = MFCreateMemoryBuffer(static_cast<DWORD>((std::min)(buffer_len, static_cast<size_t>((std::numeric_limits<DWORD>::max)()))), &out_buffer);
    if (!ok(hr, "MFCreateMemoryBuffer output")) {
        if (error_out) *error_out = static_cast<uint32_t>(hr);
        return -1;
    }
    ComPtr<IMFSample> out_sample;
    hr = MFCreateSample(&out_sample);
    if (!ok(hr, "MFCreateSample output")) {
        if (error_out) *error_out = static_cast<uint32_t>(hr);
        return -1;
    }
    out_sample->AddBuffer(out_buffer.Get());

    MFT_OUTPUT_DATA_BUFFER out{};
    out.dwStreamID = 0;
    out.pSample = out_sample.Get();
    DWORD status = 0;
    hr = decoder->transform->ProcessOutput(0, 1, &out, &status);
    if (out.pEvents) {
        out.pEvents->Release();
    }
    if (hr == MF_E_TRANSFORM_NEED_MORE_INPUT) {
        return kNoOutput;
    }
    if (hr == MF_E_TRANSFORM_STREAM_CHANGE) {
        if (!set_output_type(decoder)) {
            if (error_out) *error_out = static_cast<uint32_t>(hr);
            return -1;
        }
        return kNoOutput;
    }
    if (FAILED(hr)) {
        native_log_error("MFT ProcessOutput failed: {:#010x}", static_cast<unsigned>(hr));
        if (error_out) *error_out = static_cast<uint32_t>(hr);
        return -1;
    }
    if (!output_requested) {
        return kDrainedOutputWithoutCopy;
    }
    if (!output_sample_to_nv12(out_sample.Get(), decoder->width, decoder->height, output, output_len)) {
        if (error_out) *error_out = 1;
        return -1;
    }
    return kCopiedOutput;
}

int drain_available_output(PartiesMftH264Decoder* decoder, int output_requested, uint8_t* output, size_t output_len, uint32_t* error_out) {
    bool copied_output = false;
    for (int attempt = 0; attempt < 32; ++attempt) {
        const int status = drain_output(decoder, output_requested, output, output_len, error_out);
        if (status < 0) {
            return status;
        }
        if (status == kNoOutput) {
            return copied_output ? kCopiedOutput : kNoOutput;
        }
        if (status == kCopiedOutput) {
            copied_output = true;
        }
    }
    return copied_output ? kCopiedOutput : kNoOutput;
}

} // namespace

extern "C" {

PartiesMftH264Decoder* parties_mft_h264_decoder_create(uint32_t width, uint32_t height) {
    auto* decoder = new PartiesMftH264Decoder();
    decoder->width = width;
    decoder->height = height;

    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (SUCCEEDED(hr)) {
        decoder->com_initialized = true;
    } else if (hr != RPC_E_CHANGED_MODE) {
        native_log_error("MFT H264 CoInitializeEx failed: {:#010x}", static_cast<unsigned>(hr));
        delete decoder;
        return nullptr;
    }

    hr = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET);
    if (FAILED(hr)) {
        native_log_error("MFT H264 MFStartup failed: {:#010x}", static_cast<unsigned>(hr));
        delete decoder;
        return nullptr;
    }
    decoder->mf_started = true;

    hr = CoCreateInstance(CLSID_CMSH264DecoderMFT, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&decoder->transform));
    if (FAILED(hr) || !decoder->transform) {
        native_log_error("MFT H264 CoCreateInstance failed: {:#010x}", static_cast<unsigned>(hr));
        delete decoder;
        return nullptr;
    }

    configure_low_latency_decode(decoder);
    if (!set_input_type(decoder) || !set_output_type(decoder)) {
        delete decoder;
        return nullptr;
    }
    decoder->transform->ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
    decoder->transform->ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
    native_log_info("MFT H264 decoder ready: size={}x{}", width, height);
    return decoder;
}

void parties_mft_h264_decoder_destroy(PartiesMftH264Decoder* decoder) {
    if (!decoder) return;
    if (decoder->transform) {
        decoder->transform->ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
        decoder->transform->ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        decoder->transform.Reset();
    }
    if (decoder->mf_started) {
        MFShutdown();
    }
    if (decoder->com_initialized) {
        CoUninitialize();
    }
    delete decoder;
}

int parties_mft_h264_decoder_decode(
    PartiesMftH264Decoder* decoder,
    const uint8_t* data,
    size_t len,
    int64_t timestamp,
    int output_requested,
    uint8_t* output,
    size_t output_len,
    uint32_t* width_out,
    uint32_t* height_out,
    uint32_t* error_out
) {
    if (error_out) *error_out = 0;
    if (width_out) *width_out = decoder ? decoder->width : 0;
    if (height_out) *height_out = decoder ? decoder->height : 0;
    if (!decoder || !decoder->transform || (!data && len != 0)) {
        if (error_out) *error_out = 1;
        return -1;
    }

    int drained = drain_available_output(decoder, output_requested, output, output_len, error_out);
    if (drained < 0) {
        return drained;
    }
    const bool copied_before_input = drained == kCopiedOutput;

    ComPtr<IMFSample> input_sample;
    if (!create_sample_from_bytes(data, len, timestamp, decoder->transform.Get(), &input_sample)) {
        if (error_out) *error_out = 2;
        return -1;
    }

    HRESULT hr = decoder->transform->ProcessInput(0, input_sample.Get(), 0);
    if (hr == MF_E_NOTACCEPTING) {
        drained = drain_available_output(decoder, output_requested, output, output_len, error_out);
        if (drained < 0) {
            return drained;
        }
        if (drained == kCopiedOutput) {
            // Keep the freshest output if ProcessInput still accepts the current packet below.
        }
        hr = decoder->transform->ProcessInput(0, input_sample.Get(), 0);
    }
    if (FAILED(hr)) {
        native_log_error("MFT ProcessInput failed: {:#010x}", static_cast<unsigned>(hr));
        if (error_out) *error_out = static_cast<uint32_t>(hr);
        return -1;
    }

    drained = drain_available_output(decoder, output_requested, output, output_len, error_out);
    if (drained < 0) {
        return drained;
    }
    return drained == kCopiedOutput || copied_before_input ? kCopiedOutput : kNoOutput;
}

} // extern "C"
