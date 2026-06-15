/*
 * Portable libhevc function selector used when target-specific optimized
 * selector files are not compiled.
 * Apache-2.0 (same license as libhevc)
 */
#include "ihevc_typedefs.h"
#include "iv.h"
#include "ivd.h"
#include "ihevc_defs.h"
#include "ihevc_debug.h"
#include "ihevc_structs.h"
#include "ihevc_macros.h"
#include "ihevc_platform_macros.h"
#include "ihevc_cabac_tables.h"
#include "ihevc_disp_mgr.h"
#include "ihevc_buf_mgr.h"
#include "ihevc_dpb_mgr.h"
#include "ihevc_error.h"

#include "ihevcd_defs.h"
#include "ihevcd_function_selector.h"
#include "ihevcd_structs.h"

void ihevcd_init_function_ptr(void *pv_codec)
{
    ihevcd_init_function_ptr_generic(pv_codec);
}

void ihevcd_init_arch(void *pv_codec)
{
    codec_t *ps_codec = (codec_t *)pv_codec;
    ps_codec->e_processor_arch = ARCH_NA;
}
