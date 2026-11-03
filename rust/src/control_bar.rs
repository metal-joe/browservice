//! Port of `src/control_bar.{hpp,cpp}`: the control bar at the top of the
//! window (navigation buttons, address field with security status icon and
//! loading bar, bookmark buttons, find button and find bar, clipboard
//! button, quality selector, download button with progress bars).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Instant;

use crate::bookmarks::{Bookmark, Bookmarks, cached_bookmark_id_by_url};
use crate::button::{Button, ButtonEventHandler};
use crate::find_bar::{FindBar, FindBarEventHandler};
use crate::image_slice::ImageSlice;
use crate::menu_button::{MenuButton, MenuButtonEventHandler, MenuButtonIcon, Palette};
use crate::quality_selector::{QualitySelector, QualitySelectorEventHandler};
use crate::text::TextLayout;
use crate::text_field::{TextField, TextFieldEventHandler};
use crate::timeout::Timeout;
use crate::ui::post_task;
use crate::widget::{Widget, WidgetBase, WidgetExt, WidgetParent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityStatus {
    Secure,
    Warning,
    Insecure,
}

pub trait ControlBarEventHandler {
    fn on_address_submitted(&self, url: String);
    fn on_quality_changed(&self, idx: usize);
    fn on_pending_download_accepted(&self);
    fn on_find(&self, text: String, forward: bool, find_next: bool);
    fn on_stop_find(&self, clear_selection: bool);
    fn on_clipboard_button_pressed(&self);
    fn on_open_bookmarks_button_pressed(&self);
    /// -1 = back, 0 = refresh, 1 = forward.
    fn on_navigation_button_pressed(&self, direction: i32);
    fn on_home_button_pressed(&self);
}

const SECURE_ICON_PATTERN: &[&str] = &[
    "WWWWWBBWWWWWW",
    "WWWWBWWBWWWWW",
    "WWWBWWWWBWWWW",
    "WWWBWWWWBWWWW",
    "WWWBWWWWBWWWW",
    "WEEEEEEEEEBWW",
    "WEGGGGGGGGBWW",
    "WEGGGGGGGGBWW",
    "WEGGGGGGGGBWW",
    "WEGGGGGGGGBWW",
    "WEGGGGGGGGBWW",
    "WBBBBBBBBBBWW",
    "WWWWWWWWWWWWW",
];

const WARNING_ICON_PATTERN: &[&str] = &[
    "WWWWWBBWvWWWW",
    "WWWWBWWvYjWWW",
    "WWWBWWWvYjWWW",
    "WWWBWWvYYYjWW",
    "WWWBWWvYBYjWW",
    "WEEEEEvYBYjWW",
    "WEGGGvYYBYYjW",
    "WEGGGvYYBYYjW",
    "WEGGGvYYBYYjW",
    "WEGGvYYYYYYYj",
    "WEGGvYYYBYYYj",
    "WBBBvYYYYYYYj",
    "WWWWyyyyyyyyy",
];

const INSECURE_ICON_PATTERN: &[&str] = &[
    "WWWWWBBWWWRRW",
    "WWWWBWWBWRRRW",
    "WWWBWWWWRRRWW",
    "WWWBWWWRRRWWW",
    "WWWBWWRRRWWWW",
    "WEEEERRRWbBWW",
    "WEGGRRRWgGBWW",
    "WEGRRRWgGGBWW",
    "WERRRWgGGGBWW",
    "WRRRWgGGGGBWW",
    "RRRWgGGGGGBWW",
    "RRWbBBBBBBBWW",
    "WWWWWWWWWWWWW",
];

const GO_ICON_PATTERN: &[&str] = &[
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGBGGGGGG",
    "GGGGGGGGGGGGBBGGGGG",
    "GGGGGGGGGGGGBUBGGGG",
    "GGGccccccccccUuBGGG",
    "GGGcUUUUUUUUUUvvBGG",
    "GGGcUvMMMMMMMMMMdBG",
    "GGGcUMMMMMMMMMMddBG",
    "GGGcMDDDDDDDDMdDBGG",
    "GGGcbbbbbbbbbdDBGGG",
    "GGGGGGGGGGGGBDBGGGG",
    "GGGGGGGGGGGGBBGGGGG",
    "GGGGGGGGGGGGBGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
];

const OPEN_BOOKMARKS_ICON_PATTERN: &[&str] = &[
    "...................",
    "...................",
    "...ggggggggggggg...",
    "...ghwwwwwwwwwwH...",
    "...gchwwDDDDDDDwH..",
    "...gKqssDrrrrr*sH..",
    "...gKkkkDrrrrr*k*..",
    "...gKkkkDrrrrr*k*..",
    "...gKk--Drrrrr*k*..",
    "...gKkkkDrr*rr*k*..",
    "...gKkkkDr*kmr*k*..",
    "...gKk--D*---m*k*..",
    "...gKkkkDkkkkk*k*..",
    "...gKkkkkkkkkkkk*..",
    "...gKk---------k*..",
    "...gKkkkkkkkkkkk*..",
    "....Hkkkkkkkkkkk*..",
    "....*************..",
    "...................",
];

const BOOKMARK_OFF_ICON_PATTERN: &[&str] = &[
    "...................",
    "...................",
    ".....xSSSSSSSSP....",
    ".....Drrrrrrrr*....",
    ".....Drrrrrrrr*....",
    ".....Drrrrrrrr*....",
    ".....Drrrrrrrr*....",
    ".....Drrrrrrrr*....",
    ".....Drrrrrrrr*....",
    ".....Drrrrrrrr*....",
    ".....Drrrrrrrr*....",
    ".....Drrrrrrrr*....",
    ".....Drrr**rrr*....",
    ".....Drr*..mrr*....",
    ".....Dr*....mr*....",
    ".....D*......m*....",
    ".....D........*....",
    "...................",
    "...................",
];

const BOOKMARK_ON_ICON_PATTERN: &[&str] = &[
    "...................",
    "...................",
    "......xxxxxxxg.....",
    ".....gwvvvvvvv*....",
    "....gdg8wwwwwwC*...",
    "....grg8rrrrrrD*...",
    "....grg8rrrrrrE*...",
    "....grg7AAAAAAF*...",
    "....grg6BBBBBBG*...",
    "....grg5CCCCCCH*...",
    "....g*grDDDDDDI*...",
    "......gAEEGEEEJ*...",
    "......gAEG**EEJ*...",
    "......gAG*..mEJ*...",
    "......gA*....mJ*...",
    "......g*......m*...",
    "......g........*...",
    "...................",
    "...................",
];

const FIND_ICON_PATTERN: &[&str] = &[
    "...................",
    "...................",
    "...................",
    ".....AAA....AAA....",
    ".....A3B....A3B....",
    "....ABBBMMMMMBBB...",
    "....ARQPBXXBRQPB...",
    "....ARQPBXXBRQPB...",
    "...A3RQPBBBBRQP1B..",
    "..ABBBBBB..BBBBBBB.",
    "..A4321B....A4321B.",
    "..A4321B....A4321B.",
    "..A4321B....A4321B.",
    "..A4321B....A4321B.",
    "..A4321B....A4321B.",
    "..ARQP1B....ARQP1B.",
    "..ABBBBB....ABBBBB.",
    "...................",
    "...................",
];

const CLIPBOARD_ICON_PATTERN: &[&str] = &[
    "...................",
    "...................",
    "...eeeeeeBBeeeeee..",
    "..e#####BzyB#####E.",
    "..e####BzyyYB####E.",
    "..e#pppBYYYYBppp#E.",
    "..e#pWWBBBBBBWWP#E.",
    "..e#pWWWWWWWWWWP#E.",
    "..e#pWWWWWWWWWWP#E.",
    "..e#pWWWWWWWWWWP#E.",
    "..e#pWWWWWWWWWWP#E.",
    "..e#pWWWWWWqqqqB#E.",
    "..e#pWWWWWWqwwB##E.",
    "..e#pWWWWWWqwB###E.",
    "..e#pWWWWWWqB####E.",
    "..e#pPPPPPPB#####E.",
    "..e##############E.",
    "...EEEEEEEEEEEEEE..",
    "...................",
];

const BACK_ICON_PATTERN: &[&str] = &[
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGBGGGGGGGGGGG",
    "GGGGGGBBGGGGGGGGGGG",
    "GGGGGBUBGGGGGGGGGGG",
    "GGGGBUucccccccccccG",
    "GGGBUuvUUUUUUUUUubG",
    "GGBUvMMMMMMMMMMMdbG",
    "GGGBdhDDDDDDDDDDDbG",
    "GGGGBdDbbbbbbbbbbbG",
    "GGGGGBDBGGGGGGGGGGG",
    "GGGGGGBBGGGGGGGGGGG",
    "GGGGGGGBGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
];

const FORWARD_ICON_PATTERN: &[&str] = &[
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGBGGGGGG",
    "GGGGGGGGGGGGBBGGGGG",
    "GGGGGGGGGGGGBUBGGGG",
    "GGcccccccccccUuBGGG",
    "GGcUUUUUUUUUUUvvBGG",
    "GGcUvMMMMMMMMMMMdBG",
    "GGcMDDDDDDDDDMdDBGG",
    "GGcbbbbbbbbbbdDBGGG",
    "GGGGGGGGGGGGBDBGGGG",
    "GGGGGGGGGGGGBBGGGGG",
    "GGGGGGGGGGGGBGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
    "GGGGGGGGGGGGGGGGGGG",
];

const REFRESH_ICON_PATTERN: &[&str] = &[
    "...................",
    "...................",
    "...................",
    "........////....&..",
    "......//<<<<&&.-X..",
    "...../<||>>|||-mX..",
    "..../<|>###&||||X..",
    "..../<>#....-|||X..",
    ".../<|#....-m|||X..",
    ".../<>#...&XXXXXX..",
    ".../<>#............",
    ".../<>#............",
    ".../<|#....../-&...",
    "....&||&..../<|#...",
    "....&|||////<|>#...",
    ".....&|||<<<|>#....",
    "......##>>>>##.....",
    "........####.......",
    "...................",
];

const HOME_ICON_PATTERN: &[&str] = &[
    "...................",
    "...................",
    "........./.........",
    "......../%&.Ab.....",
    "......./%):&AB.....",
    "....../%):::&B.....",
    "...../%):::::&.....",
    "..../%):::::::&....",
    ".../%):::::::::&...",
    "../%)X:::::::::_&..",
    "...)|X:::PPPP::_.&.",
    "....|X:::Pddp::_...",
    "....|X:::Pddp::_...",
    "....|X:::Pddp::_...",
    "....|X:::Pddp::_...",
    "....|X:::Pddp::_...",
    ".....-:::Pddp::_...",
    ".....___________...",
    "...................",
];

const SECURE_COLORS: Palette = &[
    ('B', [0, 0, 0]),
    ('E', [128, 128, 128]),
    ('G', [192, 192, 192]),
    ('W', [255, 255, 255]),
];

const WARNING_COLORS: Palette = &[
    ('B', [0, 0, 0]),
    ('E', [128, 128, 128]),
    ('G', [192, 192, 192]),
    ('W', [255, 255, 255]),
    ('Y', [255, 255, 0]),
    ('y', [32, 32, 0]),
    ('j', [64, 64, 0]),
    ('v', [128, 128, 0]),
];

const INSECURE_COLORS: Palette = &[
    ('B', [0, 0, 0]),
    ('b', [128, 128, 128]),
    ('E', [128, 128, 128]),
    ('G', [192, 192, 192]),
    ('g', [224, 224, 224]),
    ('W', [255, 255, 255]),
    ('R', [255, 0, 0]),
];

const GO_ACTIVE: Palette = &[
    ('G', [192, 192, 192]),
    ('B', [0, 0, 0]),
    ('b', [32, 32, 32]),
    ('c', [64, 64, 64]),
    ('W', [255, 255, 255]),
    ('U', [120, 255, 120]),
    ('u', [109, 236, 109]),
    ('v', [102, 226, 102]),
    ('M', [96, 216, 96]),
    ('d', [82, 188, 82]),
    ('D', [68, 160, 68]),
];

const GO_PASSIVE: Palette = &[
    ('G', [192, 192, 192]),
    ('B', [0, 0, 0]),
    ('b', [32, 32, 32]),
    ('c', [64, 64, 64]),
    ('W', [255, 255, 255]),
    ('U', [255, 255, 255]),
    ('u', [232, 232, 232]),
    ('v', [214, 214, 214]),
    ('M', [200, 200, 200]),
    ('d', [172, 172, 172]),
    ('D', [144, 144, 144]),
];

const OPEN_BOOKMARKS_ACTIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('*', [0, 0, 0]),
    ('-', [160, 96, 0]),
    ('w', [255, 255, 255]),
    ('r', [255, 128, 128]),
    ('D', [56, 30, 30]),
    ('m', [23, 15, 15]),
    ('k', [192, 128, 0]),
    ('s', [66, 46, 0]),
    ('q', [80, 48, 0]),
    ('K', [128, 64, 0]),
    ('c', [112, 56, 0]),
    ('g', [56, 28, 0]),
    ('h', [88, 46, 0]),
    ('H', [44, 22, 0]),
];

