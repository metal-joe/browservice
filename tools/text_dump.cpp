// Reference dump of the C++ text renderer for comparison with the Rust port
// (rust/src/text.rs, test text::dump::dump). This program repeats the Pango
// and FreeType calls of src/text.cpp (TextRenderContext, TextLayout::width,
// height, indexToXCoord and render with offset (0, 0)) without the rest of
// Browservice, and prints the same format as the Rust test.
//
// Build and run:
//   g++ -std=c++17 -O2 tools/text_dump.cpp -o text_dump \
//       `pkg-config --cflags --libs pangoft2`
//   ./text_dump [input file with one text per line] > cpp.txt

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

#include <freetype/fttypes.h>
#include <pango/pangoft2.h>

using namespace std;

static string quote(const string& s) {
    // Same as Rust's {:?} for printable ASCII without quotes or backslashes.
    return "\"" + s + "\"";
}

int main(int argc, char** argv) {
    vector<string> lines;
    if(argc >= 2) {
        ifstream in(argv[1]);
        string line;
        while(getline(in, line)) lines.push_back(line);
    } else {
        lines = {"Address", "Quality", "Find", "Download", "Download (1)"};
    }

    setenv("FREETYPE_PROPERTIES", "truetype:interpreter-version=35", true);
    PangoFontMap* fontMap = pango_ft2_font_map_new();
    unsetenv("FREETYPE_PROPERTIES");
    pango_ft2_font_map_set_default_substitute(
        (PangoFT2FontMap*)fontMap,
        [](FcPattern* pattern, void*) {
            FcValue value;
            value.type = FcTypeBool;
            value.u.b = FcFalse;
            FcPatternAdd(pattern, "antialias", value, FcFalse);
            value.u.b = FcFalse;
            FcPatternAdd(pattern, "autohint", value, FcFalse);
            value.u.b = FcTrue;
            FcPatternAdd(pattern, "hinting", value, FcFalse);
        },
        nullptr,
        [](void*) {}
    );
    pango_ft2_font_map_set_resolution((PangoFT2FontMap*)fontMap, 72, 72);
    PangoContext* pangoCtx = pango_font_map_create_context(fontMap);
    pango_context_set_base_dir(pangoCtx, PANGO_DIRECTION_LTR);
    pango_context_set_language(pangoCtx, pango_language_from_string("en-US"));
    PangoFontDescription* fontDesc = pango_font_description_from_string("Verdana 11");

    PangoLayout* layout = pango_layout_new(pangoCtx);
    pango_layout_set_font_description(layout, fontDesc);
    pango_layout_set_auto_dir(layout, FALSE);
    pango_layout_set_single_paragraph_mode(layout, TRUE);

    for(const string& text : lines) {
        pango_layout_set_text(layout, text.data(), (int)text.size());

        PangoRectangle extents;
        pango_layout_get_pixel_extents(layout, nullptr, &extents);
        extents.width = max(extents.width, 1);
        extents.height = max(extents.height, 1);
        int w = extents.width;
        int h = extents.height;
        printf("text %s width %d height %d\n", quote(text).c_str(), w, h);

        printf("cursor");
        for(size_t i = 0; i <= text.size(); ++i) {
            if(i < text.size() && ((unsigned char)text[i] & 0xC0) == 0x80) continue;
            PangoRectangle rect;
            pango_layout_get_cursor_pos(layout, (int)i, &rect, nullptr);
            printf(" %d", rect.x / PANGO_SCALE);
        }
        printf("\n");

        vector<uint8_t> buffer(w * h);
        FT_Bitmap ftBitmap;
        memset(&ftBitmap, 0, sizeof(ftBitmap));
        ftBitmap.rows = h;
        ftBitmap.width = w;
        ftBitmap.pitch = w;
        ftBitmap.buffer = buffer.data();
        ftBitmap.pixel_mode = FT_PIXEL_MODE_GRAY;
        pango_ft2_render_layout(&ftBitmap, layout, -extents.x, -extents.y);

        for(int y = 0; y < h; ++y) {
            for(int x = 0; x < w; ++x) {
                putchar(buffer[y * w + x] >= 128 ? '#' : '.');
            }
            putchar('\n');
        }
    }

    g_object_unref(layout);
    pango_font_description_free(fontDesc);
    g_object_unref(pangoCtx);
    g_object_unref(fontMap);
    return 0;
}
