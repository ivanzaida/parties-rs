#include <algorithm>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <thread>
#include <vector>

#if defined(_WIN32)
#include <malloc.h>
#endif

#include "ihevc_typedefs.h"
#include "iv.h"
#include "ivd.h"
#include "ihevcd_cxa.h"

namespace {

struct PartiesLibhevcDecoder {
    void* codec_obj = nullptr;
    std::vector<uint8_t> y_buf;
    std::vector<uint8_t> u_buf;
    std::vector<uint8_t> v_buf;
    uint32_t width = 0;
    uint32_t height = 0;
};

extern "C" void parties_libhevc_decoder_destroy(PartiesLibhevcDecoder* decoder);

static inline iv_obj_t* obj(void* ptr) {
    return static_cast<iv_obj_t*>(ptr);
}

void* hevc_aligned_alloc(void*, WORD32 alignment, WORD32 size) {
#if defined(_WIN32)
    return _aligned_malloc(static_cast<size_t>(size), static_cast<size_t>(alignment));
#else
    void* ptr = nullptr;
    return posix_memalign(&ptr, static_cast<size_t>(alignment), static_cast<size_t>(size)) == 0 ? ptr : nullptr;
#endif
}

void hevc_aligned_free(void*, void* ptr) {
#if defined(_WIN32)
    _aligned_free(ptr);
#else
    free(ptr);
#endif
}

bool set_decode_mode(PartiesLibhevcDecoder* decoder, IVD_VIDEO_DECODE_MODE_T mode) {
    ihevcd_cxa_ctl_set_config_ip_t input = {};
    ihevcd_cxa_ctl_set_config_op_t output = {};
    input.s_ivd_ctl_set_config_ip_t.u4_size = sizeof(input);
    input.s_ivd_ctl_set_config_ip_t.e_cmd = IVD_CMD_VIDEO_CTL;
    input.s_ivd_ctl_set_config_ip_t.e_sub_cmd = IVD_CMD_CTL_SETPARAMS;
    input.s_ivd_ctl_set_config_ip_t.e_vid_dec_mode = mode;
    input.s_ivd_ctl_set_config_ip_t.e_frm_skip_mode = IVD_SKIP_NONE;
    input.s_ivd_ctl_set_config_ip_t.e_frm_out_mode = IVD_DISPLAY_FRAME_OUT;
    input.s_ivd_ctl_set_config_ip_t.u4_disp_wd = 0;
    output.s_ivd_ctl_set_config_op_t.u4_size = sizeof(output);
    return ihevcd_cxa_api_function(obj(decoder->codec_obj), &input, &output) == IV_SUCCESS;
}

bool set_num_cores(PartiesLibhevcDecoder* decoder, uint32_t threads) {
    ihevcd_cxa_ctl_set_num_cores_ip_t input = {};
    ihevcd_cxa_ctl_set_num_cores_op_t output = {};
    input.u4_size = sizeof(input);
    input.e_cmd = IVD_CMD_VIDEO_CTL;
    input.e_sub_cmd = static_cast<IVD_CONTROL_API_COMMAND_TYPE_T>(IHEVCD_CXA_CMD_CTL_SET_NUM_CORES);
    input.u4_num_cores = std::max<uint32_t>(1, std::min<uint32_t>(threads, 4));
    output.u4_size = sizeof(output);
    return ihevcd_cxa_api_function(obj(decoder->codec_obj), &input, &output) == IV_SUCCESS;
}

void allocate_output_buffers(PartiesLibhevcDecoder* decoder, uint32_t width, uint32_t height) {
    const uint32_t aligned_height = (height + 63u) & ~63u;
    const size_t y_size = static_cast<size_t>(width) * aligned_height;
    const size_t uv_size = y_size / 4;
    decoder->y_buf.resize(y_size);
    decoder->u_buf.resize(uv_size);
    decoder->v_buf.resize(uv_size);
    decoder->width = width;
    decoder->height = height;
}

bool refresh_output_buffers(PartiesLibhevcDecoder* decoder) {
    ihevcd_cxa_ctl_getbufinfo_ip_t input = {};
    ihevcd_cxa_ctl_getbufinfo_op_t output = {};
    input.s_ivd_ctl_getbufinfo_ip_t.u4_size = sizeof(input);
    input.s_ivd_ctl_getbufinfo_ip_t.e_cmd = IVD_CMD_VIDEO_CTL;
    input.s_ivd_ctl_getbufinfo_ip_t.e_sub_cmd = IVD_CMD_CTL_GETBUFINFO;
    output.s_ivd_ctl_getbufinfo_op_t.u4_size = sizeof(output);
    if (ihevcd_cxa_api_function(obj(decoder->codec_obj), &input, &output) != IV_SUCCESS) {
        return false;
    }
    const auto& info = output.s_ivd_ctl_getbufinfo_op_t;
    if (info.u4_num_disp_bufs < 3) {
        return false;
    }
    decoder->y_buf.resize(info.u4_min_out_buf_size[0]);
    decoder->u_buf.resize(info.u4_min_out_buf_size[1]);
    decoder->v_buf.resize(info.u4_min_out_buf_size[2]);
    return true;
}

bool reset_decoder(PartiesLibhevcDecoder* decoder, uint32_t threads) {
    ihevcd_cxa_ctl_reset_ip_t input = {};
    ihevcd_cxa_ctl_reset_op_t output = {};
    input.s_ivd_ctl_reset_ip_t.u4_size = sizeof(input);
    input.s_ivd_ctl_reset_ip_t.e_cmd = IVD_CMD_VIDEO_CTL;
    input.s_ivd_ctl_reset_ip_t.e_sub_cmd = IVD_CMD_CTL_RESET;
    output.s_ivd_ctl_reset_op_t.u4_size = sizeof(output);
    if (ihevcd_cxa_api_function(obj(decoder->codec_obj), &input, &output) != IV_SUCCESS) {
        return false;
    }
    return set_num_cores(decoder, threads) && set_decode_mode(decoder, IVD_DECODE_FRAME);
}

bool copy_i420_to_nv12(const iv_yuv_buf_t& src, uint8_t* output, size_t output_len) {
    const uint32_t width = src.u4_y_wd;
    const uint32_t height = src.u4_y_ht;
    const size_t required = static_cast<size_t>(width) * height * 3 / 2;
    if (!src.pv_y_buf || !src.pv_u_buf || !src.pv_v_buf || output_len < required) {
        return false;
    }

    auto* dst_y = output;
    auto* dst_uv = output + static_cast<size_t>(width) * height;
    const auto* src_y = static_cast<const uint8_t*>(src.pv_y_buf);
    const auto* src_u = static_cast<const uint8_t*>(src.pv_u_buf);
    const auto* src_v = static_cast<const uint8_t*>(src.pv_v_buf);

    for (uint32_t row = 0; row < height; ++row) {
        std::memcpy(dst_y + static_cast<size_t>(row) * width, src_y + static_cast<size_t>(row) * src.u4_y_strd, width);
    }

    for (uint32_t row = 0; row < height / 2; ++row) {
        const auto* u_row = src_u + static_cast<size_t>(row) * src.u4_u_strd;
        const auto* v_row = src_v + static_cast<size_t>(row) * src.u4_v_strd;
        auto* uv_row = dst_uv + static_cast<size_t>(row) * width;
        for (uint32_t column = 0; column < width / 2; ++column) {
            uv_row[column * 2] = u_row[column];
            uv_row[column * 2 + 1] = v_row[column];
        }
    }

    return true;
}

} // namespace