const OPEN_BOOKMARKS_PASSIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('*', [0, 0, 0]),
    ('-', [128, 128, 128]),
    ('w', [255, 255, 255]),
    ('r', [224, 224, 224]),
    ('D', [48, 48, 48]),
    ('m', [24, 24, 24]),
    ('k', [160, 160, 160]),
    ('s', [58, 58, 58]),
    ('q', [60, 60, 60]),
    ('K', [96, 96, 96]),
    ('c', [80, 80, 80]),
    ('g', [48, 48, 48]),
    ('h', [56, 56, 56]),
    ('H', [40, 40, 40]),
];

const BOOKMARK_OFF_ACTIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('*', [0, 0, 0]),
    ('r', [255, 128, 128]),
    ('S', [80, 80, 80]),
    ('P', [48, 48, 48]),
    ('D', [72, 48, 48]),
    ('m', [36, 24, 24]),
    ('x', [76, 64, 64]),
];

const BOOKMARK_OFF_PASSIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('*', [0, 0, 0]),
    ('r', [212, 212, 212]),
    ('S', [96, 96, 96]),
    ('P', [64, 64, 64]),
    ('D', [64, 64, 64]),
    ('m', [32, 32, 32]),
    ('x', [80, 80, 80]),
];

const BOOKMARK_ON_ACTIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('*', [0, 0, 0]),
    ('5', [255, 131, 131]),
    ('6', [255, 134, 134]),
    ('7', [255, 137, 137]),
    ('8', [255, 140, 140]),
    ('r', [255, 128, 128]),
    ('A', [253, 127, 127]),
    ('B', [250, 125, 125]),
    ('C', [247, 124, 124]),
    ('D', [244, 122, 122]),
    ('E', [241, 121, 121]),
    ('F', [238, 119, 119]),
    ('G', [235, 118, 118]),
    ('H', [232, 116, 116]),
    ('I', [229, 115, 115]),
    ('J', [226, 113, 113]),
    ('d', [216, 104, 104]),
    ('2', [228, 112, 112]),
    ('w', [255, 134, 134]),
    ('v', [255, 148, 148]),
    ('g', [96, 48, 48]),
    ('m', [48, 32, 32]),
    ('x', [96, 64, 64]),
    ('p', [48, 32, 32]),
];

