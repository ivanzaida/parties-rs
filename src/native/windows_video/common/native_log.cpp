#include "video_types.h"

#include <mutex>

extern "C" {
using PartiesNativeLogCallback = void (*)(uint8_t level, const char* message);
}

namespace {

std::mutex g_log_mutex;
PartiesNativeLogCallback g_log_callback = nullptr;

} // namespace

extern "C" void parties_native_log_set_callback(PartiesNativeLogCallback callback) {
    std::lock_guard<std::mutex> lock(g_log_mutex);
    g_log_callback = callback;
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