extern "C" {

PartiesLibhevcDecoder* parties_libhevc_decoder_create(uint32_t width, uint32_t height, uint32_t threads) {
    ihevcd_cxa_create_ip_t input = {};
    ihevcd_cxa_create_op_t output = {};

    input.s_ivd_create_ip_t.u4_size = sizeof(input);
    input.s_ivd_create_ip_t.e_cmd = IVD_CMD_CREATE;
    input.s_ivd_create_ip_t.e_output_format = IV_YUV_420P;
    input.s_ivd_create_ip_t.u4_share_disp_buf = 0;
    input.s_ivd_create_ip_t.pf_aligned_alloc = hevc_aligned_alloc;
    input.s_ivd_create_ip_t.pf_aligned_free = hevc_aligned_free;
    input.s_ivd_create_ip_t.pv_mem_ctxt = nullptr;
    input.u4_enable_frame_info = 0;
    input.u4_keep_threads_active = 0;
    input.u4_enable_yuv_formats = 0;
    output.s_ivd_create_op_t.u4_size = sizeof(output);

    if (ihevcd_cxa_api_function(nullptr, &input, &output) != IV_SUCCESS || !output.s_ivd_create_op_t.pv_handle) {
        return nullptr;
    }

    auto* decoder = new PartiesLibhevcDecoder();
    decoder->codec_obj = output.s_ivd_create_op_t.pv_handle;
    obj(decoder->codec_obj)->pv_fxns = reinterpret_cast<void*>(&ihevcd_cxa_api_function);
    obj(decoder->codec_obj)->u4_size = sizeof(iv_obj_t);
    allocate_output_buffers(decoder, width, height);

    if (!set_num_cores(decoder, threads) || !set_decode_mode(decoder, IVD_DECODE_FRAME)) {
        parties_libhevc_decoder_destroy(decoder);
        return nullptr;
    }

    return decoder;
}

void parties_libhevc_decoder_destroy(PartiesLibhevcDecoder* decoder) {
    if (!decoder) {
        return;
    }
    if (decoder->codec_obj) {
        ivd_delete_ip_t input = {};
        ivd_delete_op_t output = {};
        input.u4_size = sizeof(input);
        input.e_cmd = IVD_CMD_DELETE;
        output.u4_size = sizeof(output);
        ihevcd_cxa_api_function(obj(decoder->codec_obj), &input, &output);
    }
    delete decoder;
}

int parties_libhevc_decoder_decode(
    PartiesLibhevcDecoder* decoder,
    const uint8_t* data,
    size_t len,
    int64_t timestamp,
    int output_requested,
    uint8_t* output,
    size_t output_len,
    uint32_t* width_out,
    uint32_t* height_out,
    uint32_t threads,
    uint32_t* error_out
) {
    if (error_out) {
        *error_out = 0;
    }
    if (!decoder || !decoder->codec_obj || (!data && len != 0)) {
        if (error_out) {
            *error_out = 1;
        }
        return -1;
    }

    size_t remaining = len;
    const uint8_t* cursor = data;
    bool produced = false;

    while (remaining > 0 || len == 0) {
        ihevcd_cxa_video_decode_ip_t input = {};
        ihevcd_cxa_video_decode_op_t result = {};
        ivd_video_decode_ip_t* ip = &input.s_ivd_video_decode_ip_t;
        ivd_video_decode_op_t* op = &result.s_ivd_video_decode_op_t;

        ip->u4_size = sizeof(input);
        ip->e_cmd = IVD_CMD_VIDEO_DECODE;
        ip->u4_ts = static_cast<UWORD32>(timestamp & 0xFFFFFFFF);
        ip->pv_stream_buffer = const_cast<uint8_t*>(cursor);
        ip->u4_num_Bytes = static_cast<UWORD32>(remaining);
        ip->s_out_buffer.u4_num_bufs = 3;
        ip->s_out_buffer.pu1_bufs[0] = decoder->y_buf.data();
        ip->s_out_buffer.pu1_bufs[1] = decoder->u_buf.data();
        ip->s_out_buffer.pu1_bufs[2] = decoder->v_buf.data();
        ip->s_out_buffer.u4_min_out_buf_size[0] = static_cast<UWORD32>(decoder->y_buf.size());
        ip->s_out_buffer.u4_min_out_buf_size[1] = static_cast<UWORD32>(decoder->u_buf.size());
        ip->s_out_buffer.u4_min_out_buf_size[2] = static_cast<UWORD32>(decoder->v_buf.size());
        op->u4_size = sizeof(result);

        const IV_API_CALL_STATUS_T status = ihevcd_cxa_api_function(obj(decoder->codec_obj), &input, &result);
        if (status != IV_SUCCESS && (op->u4_error_code & 0xff) == IVD_RES_CHANGED) {
            if (!refresh_output_buffers(decoder) || !reset_decoder(decoder, threads)) {
                if (error_out) {
                    *error_out = op->u4_error_code;
                }
                return -1;
            }
            continue;
        }

        if (status != IV_SUCCESS && op->u4_num_bytes_consumed == 0) {
            if (error_out) {
                *error_out = op->u4_error_code;
            }
            return -1;
        }

        if (op->u4_output_present) {
            produced = true;
            if (width_out) {
                *width_out = op->s_disp_frm_buf.u4_y_wd;
            }
            if (height_out) {
                *height_out = op->s_disp_frm_buf.u4_y_ht;
            }
            if (output_requested && !copy_i420_to_nv12(op->s_disp_frm_buf, output, output_len)) {
                if (error_out) {
                    *error_out = 2;
                }
                return -1;
            }
        }

        const UWORD32 consumed = op->u4_num_bytes_consumed;
        if (consumed == 0 || len == 0) {
            break;
        }
        cursor += consumed;
        remaining -= consumed;
    }

    return produced ? 1 : 0;
}

void parties_libhevc_decoder_flush(PartiesLibhevcDecoder* decoder) {
    if (!decoder || !decoder->codec_obj) {
        return;
    }
    ivd_ctl_flush_ip_t input = {};
    ivd_ctl_flush_op_t output = {};
    input.u4_size = sizeof(input);
    input.e_cmd = IVD_CMD_VIDEO_CTL;
    input.e_sub_cmd = IVD_CMD_CTL_FLUSH;
    output.u4_size = sizeof(output);
    ihevcd_cxa_api_function(obj(decoder->codec_obj), &input, &output);
}

const char* parties_libhevc_decoder_version() {
    return "libhevc 1.7.0";
}

}