const BOOKMARK_ON_PASSIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('*', [0, 0, 0]),
    ('5', [227, 227, 227]),
    ('6', [230, 230, 230]),
    ('7', [233, 233, 233]),
    ('8', [236, 236, 236]),
    ('r', [224, 224, 224]),
    ('A', [221, 221, 221]),
    ('B', [218, 218, 218]),
    ('C', [215, 215, 215]),
    ('D', [212, 212, 212]),
    ('E', [209, 209, 209]),
    ('F', [206, 206, 206]),
    ('G', [203, 203, 203]),
    ('H', [200, 200, 200]),
    ('I', [197, 197, 197]),
    ('J', [194, 194, 194]),
    ('d', [192, 192, 192]),
    ('2', [200, 200, 200]),
    ('w', [230, 230, 230]),
    ('v', [240, 240, 240]),
    ('g', [64, 64, 64]),
    ('m', [40, 40, 40]),
    ('x', [80, 80, 80]),
    ('p', [40, 40, 40]),
];

const FIND_ACTIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('A', [0, 0, 96]),
    ('M', [0, 0, 64]),
    ('B', [0, 0, 0]),
    ('1', [68, 68, 164]),
    ('P', [82, 82, 170]),
    ('2', [96, 96, 176]),
    ('Q', [110, 110, 182]),
    ('3', [124, 124, 188]),
    ('R', [138, 138, 194]),
    ('4', [152, 152, 200]),
    ('X', [80, 80, 160]),
];

const FIND_PASSIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('A', [64, 64, 64]),
    ('M', [48, 48, 48]),
    ('B', [0, 0, 0]),
    ('1', [108, 108, 108]),
    ('P', [122, 122, 122]),
    ('2', [136, 136, 136]),
    ('Q', [150, 150, 150]),
    ('3', [164, 164, 164]),
    ('R', [176, 176, 176]),
    ('4', [188, 188, 188]),
    ('X', [116, 116, 116]),
];

const CLIPBOARD_ACTIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('W', [255, 255, 255]),
    ('w', [240, 240, 240]),
    ('B', [0, 0, 0]),
    ('e', [96, 48, 24]),
    ('E', [64, 32, 16]),
    ('p', [96, 64, 32]),
    ('q', [80, 80, 80]),
    ('P', [32, 32, 32]),
    ('Y', [224, 224, 0]),
    ('y', [240, 240, 0]),
    ('z', [255, 255, 0]),
    ('#', [232, 156, 118]),
];

const CLIPBOARD_PASSIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('W', [255, 255, 255]),
    ('w', [240, 240, 240]),
    ('B', [0, 0, 0]),
    ('e', [64, 64, 64]),
    ('E', [16, 16, 16]),
    ('p', [96, 96, 96]),
    ('q', [96, 96, 96]),
    ('P', [32, 32, 32]),
    ('Y', [224, 224, 224]),
    ('y', [240, 240, 240]),
    ('z', [255, 255, 255]),
    ('#', [192, 192, 192]),
];

const BACK_FORWARD_COLOR_PALETTE: Palette = &[
    ('G', [192, 192, 192]),
    ('B', [0, 0, 0]),
    ('b', [32, 32, 32]),
    ('c', [64, 64, 64]),
    ('U', [117, 250, 250]),
    ('u', [109, 236, 236]),
    ('v', [104, 229, 229]),
    ('M', [100, 221, 221]),
    ('d', [89, 200, 200]),
    ('D', [79, 179, 179]),
    ('h', [94, 211, 211]),
];

const BACK_FORWARD_GREYSCALE_PALETTE: Palette = &[
    ('G', [192, 192, 192]),
    ('B', [0, 0, 0]),
    ('b', [32, 32, 32]),
    ('c', [64, 64, 64]),
    ('U', [247, 247, 247]),
    ('u', [232, 232, 232]),
    ('v', [220, 220, 220]),
    ('M', [211, 211, 211]),
    ('d', [192, 192, 192]),
    ('D', [174, 174, 174]),
    ('h', [202, 202, 202]),
];

