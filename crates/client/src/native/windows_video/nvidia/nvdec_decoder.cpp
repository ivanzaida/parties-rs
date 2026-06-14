#include "nvdec_decoder.h"

#include <cstring>

#include <windows.h>

namespace parties_rs::video::nvidia {

static CUresult seh_cuvidParseVideoData(
        decltype(CuvidApi::cuvidParseVideoData) fn,
        CUvideoparser parser, CUVIDSOURCEDATAPACKET* pkt) {
    __try {
        return fn(parser, pkt);
    } __except (GetExceptionCode() == EXCEPTION_ACCESS_VIOLATION
                    ? EXCEPTION_EXECUTE_HANDLER
                    : EXCEPTION_CONTINUE_SEARCH) {
        return static_cast<CUresult>(999);
    }
}

NvdecDecoder::NvdecDecoder() = default;

NvdecDecoder::~NvdecDecoder() {
    if (!initialized_ && !context_lost_) return;

    if (cu_ctx_) {
        if (!context_lost_) {
            cuda_.cuCtxPushCurrent(cu_ctx_);

            if (parser_) cuvid_.cuvidDestroyVideoParser(parser_);
            if (decoder_) cuvid_.cuvidDestroyDecoder(decoder_);
            release_external_array(external_y_texture_);
            release_external_array(external_uv_texture_);
            if (pinned_nv12_) cuda_.cuMemFreeHost(pinned_nv12_);

            CUcontext dummy;
            cuda_.cuCtxPopCurrent(&dummy);
        }
        cuda_.cuCtxDestroy(cu_ctx_);
    }

    cu_ctx_ = nullptr;
    parser_ = nullptr;
    decoder_ = nullptr;
    pinned_nv12_ = nullptr;
    pinned_nv12_size_ = 0;
    initialized_ = false;
    context_lost_ = false;
}

static cudaVideoCodec to_cuvid_codec(VideoCodecId id) {
    switch (id) {
    case VideoCodecId::H264: return cudaVideoCodec_H264;
    case VideoCodecId::H265: return cudaVideoCodec_HEVC;
    case VideoCodecId::AV1:  return cudaVideoCodec_AV1;
    default:                 return cudaVideoCodec_AV1;
    }
}

bool NvdecDecoder::init(VideoCodecId codec, uint32_t width, uint32_t height) {
    if (initialized_) return false;

    if (!load_cuda(cuda_)) return false;
    if (!load_cuvid(cuvid_)) return false;

    codec_ = codec;
    cudaVideoCodec cuvid_codec = to_cuvid_codec(codec);

    CUdevice cu_device = 0;
    CUresult res = cuda_.cuDeviceGet(&cu_device, 0);
    if (res != CUDA_SUCCESS) {
        native_log_error("cuDeviceGet failed: {}", (int)res);
        return false;
    }
    char device_name[256]{};
    if (cuda_.cuDeviceGetName(device_name, static_cast<int>(sizeof(device_name)), cu_device) == CUDA_SUCCESS) {
        native_log_info("NVDEC selected CUDA device: ordinal={} name='{}'", 0, device_name);
    } else {
        native_log_info("NVDEC selected CUDA device: ordinal={}", 0);
    }

    res = cuda_.cuCtxCreate(&cu_ctx_, CU_CTX_SCHED_AUTO, cu_device);
    if (res != CUDA_SUCCESS) {
        native_log_error("cuCtxCreate failed: {}", (int)res);
        return false;
    }

    CUVIDDECODECAPS caps{};
    caps.eCodecType = cuvid_codec;
    caps.eChromaFormat = cudaVideoChromaFormat_420;
    caps.nBitDepthMinus8 = 0;

    res = cuvid_.cuvidGetDecoderCaps(&caps);
    if (res != CUDA_SUCCESS || !caps.bIsSupported) {
        native_log_error("{} not supported (res={}, supported={})",
                  codec_name(codec), (int)res, (int)caps.bIsSupported);
        cuda_.cuCtxDestroy(cu_ctx_);
        cu_ctx_ = nullptr;
        return false;
    }

    if (width > caps.nMaxWidth || height > caps.nMaxHeight) {
        native_log_error("Resolution {}x{} exceeds max {}x{}",
                  width, height, caps.nMaxWidth, caps.nMaxHeight);
        cuda_.cuCtxDestroy(cu_ctx_);
        cu_ctx_ = nullptr;
        return false;
    }

    width_ = width;
    height_ = height;

    CUVIDPARSERPARAMS parser_params{};
    parser_params.CodecType = cuvid_codec;
    parser_params.ulMaxNumDecodeSurfaces = 10;
    parser_params.ulMaxDisplayDelay = 0;  // No B-frames in our stream, display immediately
    parser_params.pUserData = this;
    parser_params.pfnSequenceCallback = handle_sequence;
    parser_params.pfnDecodePicture = handle_decode;
    parser_params.pfnDisplayPicture = handle_display;

    res = cuvid_.cuvidCreateVideoParser(&parser_, &parser_params);
    if (res != CUDA_SUCCESS) {
        native_log_error("cuvidCreateVideoParser failed: {}", (int)res);
        cuda_.cuCtxDestroy(cu_ctx_);
        cu_ctx_ = nullptr;
        return false;
    }

    CUcontext dummy;
    cuda_.cuCtxPopCurrent(&dummy);

    native_log_info("Initialized {} decoder ({}x{})",
             codec_name(codec), width_, height_);
    initialized_ = true;
    return true;
}

bool NvdecDecoder::decode(const uint8_t* data, size_t len, int64_t timestamp) {
    if (!initialized_ || context_lost_) return false;

    CUresult res = cuda_.cuCtxPushCurrent(cu_ctx_);
    if (res != CUDA_SUCCESS) {
        native_log_error("CUDA context lost (cuCtxPushCurrent={})", (int)res);
        context_lost_ = true;
        initialized_ = false;
        return false;
    }

    CUVIDSOURCEDATAPACKET pkt{};
    pkt.flags = CUVID_PKT_TIMESTAMP;
    pkt.payload_size = static_cast<unsigned long>(len);
    pkt.payload = data;
    pkt.timestamp = timestamp;

    res = seh_cuvidParseVideoData(cuvid_.cuvidParseVideoData, parser_, &pkt);

    CUcontext dummy;
    cuda_.cuCtxPopCurrent(&dummy);

    if (res != CUDA_SUCCESS) {
        native_log_error("cuvidParseVideoData failed: {} (GPU context invalidated)", (int)res);
        context_lost_ = true;
        initialized_ = false;
        return false;
    }

    return true;
}

void NvdecDecoder::flush() {
    if (!initialized_ || context_lost_) return;

    if (cuda_.cuCtxPushCurrent(cu_ctx_) != CUDA_SUCCESS) return;

    CUVIDSOURCEDATAPACKET pkt{};
    pkt.flags = CUVID_PKT_ENDOFSTREAM;
    seh_cuvidParseVideoData(cuvid_.cuvidParseVideoData, parser_, &pkt);

    CUcontext dummy;
    cuda_.cuCtxPopCurrent(&dummy);
}

DecoderInfo NvdecDecoder::info() const {
    return {VideoBackend::NVDEC, codec_};
}

void NvdecDecoder::set_output_buffer(uint8_t* nv12, size_t nv12_size) {
    external_nv12_ = nv12;
    external_nv12_size_ = nv12_size;
}

void NvdecDecoder::set_output_arrays(CUarray y_array, CUarray uv_array, uint32_t width, uint32_t height) {
    external_y_array_ = y_array;
    external_uv_array_ = uv_array;
    external_array_width_ = width;
    external_array_height_ = height;
}

bool NvdecDecoder::set_output_d3d12_textures(
        void* y_handle, unsigned long long y_size,
        void* uv_handle, unsigned long long uv_size,
        uint32_t width, uint32_t height) {
    if (!initialized_ || context_lost_ || !y_handle || !uv_handle || width == 0 || height == 0) {
        return false;
    }
    if (cuda_.cuCtxPushCurrent(cu_ctx_) != CUDA_SUCCESS) {
        context_lost_ = true;
        initialized_ = false;
        return false;
    }

    const bool y_ok = ensure_external_array(external_y_texture_, y_handle, y_size, width, height, 1);
    const bool uv_ok = ensure_external_array(external_uv_texture_, uv_handle, uv_size, width / 2, height / 2, 2);

    CUcontext dummy;
    cuda_.cuCtxPopCurrent(&dummy);

    if (!y_ok || !uv_ok) {
        set_output_arrays(nullptr, nullptr, 0, 0);
        return false;
    }
    set_output_arrays(external_y_texture_.array, external_uv_texture_.array, width, height);
    return true;
}

void NvdecDecoder::clear_output_d3d12_textures() {
    set_output_arrays(nullptr, nullptr, 0, 0);
}

bool NvdecDecoder::ensure_external_array(
        ExternalArray& target, void* handle, unsigned long long size,
        uint32_t width, uint32_t height, uint32_t channels) {
    if (target.handle == handle && target.size == size &&
        target.width == width && target.height == height && target.channels == channels &&
        target.memory && target.mipmapped && target.array) {
        return true;
    }

    release_external_array(target);

    CUDA_EXTERNAL_MEMORY_HANDLE_DESC handle_desc{};
    handle_desc.type = CU_EXTERNAL_MEMORY_HANDLE_TYPE_D3D12_RESOURCE;
    handle_desc.handle.win32.handle = handle;
    handle_desc.size = size;
    handle_desc.flags = CUDA_EXTERNAL_MEMORY_DEDICATED;

    CUresult res = cuda_.cuImportExternalMemory(&target.memory, &handle_desc);
    if (res != CUDA_SUCCESS) {
        native_log_error("cuImportExternalMemory failed: {}", (int)res);
        release_external_array(target);
        return false;
    }

    CUDA_EXTERNAL_MEMORY_MIPMAPPED_ARRAY_DESC array_desc{};
    array_desc.offset = 0;
    array_desc.arrayDesc.Width = width;
    array_desc.arrayDesc.Height = height;
    array_desc.arrayDesc.Depth = 0;
    array_desc.arrayDesc.Format = CU_AD_FORMAT_UNSIGNED_INT8;
    array_desc.arrayDesc.NumChannels = channels;
    array_desc.arrayDesc.Flags = 0;
    array_desc.numLevels = 1;

    res = cuda_.cuExternalMemoryGetMappedMipmappedArray(&target.mipmapped, target.memory, &array_desc);
    if (res != CUDA_SUCCESS) {
        native_log_error("cuExternalMemoryGetMappedMipmappedArray failed: {}", (int)res);
        release_external_array(target);
        return false;
    }

    res = cuda_.cuMipmappedArrayGetLevel(&target.array, target.mipmapped, 0);
    if (res != CUDA_SUCCESS) {
        native_log_error("cuMipmappedArrayGetLevel failed: {}", (int)res);
        release_external_array(target);
        return false;
    }

    target.handle = handle;
    target.size = size;
    target.width = width;
    target.height = height;
    target.channels = channels;
    return true;
}

void NvdecDecoder::release_external_array(ExternalArray& target) {
    if (target.mipmapped) {
        cuda_.cuMipmappedArrayDestroy(target.mipmapped);
    }
    if (target.memory) {
        cuda_.cuDestroyExternalMemory(target.memory);
    }
    target = ExternalArray{};
}

int NvdecDecoder::handle_sequence(void* user, CUVIDEOFORMAT* fmt) {
    return static_cast<NvdecDecoder*>(user)->on_sequence(fmt);
}

int NvdecDecoder::handle_decode(void* user, CUVIDPICPARAMS* pic) {
    return static_cast<NvdecDecoder*>(user)->on_decode(pic);
}

int NvdecDecoder::handle_display(void* user, CUVIDPARSERDISPINFO* info) {
    return static_cast<NvdecDecoder*>(user)->on_display(info);
}

int NvdecDecoder::on_sequence(CUVIDEOFORMAT* fmt) {

    if (fmt->coded_width == 0 || fmt->coded_height == 0 ||
        fmt->chroma_format > cudaVideoChromaFormat_444) {
        native_log_error("Ignoring invalid sequence header");
        return 1;
    }

    // Compute the actual output dimensions (may differ from coded_width/height
    // if the stream specifies a display/crop area).
    uint32_t target_w = fmt->coded_width;
    uint32_t target_h = fmt->coded_height;
    if (fmt->display_area.right > fmt->display_area.left &&
        fmt->display_area.bottom > fmt->display_area.top) {
        target_w = fmt->display_area.right - fmt->display_area.left;
        target_h = fmt->display_area.bottom - fmt->display_area.top;
    }

    // If the format hasn't changed, reuse the existing decoder.
    // Recreating on every keyframe's sequence header is expensive (GPU alloc)
    // and drops any frame buffered in the parser's display pipeline.
    if (decoder_ &&
        target_w == width_ && target_h == height_ &&
        fmt->bit_depth_luma_minus8 + 8 == bit_depth_ &&
        fmt->min_num_decode_surfaces + 4 <= num_decode_surfaces_) {
        return num_decode_surfaces_;
    }

    native_log_info("on_sequence: recreating decoder ({}x{} bd={} surfaces={} -> {}x{} bd={} surfaces={})",
             width_, height_, bit_depth_, num_decode_surfaces_,
             target_w, target_h, fmt->bit_depth_luma_minus8 + 8,
             fmt->min_num_decode_surfaces + 4);

    if (decoder_) {
        cuvid_.cuvidDestroyDecoder(decoder_);
        decoder_ = nullptr;
    }

    bit_depth_ = fmt->bit_depth_luma_minus8 + 8;
    num_decode_surfaces_ = fmt->min_num_decode_surfaces + 4;
    width_ = target_w;
    height_ = target_h;

    bool is_10bit = fmt->bit_depth_luma_minus8 > 0;
    auto output_fmt = is_10bit ? cudaVideoSurfaceFormat_P016
                               : cudaVideoSurfaceFormat_NV12;

    CUVIDDECODECREATEINFO create_info{};
    create_info.ulWidth = fmt->coded_width;
    create_info.ulHeight = fmt->coded_height;
    create_info.ulNumDecodeSurfaces = num_decode_surfaces_;
    create_info.CodecType = fmt->codec;
    create_info.ChromaFormat = fmt->chroma_format;
    create_info.ulCreationFlags = cudaVideoCreate_PreferCUVID;
    create_info.bitDepthMinus8 = fmt->bit_depth_luma_minus8;
    create_info.OutputFormat = output_fmt;
    create_info.DeinterlaceMode = cudaVideoDeinterlaceMode_Weave;
    create_info.ulTargetWidth = target_w;
    create_info.ulTargetHeight = target_h;
    create_info.ulNumOutputSurfaces = 2;

    if (fmt->display_area.right > fmt->display_area.left &&
        fmt->display_area.bottom > fmt->display_area.top) {
        create_info.display_area.left = static_cast<short>(fmt->display_area.left);
        create_info.display_area.top = static_cast<short>(fmt->display_area.top);
        create_info.display_area.right = static_cast<short>(fmt->display_area.right);
        create_info.display_area.bottom = static_cast<short>(fmt->display_area.bottom);
    }

    CUresult res = cuvid_.cuvidCreateDecoder(&decoder_, &create_info);
    if (res != CUDA_SUCCESS) {
        native_log_error("cuvidCreateDecoder failed: {}", (int)res);
        return 0;
    }

    const size_t nv12_size = static_cast<size_t>(width_) * height_ * 3 / 2;
    if ((!external_nv12_ || external_nv12_size_ < nv12_size) && nv12_size > pinned_nv12_size_) {
        if (pinned_nv12_) cuda_.cuMemFreeHost(pinned_nv12_);
        pinned_nv12_ = nullptr;
        pinned_nv12_size_ = 0;

        void* ptr = nullptr;
        res = cuda_.cuMemAllocHost(&ptr, nv12_size);
        if (res == CUDA_SUCCESS) {
            pinned_nv12_ = static_cast<uint8_t*>(ptr);
            pinned_nv12_size_ = nv12_size;
        }
    }

    return static_cast<int>(num_decode_surfaces_);
}

int NvdecDecoder::on_decode(CUVIDPICPARAMS* pic) {
    if (!decoder_) return 0;

    CUresult res = cuvid_.cuvidDecodePicture(decoder_, pic);
    if (res != CUDA_SUCCESS) {
        native_log_error("cuvidDecodePicture failed: {}", (int)res);
        return 0;
    }
    return 1;
}

int NvdecDecoder::on_display(CUVIDPARSERDISPINFO* disp_info) {
    if (!decoder_ || !disp_info || !on_decoded) return 1;

    CUVIDPROCPARAMS proc{};
    proc.progressive_frame = disp_info->progressive_frame;
    proc.top_field_first = disp_info->top_field_first;

    unsigned long long dev_ptr = 0;
    unsigned int pitch = 0;

    CUresult res = cuvid_.cuvidMapVideoFrame64(
        decoder_, disp_info->picture_index, &dev_ptr, &pitch, &proc);
    if (res != CUDA_SUCCESS) {
        native_log_error("cuvidMapVideoFrame64 failed: {}", (int)res);
        return 0;
    }

    const bool output_arrays =
        external_y_array_ && external_uv_array_ &&
        external_array_width_ == width_ && external_array_height_ == height_;
    const size_t host_size = static_cast<size_t>(width_) * height_ * 3 / 2;
    uint8_t* host_nv12 = nullptr;
    if (!output_arrays && external_nv12_ && external_nv12_size_ >= host_size) {
        host_nv12 = external_nv12_;
    } else if (!output_arrays && pinned_nv12_ && pinned_nv12_size_ >= host_size) {
        host_nv12 = pinned_nv12_;
    }

    {

        if (output_arrays) {
            CUDA_MEMCPY2D copy{};

            copy.srcMemoryType = CU_MEMORYTYPE_DEVICE;
            copy.srcDevice = dev_ptr;
            copy.srcPitch = pitch;
            copy.dstMemoryType = CU_MEMORYTYPE_ARRAY;
            copy.dstArray = external_y_array_;
            copy.WidthInBytes = width_;
            copy.Height = height_;
            CUresult r = cuda_.cuMemcpy2D(&copy);
            if (r != CUDA_SUCCESS) {
                native_log_error("cuMemcpy2D Y array failed: {}", (int)r);
            }

            copy.srcDevice = dev_ptr + static_cast<CUdeviceptr>(pitch) * height_;
            copy.dstArray = external_uv_array_;
            copy.WidthInBytes = width_;
            copy.Height = height_ / 2;
            r = cuda_.cuMemcpy2D(&copy);
            if (r != CUDA_SUCCESS) {
                native_log_error("cuMemcpy2D UV array failed: {}", (int)r);
            }
        } else if (host_nv12) {
            CUDA_MEMCPY2D copy{};

            copy.srcMemoryType = CU_MEMORYTYPE_DEVICE;
            copy.srcDevice = dev_ptr;
            copy.srcPitch = pitch;
            copy.dstMemoryType = CU_MEMORYTYPE_HOST;
            copy.dstHost = host_nv12;
            copy.dstPitch = width_;
            copy.WidthInBytes = width_;
            copy.Height = height_;
            CUresult r = cuda_.cuMemcpy2D(&copy);
            if (r != CUDA_SUCCESS) {
                native_log_error("cuMemcpy2D Y failed: {}", (int)r);
            }

            copy.srcDevice = dev_ptr + static_cast<CUdeviceptr>(pitch) * height_;
            copy.dstHost = host_nv12 + static_cast<size_t>(width_) * height_;
            copy.Height = height_ / 2;
            r = cuda_.cuMemcpy2D(&copy);
            if (r != CUDA_SUCCESS) {
                native_log_error("cuMemcpy2D UV failed: {}", (int)r);
            }
        }
    }

    cuvid_.cuvidUnmapVideoFrame64(decoder_, dev_ptr);
    if (output_arrays) {
        DecodedFrame frame{};
        frame.width = width_;
        frame.height = height_;
        frame.timestamp = disp_info->timestamp;
        frame.nv12 = true;
        on_decoded(frame);
        return 1;
    }
    if (!host_nv12) {
        return 1;
    }

    uint32_t y_size = width_ * height_;

    DecodedFrame frame{};
    frame.y_plane = host_nv12;
    frame.u_plane = host_nv12 + y_size;
    frame.v_plane = nullptr;
    frame.y_stride = width_;
    frame.uv_stride = width_;
    frame.width = width_;
    frame.height = height_;
    frame.timestamp = disp_info->timestamp;
    frame.nv12 = true;

    on_decoded(frame);
    return 1;
}

} // namespace parties_rs::video::nvidia
