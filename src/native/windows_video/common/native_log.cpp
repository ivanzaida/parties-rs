#include "video_types.h"

#include <atomic>
#include <cstdio>
#include <mutex>

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

extern "C" {
using PartiesNativeLogCallback = void (*)(uint8_t level, const char* message);
}

namespace {

std::mutex g_log_mutex;
PartiesNativeLogCallback g_log_callback = nullptr;
std::atomic<bool> g_seh_installed{false};
std::atomic<bool> g_seh_logging{false};

const char* seh_code_name(DWORD code) {
    switch (code) {
    case EXCEPTION_ACCESS_VIOLATION:
        return "access violation";
    case EXCEPTION_ARRAY_BOUNDS_EXCEEDED:
        return "array bounds exceeded";
    case EXCEPTION_BREAKPOINT:
        return "breakpoint";
    case EXCEPTION_DATATYPE_MISALIGNMENT:
        return "datatype misalignment";
    case EXCEPTION_FLT_DIVIDE_BY_ZERO:
        return "float divide by zero";
    case EXCEPTION_ILLEGAL_INSTRUCTION:
        return "illegal instruction";
    case EXCEPTION_INT_DIVIDE_BY_ZERO:
        return "integer divide by zero";
    case EXCEPTION_IN_PAGE_ERROR:
        return "in-page error";
    case EXCEPTION_STACK_OVERFLOW:
        return "stack overflow";
    case 0xC0000409:
        return "stack buffer overrun / fail fast";
    default:
        return "unknown";
    }
}

bool seh_code_is_diagnostic(DWORD code) {
    switch (code) {
    case EXCEPTION_ACCESS_VIOLATION:
    case EXCEPTION_ARRAY_BOUNDS_EXCEEDED:
    case EXCEPTION_DATATYPE_MISALIGNMENT:
    case EXCEPTION_FLT_DIVIDE_BY_ZERO:
    case EXCEPTION_ILLEGAL_INSTRUCTION:
    case EXCEPTION_INT_DIVIDE_BY_ZERO:
    case EXCEPTION_IN_PAGE_ERROR:
    case EXCEPTION_STACK_OVERFLOW:
    case 0xC0000409:
        return true;
    default:
        return false;
    }
}

void log_seh_exception(const char* source, EXCEPTION_POINTERS* info) {
    if (!info || !info->ExceptionRecord) {
        return;
    }

    auto* record = info->ExceptionRecord;
    HMODULE module = nullptr;
    char module_path[MAX_PATH] = "unknown";
    uintptr_t module_rva = 0;
    if (GetModuleHandleExA(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            reinterpret_cast<LPCSTR>(record->ExceptionAddress),
            &module) &&
        module) {
        module_rva = reinterpret_cast<uintptr_t>(record->ExceptionAddress) - reinterpret_cast<uintptr_t>(module);
        const DWORD path_len = GetModuleFileNameA(module, module_path, static_cast<DWORD>(sizeof(module_path)));
        if (path_len == 0 || path_len >= sizeof(module_path)) {
            std::snprintf(module_path, sizeof(module_path), "module@%p", module);
        }
    }
    const DWORD code = record->ExceptionCode;
    const ULONG_PTR p0 = record->NumberParameters > 0 ? record->ExceptionInformation[0] : 0;
    const ULONG_PTR p1 = record->NumberParameters > 1 ? record->ExceptionInformation[1] : 0;
    const ULONG_PTR p2 = record->NumberParameters > 2 ? record->ExceptionInformation[2] : 0;
    char message[512];
    std::snprintf(
        message,
        sizeof(message),
        "[seh] %s exception: code=0x%08lx kind=%s address=%p module=%s+0x%Ix fault0=0x%Ix fault1=0x%Ix fault2=0x%Ix",
        source ? source : "windows",
        static_cast<unsigned long>(code),
        seh_code_name(code),
        record->ExceptionAddress,
        module_path,
        module_rva,
        p0,
        p1,
        p2);
    parties_rs::video::native_log_emit(parties_rs::video::NativeLogLevel::Error, message);
}

LONG WINAPI parties_vectored_exception_handler(EXCEPTION_POINTERS* info) {
    if (!info || !info->ExceptionRecord || !seh_code_is_diagnostic(info->ExceptionRecord->ExceptionCode)) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    if (!g_seh_logging.exchange(true, std::memory_order_acq_rel)) {
        log_seh_exception("vectored", info);
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

LONG WINAPI parties_unhandled_exception_filter(EXCEPTION_POINTERS* info) {
    log_seh_exception("unhandled", info);
    return EXCEPTION_CONTINUE_SEARCH;
}

} // namespace

extern "C" void parties_native_log_set_callback(PartiesNativeLogCallback callback) {
    std::lock_guard<std::mutex> lock(g_log_mutex);
    g_log_callback = callback;
}

extern "C" void parties_native_seh_install() {
    bool expected = false;
    if (!g_seh_installed.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
        return;
    }

    AddVectoredExceptionHandler(1, parties_vectored_exception_handler);
    SetUnhandledExceptionFilter(parties_unhandled_exception_filter);
    parties_rs::video::native_log_emit(parties_rs::video::NativeLogLevel::Info, "[seh] windows SEH logger installed");
}

namespace parties_rs::video {

void native_log_emit(NativeLogLevel level, const char* message) {
    PartiesNativeLogCallback callback = nullptr;
    {
        std::lock_guard<std::mutex> lock(g_log_mutex);
        callback = g_log_callback;
    }

    if (callback) {
        callback(static_cast<uint8_t>(level), message ? message : "");
    }
}

} // namespace parties_rs::video