const REFRESH_ACTIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('X', [12, 14, 14]),
    ('#', [24, 29, 29]),
    ('&', [36, 43, 43]),
    ('/', [48, 58, 58]),
    ('-', [54, 68, 68]),
    ('<', [114, 243, 243]),
    ('m', [107, 232, 232]),
    ('|', [100, 221, 221]),
    ('>', [85, 190, 190]),
];

const REFRESH_PASSIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('X', [14, 14, 14]),
    ('#', [27, 27, 27]),
    ('&', [41, 41, 41]),
    ('/', [54, 54, 54]),
    ('-', [63, 63, 63]),
    ('<', [238, 238, 238]),
    ('m', [224, 224, 224]),
    ('|', [211, 211, 211]),
    ('>', [183, 183, 183]),
];

const HOME_ACTIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('|', [60, 60, 34]),
    ('-', [54, 54, 32]),
    ('_', [48, 48, 28]),
    (':', [236, 236, 160]),
    ('X', [160, 160, 100]),
    ('d', [248, 176, 48]),
    ('p', [122, 86, 18]),
    ('P', [106, 74, 14]),
    ('/', [72, 72, 72]),
    ('%', [156, 156, 156]),
    ('&', [60, 60, 60]),
    (')', [72, 72, 72]),
    ('A', [92, 74, 56]),
    ('b', [80, 64, 48]),
    ('B', [68, 54, 40]),
];

const HOME_PASSIVE: Palette = &[
    ('.', [192, 192, 192]),
    ('|', [44, 44, 44]),
    ('-', [38, 38, 38]),
    ('_', [32, 32, 32]),
    (':', [226, 226, 226]),
    ('X', [144, 144, 144]),
    ('d', [186, 186, 186]),
    ('p', [84, 84, 84]),
    ('P', [74, 74, 74]),
    ('/', [72, 72, 72]),
    ('%', [156, 156, 156]),
    ('&', [52, 52, 52]),
    (')', [64, 64, 64]),
    ('A', [78, 78, 78]),
    ('b', [68, 68, 68]),
    ('B', [58, 58, 58]),
];


fn security_status_icon(status: SecurityStatus) -> ImageSlice {
    match status {
        SecurityStatus::Secure => ImageSlice::create_image_from_strings(SECURE_ICON_PATTERN, SECURE_COLORS),
        SecurityStatus::Warning => ImageSlice::create_image_from_strings(WARNING_ICON_PATTERN, WARNING_COLORS),
        SecurityStatus::Insecure => {
            ImageSlice::create_image_from_strings(INSECURE_ICON_PATTERN, INSECURE_COLORS)
        }
    }
}

fn icon(pattern: &[&str], active: Palette, passive: Palette) -> MenuButtonIcon {
    MenuButtonIcon::from_pattern(pattern, active, passive)
}

fn bookmark_on_icon() -> MenuButtonIcon {
    icon(BOOKMARK_ON_ICON_PATTERN, BOOKMARK_ON_ACTIVE, BOOKMARK_ON_PASSIVE)
}

fn bookmark_off_icon() -> MenuButtonIcon {
    icon(BOOKMARK_OFF_ICON_PATTERN, BOOKMARK_OFF_ACTIVE, BOOKMARK_OFF_PASSIVE)
}

const BTN_WIDTH: i32 = 22;

/// Horizontal layout of the control bar (`ControlBar::Layout`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub width: i32,

    pub back_button_start: i32,
    pub back_button_end: i32,
    pub forward_button_start: i32,
    pub forward_button_end: i32,
    pub refresh_button_start: i32,
    pub refresh_button_end: i32,
    pub home_button_start: i32,
    pub home_button_end: i32,

    pub addr_text_start: i32,
    pub addr_text_end: i32,
    pub addr_box_start: i32,
    pub addr_box_end: i32,
    pub go_button_start: i32,
    pub go_button_end: i32,
    pub bookmark_toggle_button_start: i32,
    pub bookmark_toggle_button_end: i32,
    pub security_icon_start: i32,
    pub addr_field_start: i32,
    pub addr_field_end: i32,

    pub separator0_pos: i32,
    pub separator1_pos: i32,
    pub separator2_pos: i32,
    pub separator3_pos: i32,
    pub separator0_visible: bool,
    pub separator2_visible: bool,
    pub separator3_visible: bool,

    pub quality_text_start: i32,
    pub quality_text_end: i32,
    pub quality_selector_start: i32,
    pub quality_selector_end: i32,

    pub download_start: i32,
    pub download_end: i32,

    pub open_bookmarks_button_start: i32,
    pub open_bookmarks_button_end: i32,
    pub find_button_start: i32,
    pub find_button_end: i32,
    pub clipboard_button_start: i32,
    pub clipboard_button_end: i32,

    pub find_text_start: i32,
    pub find_text_end: i32,
    pub find_bar_start: i32,
    pub find_bar_end: i32,
}

