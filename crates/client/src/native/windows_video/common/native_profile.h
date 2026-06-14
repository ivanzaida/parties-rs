#pragma once

#include "video_types.h"

#include <algorithm>
#include <chrono>
#include <cstdlib>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

namespace parties_rs::video {

namespace native_profile_detail {

struct Stats {
    uint64_t count = 0;
    std::chrono::nanoseconds total{0};
    std::chrono::nanoseconds max{0};
};

inline bool truthy(const char* value) {
    if (!value) {
        return false;
    }
    const std::string text(value);
    return text == "1" || text == "true" || text == "TRUE" || text == "yes" || text == "YES" ||
           text == "on" || text == "ON";
}

inline bool enabled() {
    static const bool is_enabled = truthy(std::getenv("PARTIES_PROFILE")) || truthy(std::getenv("PARTIES_NATIVE_PROFILE"));
    return is_enabled;
}

inline std::chrono::milliseconds interval() {
    const char* value = std::getenv("PARTIES_PROFILE_INTERVAL_MS");
    if (!value) {
        return std::chrono::milliseconds(2000);
    }
    const int millis = std::atoi(value);
    return std::chrono::milliseconds(millis > 0 ? millis : 2000);
}

inline void record(const char* name, std::chrono::nanoseconds elapsed) {
    struct State {
        std::mutex mutex;
        std::unordered_map<std::string, Stats> stats;
        std::chrono::steady_clock::time_point last_log = std::chrono::steady_clock::now();
    };
    static State state;

    std::vector<std::pair<std::string, Stats>> summaries;
    {
        std::lock_guard<std::mutex> lock(state.mutex);
        Stats& stat = state.stats[name];
        stat.count += 1;
        stat.total += elapsed;
        stat.max = (std::max)(stat.max, elapsed);

        const auto now = std::chrono::steady_clock::now();
        if (now - state.last_log < interval()) {
            return;
        }
        state.last_log = now;
        summaries.reserve(state.stats.size());
        for (const auto& entry : state.stats) {
            summaries.push_back(entry);
        }
        state.stats.clear();
    }

    for (const auto& [span_name, stat] : summaries) {
        const double total_ms = std::chrono::duration<double, std::milli>(stat.total).count();
        const double avg_ms = total_ms / static_cast<double>((std::max<uint64_t>)(stat.count, 1));
        const double max_ms = std::chrono::duration<double, std::milli>(stat.max).count();
        native_log_info("[native/profile] {}: count={} avg={}ms max={}ms", span_name, stat.count, avg_ms, max_ms);
    }
}

} // namespace native_profile_detail

class NativeProfileSpan {
public:
    explicit NativeProfileSpan(const char* name)
        : name_(name), enabled_(native_profile_detail::enabled()), started_at_(std::chrono::steady_clock::now()) {}

    ~NativeProfileSpan() {
        if (!enabled_) {
            return;
        }
        native_profile_detail::record(name_, std::chrono::steady_clock::now() - started_at_);
    }

private:
    const char* name_;
    bool enabled_;
    std::chrono::steady_clock::time_point started_at_;
};

} // namespace parties_rs::video
