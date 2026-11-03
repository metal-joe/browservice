#pragma once

#include "browser_font_render_mode.hpp"

namespace browservice {

extern const char* BrowserviceVersion;

class Config {
SHARED_ONLY_CLASS(Config);
private:
    class Src;
    int dummy_;

public:
    Config(CKey, Src& src);

    // Returns empty pointer if reading the configuration failed or help/version/credits
    // was shown and the program should be terminated. In the latter case exitSuccess is
    // set to true (the program should exit with status 0); otherwise it is set to false.
    static shared_ptr<Config> read(int argc, const char* const* argv, bool& exitSuccess);
    static shared_ptr<Config> read(int argc, const wchar_t* const* argv, bool& exitSuccess);

public:
    const vector<pair<string, string>> viceOpts;
    const string vicePlugin;
    const string userAgent;
#ifndef _WIN32
    const bool useDedicatedXvfb;
#endif
    const bool blockFileScheme;
    const string startPage;
    const string dataDir;
    const int windowLimit;
    const vector<pair<string, optional<string>>> chromiumArgs;
    const optional<bool> showSoftNavigationButtons;
    const double initialZoom;
    const BrowserFontRenderMode browserFontRenderMode;
    const set<string> certificateCheckExceptions;
    const bool showControlBar;
};

}