impl Layout {
    pub fn new(
        width: i32,
        is_quality_selector_visible: bool,
        is_clipboard_button_visible: bool,
        is_download_visible: bool,
        is_find_bar_visible: bool,
        show_soft_navigation_buttons: bool,
    ) -> Layout {
        let mut l = Layout {
            width,
            ..Default::default()
        };
        let content_start = 1;
        let content_end = width - 1;

        const SEPARATOR_WIDTH: i32 = 4;
        const ADDRESS_TEXT_WIDTH: i32 = 52;
        const QUALITY_TEXT_WIDTH: i32 = 46;
        const FIND_TEXT_WIDTH: i32 = 29;

        let separator0_start;
        let separator0_end;
        if show_soft_navigation_buttons {
            l.back_button_start = content_start;
            l.back_button_end = l.back_button_start + BTN_WIDTH;
            l.forward_button_start = l.back_button_end;
            l.forward_button_end = l.forward_button_start + BTN_WIDTH;
            l.refresh_button_start = l.forward_button_end;
            l.refresh_button_end = l.refresh_button_start + BTN_WIDTH;
            l.home_button_start = l.refresh_button_end;
            l.home_button_end = l.home_button_start + BTN_WIDTH;
            separator0_start = l.home_button_end;
            separator0_end = separator0_start + SEPARATOR_WIDTH;
            l.separator0_visible = true;
        } else {
            // (The C++ version leaves the refresh button range uninitialized
            // here; it is not used without soft navigation buttons.)
            l.back_button_start = content_start;
            l.back_button_end = content_start;
            l.forward_button_start = content_start;
            l.forward_button_end = content_start;
            l.refresh_button_start = content_start;
            l.refresh_button_end = content_start;
            l.home_button_start = content_start;
            l.home_button_end = content_start;
            separator0_start = content_start;
            separator0_end = content_start;
            l.separator0_visible = false;
        }

        l.separator0_pos = separator0_start + SEPARATOR_WIDTH / 2;

        let download_width = if is_download_visible { 88 } else { 0 };
        let download_spacer_width = if is_download_visible && is_quality_selector_visible { 2 } else { 0 };

        let separator3_start;
        if is_find_bar_visible {
            l.find_bar_end = content_end;
            l.find_bar_start = l.find_bar_end - FindBar::WIDTH;
            l.find_text_end = l.find_bar_start;
            l.find_text_start = l.find_text_end - FIND_TEXT_WIDTH;
            let separator3_end = l.find_text_start;
            separator3_start = separator3_end - SEPARATOR_WIDTH;
            l.separator3_visible = true;
        } else {
            l.find_text_start = content_end;
            l.find_text_end = content_end;
            l.find_bar_start = content_end;
            l.find_bar_end = content_end;
            separator3_start = content_end;
            l.separator3_visible = false;
        }

        l.separator3_pos = separator3_start + SEPARATOR_WIDTH / 2;

        l.download_end = separator3_start;
        l.download_start = l.download_end - download_width;

        let download_spacer_end = l.download_start;
        let download_spacer_start = download_spacer_end - download_spacer_width;

        l.quality_selector_end = download_spacer_start;
        l.quality_selector_start = if is_quality_selector_visible {
            l.quality_selector_end - QualitySelector::WIDTH
        } else {
            l.quality_selector_end
        };

        l.quality_text_end = l.quality_selector_start;
        l.quality_text_start = if is_quality_selector_visible {
            l.quality_text_end - QUALITY_TEXT_WIDTH
        } else {
            l.quality_text_end
        };

        let separator2_end = l.quality_text_start;
        let separator2_start;
        if is_quality_selector_visible || is_download_visible {
            separator2_start = separator2_end - SEPARATOR_WIDTH;
            l.separator2_visible = true;
        } else {
            separator2_start = separator2_end;
            l.separator2_visible = false;
        }
        l.separator2_pos = separator2_start + SEPARATOR_WIDTH / 2;

        l.clipboard_button_end = separator2_start;
        l.clipboard_button_start = if is_clipboard_button_visible {
            l.clipboard_button_end - BTN_WIDTH
        } else {
            l.clipboard_button_end
        };

        l.find_button_end = l.clipboard_button_start;
        l.find_button_start = l.find_button_end - BTN_WIDTH;

        l.open_bookmarks_button_end = l.find_button_start;
        l.open_bookmarks_button_start = l.open_bookmarks_button_end - BTN_WIDTH;

        let separator1_end = l.open_bookmarks_button_start;
        let separator1_start = separator1_end - SEPARATOR_WIDTH;
        l.separator1_pos = separator1_start + SEPARATOR_WIDTH / 2;

        let addr_start = separator0_end;
        let addr_end = separator1_start;

        l.addr_text_start = addr_start;
        l.addr_text_end = l.addr_text_start + ADDRESS_TEXT_WIDTH;

        l.bookmark_toggle_button_end = addr_end;
        l.bookmark_toggle_button_start = l.bookmark_toggle_button_end - BTN_WIDTH;

        l.go_button_end = l.bookmark_toggle_button_start;
        l.go_button_start = l.go_button_end - BTN_WIDTH;

        l.addr_box_start = l.addr_text_end;
        l.addr_box_end = l.go_button_start - 1;

        let addr_box_inner_start = l.addr_box_start + 2;
        let addr_box_inner_end = l.addr_box_end - 2;

        l.security_icon_start = addr_box_inner_start + 4;
        let security_icon_end = l.security_icon_start + 13;

        l.addr_field_start = security_icon_end + 4;
        l.addr_field_end = addr_box_inner_end;

        l
    }
}

pub struct ControlBar {
    base: WidgetBase,
    self_weak: Weak<ControlBar>,
    event_handler: Weak<dyn ControlBarEventHandler>,

    clipboard_button_enabled: Cell<bool>,
    show_soft_navigation_buttons: bool,

    animation_timeout: Rc<Timeout>,

    addr_text: TextLayout,
    addr_field: Rc<TextField>,

    security_status: Cell<SecurityStatus>,

    back_button: Rc<MenuButton>,
    forward_button: Rc<MenuButton>,
    refresh_button: Rc<MenuButton>,
    home_button: Rc<MenuButton>,
    go_button: Rc<MenuButton>,
    bookmark_toggle_button: Rc<MenuButton>,
    open_bookmarks_button: Rc<MenuButton>,
    find_button: Rc<MenuButton>,
    clipboard_button: Rc<MenuButton>,

    quality_text: TextLayout,
    quality_selector: RefCell<Option<Rc<QualitySelector>>>,

    find_bar_visible: Cell<bool>,
    find_text: TextLayout,
    find_bar: Rc<FindBar>,

    pending_download_count: Cell<i32>,
    download_progress: RefCell<Vec<i32>>,
    download_button: Rc<Button>,

    loading: Cell<bool>,
    loading_animation_start_time: Cell<Option<Instant>>,

    address: RefCell<String>,
    page_title: RefCell<String>,
    bookmark_id: Cell<Option<u64>>,
}

crate::impl_widget_parent_for_widget!(ControlBar);

impl ControlBar {
    pub const HEIGHT: i32 = 27;

    pub fn new(
        widget_parent: Weak<dyn WidgetParent>,
        event_handler: Weak<dyn ControlBarEventHandler>,
        show_soft_navigation_buttons: bool,
    ) -> Rc<ControlBar> {
        Rc::new_cyclic(|self_weak: &Weak<ControlBar>| {
            let parent: Weak<dyn WidgetParent> = self_weak.clone();
            let text_handler: Weak<dyn TextFieldEventHandler> = self_weak.clone();
            let mb_handler: Weak<dyn MenuButtonEventHandler> = self_weak.clone();
            let button_handler: Weak<dyn ButtonEventHandler> = self_weak.clone();
            let find_handler: Weak<dyn FindBarEventHandler> = self_weak.clone();

            let text = |s: &str| {
                let layout = TextLayout::new();
                layout.set_text(s.to_string());
                layout
            };
            let menu_button = |icon| MenuButton::new(icon, parent.clone(), mb_handler.clone());

            let addr_field = TextField::new(parent.clone(), text_handler);
            addr_field.set_allow_empty_submit(false);

            ControlBar {
                base: WidgetBase::new(widget_parent),
                self_weak: self_weak.clone(),
                event_handler,
                clipboard_button_enabled: Cell::new(false),
                show_soft_navigation_buttons,
                animation_timeout: Timeout::create(30),
                addr_text: text("Address"),
                addr_field,
                security_status: Cell::new(SecurityStatus::Insecure),
                back_button: menu_button(icon(
                    BACK_ICON_PATTERN,
                    BACK_FORWARD_COLOR_PALETTE,
                    BACK_FORWARD_GREYSCALE_PALETTE,
                )),
                forward_button: menu_button(icon(
                    FORWARD_ICON_PATTERN,
                    BACK_FORWARD_COLOR_PALETTE,
                    BACK_FORWARD_GREYSCALE_PALETTE,
                )),
                refresh_button: menu_button(icon(REFRESH_ICON_PATTERN, REFRESH_ACTIVE, REFRESH_PASSIVE)),
                home_button: menu_button(icon(HOME_ICON_PATTERN, HOME_ACTIVE, HOME_PASSIVE)),
                go_button: menu_button(icon(GO_ICON_PATTERN, GO_ACTIVE, GO_PASSIVE)),
                bookmark_toggle_button: menu_button(bookmark_off_icon()),
                open_bookmarks_button: menu_button(icon(
                    OPEN_BOOKMARKS_ICON_PATTERN,
                    OPEN_BOOKMARKS_ACTIVE,
                    OPEN_BOOKMARKS_PASSIVE,
                )),
                find_button: menu_button(icon(FIND_ICON_PATTERN, FIND_ACTIVE, FIND_PASSIVE)),
                clipboard_button: menu_button(icon(
                    CLIPBOARD_ICON_PATTERN,
                    CLIPBOARD_ACTIVE,
                    CLIPBOARD_PASSIVE,
                )),
                quality_text: text("Quality"),
                quality_selector: RefCell::new(None),
                find_bar_visible: Cell::new(false),
                find_text: text("Find"),
                find_bar: FindBar::new(parent.clone(), find_handler),
                pending_download_count: Cell::new(0),
                download_progress: RefCell::new(Vec::new()),
                download_button: Button::new(parent.clone(), button_handler),
                loading: Cell::new(false),
                loading_animation_start_time: Cell::new(None),
                address: RefCell::new(String::new()),
                page_title: RefCell::new(String::new()),
                bookmark_id: Cell::new(None),
            }
        })
    }

    fn post_event(&self, func: impl FnOnce(&dyn ControlBarEventHandler) + 'static) {
        let handler = self.event_handler.clone();
        post_task(move || {
            if let Some(handler) = handler.upgrade() {
                func(&*handler);
            }
        });
    }

    pub fn enable_quality_selector(&self, labels: Vec<String>, choice_idx: usize) {
        if self.quality_selector.borrow().is_some() {
            crate::panic_log!("Requirement '!qualitySelector_' failed");
        }
        let parent: Weak<dyn WidgetParent> = self.self_weak.clone();
        let handler: Weak<dyn QualitySelectorEventHandler> = self.self_weak.clone();
        let selector = QualitySelector::new(parent, handler, labels, choice_idx);
        *self.quality_selector.borrow_mut() = Some(selector);
        self.widget_viewport_updated();
        self.signal_view_dirty();
    }

    pub fn enable_clipboard_button(&self) {
        if !self.clipboard_button_enabled.get() {
            self.clipboard_button_enabled.set(true);
            self.widget_viewport_updated();
            self.signal_view_dirty();
        }
    }

    pub fn set_security_status(&self, value: SecurityStatus) {
        if self.security_status.get() != value {
            self.security_status.set(value);
            self.signal_view_dirty();
        }
    }

    pub fn set_address(&self, addr: String) {
        *self.address.borrow_mut() = addr.clone();
        self.set_bookmark_id(cached_bookmark_id_by_url(&addr));
        self.addr_field.set_text(addr);
    }

    pub fn set_page_title(&self, page_title: String) {
        *self.page_title.borrow_mut() = page_title;
    }

    pub fn set_loading(&self, loading: bool) {
        if loading != self.loading.get() {
            self.loading.set(loading);
            self.signal_view_dirty();
        }
    }

    pub fn set_pending_download_count(&self, count: i32) {
        if count < 0 {
            crate::panic_log!("Requirement 'count >= 0' failed");
        }
        if count != self.pending_download_count.get() {
            self.pending_download_count.set(count);

            if count > 0 {
                self.download_button.set_enabled(true);
                self.download_button.set_text(format!("Download ({count})"));
            } else {
                self.download_button.set_enabled(false);
                self.download_button.set_text("Download".to_string());
            }

            self.widget_viewport_updated();
            self.signal_view_dirty();
        }
    }

    pub fn set_download_progress(&self, progress: Vec<i32>) {
        if progress != *self.download_progress.borrow() {
            *self.download_progress.borrow_mut() = progress;
            self.signal_view_dirty();
            self.widget_viewport_updated();
        }
    }

    pub fn open_find_bar(&self) {
        self.find_bar_visible.set(true);
        self.find_bar.open();
        self.find_bar.activate();
        self.widget_viewport_updated();
        self.signal_view_dirty();
    }

    pub fn find_next(&self) {
        if self.find_bar_visible.get() {
            self.find_bar.find_next();
        }
    }

    pub fn set_find_result(&self, found: bool) {
        if self.find_bar_visible.get() {
            self.find_bar.set_find_result(found);
        }
    }

    pub fn activate_address(&self) {
        self.addr_field.activate();
    }

    fn is_download_visible(&self) -> bool {
        self.pending_download_count.get() > 0 || !self.download_progress.borrow().is_empty()
    }

    /// The current layout (for tests and image comparisons).
    pub fn layout(&self) -> Layout {
        Layout::new(
            self.viewport().width(),
            self.quality_selector.borrow().is_some(),
            self.clipboard_button_enabled.get(),
            self.is_download_visible(),
            self.find_bar_visible.get(),
            self.show_soft_navigation_buttons,
        )
    }

    fn set_bookmark_id(&self, bookmark_id: Option<u64>) {
        self.bookmark_id.set(bookmark_id);
        self.bookmark_toggle_button.set_icon(if bookmark_id.is_some() {
            bookmark_on_icon()
        } else {
            bookmark_off_icon()
        });
    }

    fn toggle_bookmark(&self) {
        let Some(mut bookmarks) = Bookmarks::load() else { return };
        if let Some(id) = self.bookmark_id.get() {
            bookmarks.remove_bookmark(id);
            if bookmarks.save() {
                self.set_bookmark_id(None);
            }
        } else {
            let address = self.address.borrow().clone();
            if address != "browservice://bookmarks/" {
                let mut title = self.page_title.borrow().clone();
                if title.is_empty() {
                    title = address.clone();
                }
                let time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let id = bookmarks.put_bookmark(Bookmark {
                    url: address,
                    title,
                    time,
                });
                if bookmarks.save() {
                    self.set_bookmark_id(Some(id));
                }
            }
        }
    }
}

impl TextFieldEventHandler for ControlBar {
    fn on_text_field_submitted(&self, text: String) {
        self.post_event(move |h| h.on_address_submitted(text));
    }
}

impl MenuButtonEventHandler for ControlBar {
    fn on_menu_button_pressed(&self, button: Weak<MenuButton>) {
        if self.go_button.is(&button) {
            let text = self.addr_field.text();
            self.post_event(move |h| h.on_address_submitted(text));
        }

        if self.bookmark_toggle_button.is(&button) && !self.address.borrow().is_empty() {
            self.toggle_bookmark();
        }

        if self.open_bookmarks_button.is(&button) {
            self.post_event(|h| h.on_open_bookmarks_button_pressed());
        }

        if self.find_button.is(&button) {
            self.open_find_bar();
        }

        if self.clipboard_button.is(&button) && self.clipboard_button_enabled.get() {
            self.post_event(|h| h.on_clipboard_button_pressed());
        }

        if self.back_button.is(&button) {
            self.post_event(|h| h.on_navigation_button_pressed(-1));
        }
        if self.forward_button.is(&button) {
            self.post_event(|h| h.on_navigation_button_pressed(1));
        }
        if self.refresh_button.is(&button) {
            self.post_event(|h| h.on_navigation_button_pressed(0));
        }
        if self.home_button.is(&button) {
            self.post_event(|h| h.on_home_button_pressed());
        }
    }
}

impl QualitySelectorEventHandler for ControlBar {
    fn on_quality_changed(&self, idx: usize) {
        self.post_event(move |h| h.on_quality_changed(idx));
    }
}

impl ButtonEventHandler for ControlBar {
    fn on_button_pressed(&self) {
        self.post_event(|h| h.on_pending_download_accepted());
    }
}

impl FindBarEventHandler for ControlBar {
    fn on_find_bar_close(&self) {
        if self.find_bar_visible.get() {
            self.find_bar_visible.set(false);
            self.widget_viewport_updated();
            self.signal_view_dirty();
        }
    }

    fn on_find(&self, text: String, forward: bool, find_next: bool) {
        if self.find_bar_visible.get() {
            self.post_event(move |h| h.on_find(text, forward, find_next));
        }
    }

    fn on_stop_find(&self, clear_selection: bool) {
        self.post_event(move |h| h.on_stop_find(clear_selection));
    }
}

impl Widget for ControlBar {
    fn base(&self) -> &WidgetBase {
        &self.base
    }

    fn widget_viewport_updated(&self) {
        let viewport = self.viewport();
        let l = self.layout();
        let h = Self::HEIGHT;

        if self.show_soft_navigation_buttons {
            self.back_button
                .set_viewport(viewport.sub_rect(l.back_button_start, l.back_button_end, 1, h - 4));
            self.forward_button
                .set_viewport(viewport.sub_rect(l.forward_button_start, l.forward_button_end, 1, h - 4));
            self.refresh_button
                .set_viewport(viewport.sub_rect(l.refresh_button_start, l.refresh_button_end, 1, h - 4));
            self.home_button
                .set_viewport(viewport.sub_rect(l.home_button_start, l.home_button_end, 1, h - 4));
        }
        self.addr_field
            .set_viewport(viewport.sub_rect(l.addr_field_start, l.addr_field_end, 3, h - 8));
        self.go_button
            .set_viewport(viewport.sub_rect(l.go_button_start, l.go_button_end, 1, h - 4));
        self.bookmark_toggle_button.set_viewport(viewport.sub_rect(
            l.bookmark_toggle_button_start,
            l.bookmark_toggle_button_end,
            1,
            h - 4,
        ));
        self.open_bookmarks_button.set_viewport(viewport.sub_rect(
            l.open_bookmarks_button_start,
            l.open_bookmarks_button_end,
            1,
            h - 4,
        ));
        self.find_button
            .set_viewport(viewport.sub_rect(l.find_button_start, l.find_button_end, 1, h - 4));
        let quality_selector = self.quality_selector.borrow().clone();
        if let Some(quality_selector) = quality_selector {
            quality_selector.set_viewport(viewport.sub_rect(
                l.quality_selector_start,
                l.quality_selector_end,
                1,
                h - 4,
            ));
        }
        if self.clipboard_button_enabled.get() {
            self.clipboard_button.set_viewport(viewport.sub_rect(
                l.clipboard_button_start,
                l.clipboard_button_end,
                1,
                h - 4,
            ));
        }
        if self.is_download_visible() {
            let bar = if self.download_progress.borrow().is_empty() { 0 } else { 5 };
            self.download_button
                .set_viewport(viewport.sub_rect(l.download_start, l.download_end, 1, h - 4 - bar));
        }
        if self.find_bar_visible.get() {
            self.find_bar
                .set_viewport(viewport.sub_rect(l.find_bar_start, l.find_bar_end, 1, h - 4));
        } else {
            self.find_bar.set_viewport(ImageSlice::default());
        }
    }

    fn widget_render(&self) {
        self.animation_timeout.clear(false);

        let viewport = self.viewport();
        let l = self.layout();
        let h = Self::HEIGHT;
        let w = l.width;

        // Frame
        viewport.fill(0, w - 1, 0, 1, 255);
        viewport.fill(0, 1, 1, h - 4, 255);
        viewport.fill(w - 1, w, 0, h - 3, 128);
        viewport.fill(0, w - 1, h - 4, h - 3, 128);
        viewport.fill(0, w, h - 3, h - 2, 255);
        viewport.fill(0, w, h - 2, h - 1, 128);
        viewport.fill_rgb(0, w, h - 1, h, 0, 0, 0);

        // Background
        viewport.fill(1, w - 1, 1, h - 4, 192);

        // "Address" text
        self.addr_text
            .render(&viewport.sub_rect(l.addr_text_start, l.addr_text_end, 1, h - 4), 3, -4, 0);

        // Address field frame
        let (bs, be) = (l.addr_box_start, l.addr_box_end);
        viewport.fill(bs, be - 1, 1, 2, 128);
        viewport.fill(bs, bs + 1, 2, h - 5, 128);
        viewport.fill(be - 1, be, 1, h - 4, 255);
        viewport.fill(bs, be - 1, h - 5, h - 4, 255);
        viewport.fill(bs + 1, be - 2, 2, 3, 0);
        viewport.fill(bs + 1, bs + 2, 3, h - 6, 0);
        viewport.fill(be - 2, be - 1, 2, h - 5, 192);
        viewport.fill(bs + 1, be - 2, h - 6, h - 5, 192);

        // Address field background
        viewport.fill(bs + 2, be - 2, 3, h - 6, 255);

        // Security icon
        viewport.put_image(&security_status_icon(self.security_status.get()), l.security_icon_start, 6);

        // Separators
        let separator = |pos: i32| {
            viewport.fill(pos - 1, pos, 1, h - 4, 128);
            viewport.fill(pos, pos + 1, 1, h - 4, 255);
        };
        if l.separator0_visible {
            separator(l.separator0_pos);
        }
        separator(l.separator1_pos);
        if l.separator2_visible {
            separator(l.separator2_pos);
        }
        if l.separator3_visible {
            separator(l.separator3_pos);
        }

        // "Quality" text
        if self.quality_selector.borrow().is_some() {
            self.quality_text.render(
                &viewport.sub_rect(l.quality_text_start, l.quality_text_end, 1, h - 4),
                3,
                -4,
                0,
            );
        }

        // "Find" text
        if self.find_bar_visible.get() {
            self.find_text
                .render(&viewport.sub_rect(l.find_text_start, l.find_text_end, 1, h - 4), 3, -4, 0);
        }

        // Download progress bar
        let download_progress = self.download_progress.borrow().clone();
        if !download_progress.is_empty() {
            let start_y = h - 9;
            let end_y = h - 4;

            let mut start_x = l.download_start;
            let download_count = download_progress.len() as i32;
            let range_length = l.download_end - l.download_start;
            for (i, &progress) in download_progress.iter().enumerate() {
                let i = i as i32;
                let end_x = l.download_start + ((i + 1) * range_length + download_count - 1) / download_count;
                let bar_max_x = end_x - if i == download_count - 1 { 0 } else { 1 };
                let bar_end_x = start_x + progress * (bar_max_x - start_x) / 100;
                viewport.fill_rgb(start_x, bar_end_x, start_y, end_y, 0, 0, 255);
                viewport.fill(bar_end_x, end_x, start_y, end_y, 192);

                start_x = end_x;
            }
        }

        // Loading bar
        if self.loading.get() {
            let elapsed: i64 = match self.loading_animation_start_time.get() {
                Some(start) => start.elapsed().as_millis() as i64,
                None => {
                    self.loading_animation_start_time.set(Some(Instant::now()));
                    0
                }
            };

            let slice = viewport.sub_rect(l.addr_field_start - 2, l.addr_box_end - 2, h - 7, h - 6);

            if !slice.is_empty() {
                let sw = slice.width();
                let bar_width = sw / 12;
                let p = elapsed * sw as i64 / 5000;
                let bar_start = (p % sw as i64) as i32;
                let bar_end = ((p + bar_width as i64) % sw as i64) as i32;

                if bar_start <= bar_end {
                    slice.fill_rgb(bar_start, bar_end, 0, slice.height(), 0, 0, 255);
                } else {
                    slice.fill_rgb(0, bar_end, 0, slice.height(), 0, 0, 255);
                    slice.fill_rgb(bar_start, sw, 0, slice.height(), 0, 0, 255);
                }
            }

            let self_weak = self.self_weak.clone();
            self.animation_timeout.set(move || {
                if let Some(this) = self_weak.upgrade() {
                    this.signal_view_dirty();
                }
            });
        } else {
            self.loading_animation_start_time.set(None);
        }
    }

    fn widget_list_children(&self) -> Vec<Rc<dyn Widget>> {
        let mut children: Vec<Rc<dyn Widget>> = vec![
            self.addr_field.clone(),
            self.go_button.clone(),
            self.bookmark_toggle_button.clone(),
            self.open_bookmarks_button.clone(),
            self.find_button.clone(),
        ];
        if self.show_soft_navigation_buttons {
            children.push(self.back_button.clone());
            children.push(self.forward_button.clone());
            children.push(self.refresh_button.clone());
            children.push(self.home_button.clone());
        }
        if let Some(quality_selector) = self.quality_selector.borrow().clone() {
            children.push(quality_selector);
        }
        if self.clipboard_button_enabled.get() {
            children.push(self.clipboard_button.clone());
        }
        if self.is_download_visible() {
            children.push(self.download_button.clone());
        }
        if self.find_bar_visible.get() {
            children.push(self.find_bar.clone());
        }
        children
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_with_quality_selector_and_clipboard_button() {
        let l = Layout::new(641, true, true, false, false, false);
        assert_eq!((l.quality_selector_start, l.quality_selector_end), (592, 640));
        assert_eq!((l.quality_text_start, l.quality_text_end), (546, 592));
        assert!(l.separator2_visible && !l.separator0_visible && !l.separator3_visible);
        assert_eq!(l.separator2_pos, 544);
        assert_eq!((l.clipboard_button_start, l.clipboard_button_end), (520, 542));
        assert_eq!((l.find_button_start, l.find_button_end), (498, 520));
        assert_eq!((l.open_bookmarks_button_start, l.open_bookmarks_button_end), (476, 498));
        assert_eq!(l.separator1_pos, 474);
        assert_eq!((l.addr_text_start, l.addr_text_end), (1, 53));
        assert_eq!((l.bookmark_toggle_button_start, l.bookmark_toggle_button_end), (450, 472));
        assert_eq!((l.go_button_start, l.go_button_end), (428, 450));
        assert_eq!((l.addr_box_start, l.addr_box_end), (53, 427));
        assert_eq!(l.security_icon_start, 59);
        assert_eq!((l.addr_field_start, l.addr_field_end), (76, 425));
    }

    #[test]
    fn layout_with_everything() {
        let l = Layout::new(800, true, true, true, true, true);
        assert_eq!((l.back_button_start, l.home_button_end), (1, 89));
        assert_eq!(l.separator0_pos, 91);
        assert_eq!((l.find_bar_start, l.find_bar_end), (619, 799));
        assert_eq!((l.find_text_start, l.find_text_end), (590, 619));
        assert_eq!(l.separator3_pos, 588);
        assert_eq!((l.download_start, l.download_end), (498, 586));
        assert_eq!((l.quality_selector_start, l.quality_selector_end), (448, 496));
        assert_eq!(l.addr_text_start, 93);
    }
}
