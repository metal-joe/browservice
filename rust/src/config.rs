//! Command line configuration. Port of `src/config.{hpp,cpp}` and
//! `src/config_defs.hpp`: the same options with the same names, defaults,
//! validation rules, help text and error messages.

use std::collections::{BTreeSet, HashMap};
use std::io::Write;

use crate::browser_font_render_mode::{
    BrowserFontRenderMode, browser_font_render_mode_description, list_browser_font_render_modes,
};
use crate::credits::CREDITS;
use crate::vice::VicePlugin;

pub const BROWSERVICE_VERSION: &str = "0.9.12.3";

/// The CEF version the program is built against (CEF_VERSION in cef_version.h).
pub fn cef_version() -> &'static str {
    let bytes = cef::sys::CEF_VERSION;
    std::str::from_utf8(&bytes[..bytes.len() - 1]).expect("CEF_VERSION is ASCII")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub vice_opts: Vec<(String, String)>,
    pub vice_plugin: String,
    pub user_agent: String,
    #[cfg(not(windows))]
    pub use_dedicated_xvfb: bool,
    pub block_file_scheme: bool,
    pub start_page: String,
    pub data_dir: String,
    pub window_limit: i32,
    pub chromium_args: Vec<(String, Option<String>)>,
    pub show_soft_navigation_buttons: Option<bool>,
    pub initial_zoom: f64,
    pub browser_font_render_mode: BrowserFontRenderMode,
    pub certificate_check_exceptions: BTreeSet<String>,
    pub show_control_bar: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            vice_opts: Vec::new(),
            vice_plugin: if cfg!(windows) {
                "retrojsvice.dll".to_string()
            } else {
                "retrojsvice.so".to_string()
            },
            user_agent: String::new(),
            #[cfg(not(windows))]
            use_dedicated_xvfb: true,
            block_file_scheme: true,
            start_page: "about:blank".to_string(),
            data_dir: String::new(),
            window_limit: 32,
            chromium_args: Vec::new(),
            show_soft_navigation_buttons: None,
            initial_zoom: 1.0,
            browser_font_render_mode: BrowserFontRenderMode::AntiAliasing,
            certificate_check_exceptions: BTreeSet::new(),
            show_control_bar: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Value parsers (OptParser<T> in config.cpp).
// ---------------------------------------------------------------------------

/// `isspace` in the C locale.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

pub fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "1" | "yes" | "true" | "enable" | "enabled" => Some(true),
        "0" | "no" | "false" | "disable" | "disabled" => Some(false),
        _ => None,
    }
}

pub fn parse_optional_bool(s: &str) -> Option<Option<bool>> {
    match s.to_ascii_lowercase().as_str() {
        "auto" | "automatic" | "autodetect" => Some(None),
        other => parse_bool(other).map(Some),
    }
}

/// Equivalent of `parseString<int>`: `istream >> int` must succeed and consume
/// the whole string. Leading whitespace is skipped, an optional sign is
/// accepted and out of range values are rejected.
pub fn parse_int(s: &str) -> Option<i32> {
    let t = s.trim_start_matches(|c: char| c.is_ascii() && is_c_space(c as u8));
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Rust's parser accepts a leading '+' or '-' just like istream.
    t.parse::<i32>().ok()
}

/// Equivalent of `parseString<double>`: `istream >> double` must succeed and
/// consume the whole string. Accepts decimal floating point notation with
/// optional sign and exponent after optional leading whitespace (no "inf",
/// "nan" or hexadecimal floats, which libstdc++ rejects too).
pub fn parse_double(s: &str) -> Option<f64> {
    let t = s.trim_start_matches(|c: char| c.is_ascii() && is_c_space(c as u8));
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut mantissa_digits = i - int_start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        mantissa_digits += i - frac_start;
    }
    if mantissa_digits == 0 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    let val: f64 = t.parse().ok()?;
    // istream sets failbit on overflow.
    if val.is_infinite() {
        return None;
    }
    Some(val)
}

pub fn parse_browser_font_render_mode(s: &str) -> Option<BrowserFontRenderMode> {
    let s = s.to_ascii_lowercase();
    list_browser_font_render_modes()
        .into_iter()
        .find(|(_, name)| *name == s)
        .map(|(mode, _)| mode)
}

pub fn parse_chromium_args(s: &str) -> Option<Vec<(String, Option<String>)>> {
    if s.is_empty() {
        return Some(Vec::new());
    }
    let mut ret = Vec::new();
    for item in s.split(',') {
        let (mut name, value) = match item.find('=') {
            Some(eq) => (&item[..eq], Some(item[eq + 1..].to_string())),
            None => (item, None),
        };
        for _ in 0..2 {
            if let Some(rest) = name.strip_prefix('-') {
                name = rest;
            }
        }
        if name.is_empty() {
            return None;
        }
        ret.push((name.to_string(), value));
    }
    Some(ret)
}

pub fn parse_certificate_check_exceptions(s: &str) -> Option<BTreeSet<String>> {
    if s.is_empty() {
        return Some(BTreeSet::new());
    }
    let mut ret = BTreeSet::new();
    for item in s.split(',') {
        if item.is_empty() {
            return None;
        }
        ret.insert(item.to_string());
    }
    Some(ret)
}

// ---------------------------------------------------------------------------
// Default value formatting (DefaultValFormatter<T> in config.cpp).
// ---------------------------------------------------------------------------

fn format_bool(val: bool) -> String {
    if val { "yes" } else { "no" }.to_string()
}

fn format_optional_bool(val: Option<bool>) -> String {
    match val {
        Some(v) => format_bool(v),
        None => "auto".to_string(),
    }
}

fn format_browser_font_render_mode(val: BrowserFontRenderMode) -> String {
    list_browser_font_render_modes()
        .into_iter()
        .find(|(mode, _)| *mode == val)
        .map(|(_, name)| name.to_string())
        .expect("Invalid browser font render mode")
}

/// Formats a double like `ostream << double` with default flags (equivalent
/// to printf "%g" with precision 6).
pub fn format_double(val: f64) -> String {
    if val.is_nan() {
        return "nan".to_string();
    }
    if val.is_infinite() {
        return if val < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    if val == 0.0 {
        return if val.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    const P: i32 = 6;
    // Round to P significant digits first to determine the exponent.
    let sci = format!("{:.*e}", (P - 1) as usize, val);
    let (mantissa, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let strip = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if !(-4..P).contains(&exp) {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", strip(mantissa), sign, exp.abs())
    } else {
        let decimals = (P - 1 - exp).max(0) as usize;
        strip(&format!("{:.*}", decimals, val))
    }
}

// ---------------------------------------------------------------------------
// Option table (config_defs.hpp).
// ---------------------------------------------------------------------------

struct OptInfo {
    name: &'static str,
    val_spec: &'static str,
    desc: fn() -> String,
    default_val_str: fn() -> String,
    /// Parses and validates the value and stores it on success.
    apply: fn(&mut Config, &str) -> bool,
}

fn set_if<T>(slot: &mut T, val: Option<T>, validate: impl FnOnce(&T) -> bool) -> bool {
    match val {
        Some(v) if validate(&v) => {
            *slot = v;
            true
        }
        _ => false,
    }
}

fn default_str(s: impl AsRef<str>) -> String {
    format!("default: {}", s.as_ref())
}

fn opt_infos() -> Vec<OptInfo> {
    let mut opts = vec![
        OptInfo {
            name: "vice-plugin",
            val_spec: "FILENAME",
            desc: || "vice plugin to use for the user interface".to_string(),
            default_val_str: || default_str(Config::default().vice_plugin),
            apply: |c, v| set_if(&mut c.vice_plugin, Some(v.to_string()), |v| !v.is_empty()),
        },
        OptInfo {
            name: "user-agent",
            val_spec: "STRING",
            desc: || "value for the User-Agent headers sent by the embedded browser".to_string(),
            default_val_str: || "default determined by CEF".to_string(),
            apply: |c, v| set_if(&mut c.user_agent, Some(v.to_string()), |v| !v.is_empty()),
        },
    ];
    #[cfg(not(windows))]
    opts.push(OptInfo {
        name: "use-dedicated-xvfb",
        val_spec: "YES/NO",
        desc: || {
            concat!(
                "if enabled, the browser is run in its own Xvfb X server; ",
                "otherwise, the browser shares the X session of the environment"
            )
            .to_string()
        },
        default_val_str: || default_str(format_bool(Config::default().use_dedicated_xvfb)),
        apply: |c, v| set_if(&mut c.use_dedicated_xvfb, parse_bool(v), |_| true),
    });
    opts.extend([
        OptInfo {
            name: "block-file-scheme",
            val_spec: "YES/NO",
            desc: || {
                concat!(
                    "if enabled, attempts to access local files through the file:// URI scheme are blocked ",
                    "(WARNING: there may be ways around the block; do NOT allow untrusted users to access Browservice)"
                )
                .to_string()
            },
            default_val_str: || default_str(format_bool(Config::default().block_file_scheme)),
            apply: |c, v| set_if(&mut c.block_file_scheme, parse_bool(v), |_| true),
        },
        OptInfo {
            name: "start-page",
            val_spec: "URL",
            desc: || "URL of the initial page for each new window".to_string(),
            default_val_str: || default_str(Config::default().start_page),
            apply: |c, v| set_if(&mut c.start_page, Some(v.to_string()), |v| !v.is_empty()),
        },
        OptInfo {
            name: "data-dir",
            val_spec: "PATH",
            desc: || {
                "absolute path to a directory that will be used to store data such as cookies and cache; if empty, the browser runs in incognito mode".to_string()
            },
            default_val_str: || "default empty".to_string(),
            apply: |c, v| set_if(&mut c.data_dir, Some(v.to_string()), |_| true),
        },
        OptInfo {
            name: "window-limit",
            val_spec: "COUNT",
            desc: || "maximum number of browser windows that can be open at the same time".to_string(),
            default_val_str: || default_str(Config::default().window_limit.to_string()),
            apply: |c, v| set_if(&mut c.window_limit, parse_int(v), |&v| v >= 1),
        },
        OptInfo {
            name: "chromium-args",
            val_spec: "NAME(=VAL),...",
            desc: || "comma-separated extra arguments to be forwarded to Chromium".to_string(),
            default_val_str: || "default empty".to_string(),
            apply: |c, v| set_if(&mut c.chromium_args, parse_chromium_args(v), |_| true),
        },
        OptInfo {
            name: "show-soft-navigation-buttons",
            val_spec: "YES/NO/AUTO",
            desc: || {
                concat!(
                    "if enabled, navigation buttons (Back/Forward/Refresh/Home) are added to the control bar in the browser view; ",
                    "in auto mode, enabled unless the vice plugin reports to implement navigation buttons"
                )
                .to_string()
            },
            default_val_str: || {
                default_str(format_optional_bool(Config::default().show_soft_navigation_buttons))
            },
            apply: |c, v| {
                set_if(&mut c.show_soft_navigation_buttons, parse_optional_bool(v), |_| true)
            },
        },
        OptInfo {
            name: "initial-zoom",
            val_spec: "FACTOR",
            desc: || "initial zoom factor for new browser windows".to_string(),
            default_val_str: || default_str(format_double(Config::default().initial_zoom)),
            apply: |c, v| {
                set_if(&mut c.initial_zoom, parse_double(v), |&v| v.is_finite() && v > 0.0)
            },
        },
        OptInfo {
            name: "browser-font-render-mode",
            val_spec: "MODE",
            desc: || {
                format!(
                    "font render mode for the browser area; supported modes: {}",
                    browser_font_render_mode_description()
                )
            },
            default_val_str: || {
                default_str(format_browser_font_render_mode(
                    Config::default().browser_font_render_mode,
                ))
            },
            apply: |c, v| {
                set_if(&mut c.browser_font_render_mode, parse_browser_font_render_mode(v), |_| true)
            },
        },
        OptInfo {
            name: "certificate-check-exceptions",
            val_spec: "DOMAIN,...",
            desc: || {
                concat!(
                    "comma-separated list of domains for which SSL/TLS certificate checking is disabled; ",
                    "only exact match is considered, e.g. exception for 'example.com' does not cover 'www.example.com'"
                )
                .to_string()
            },
            default_val_str: || "default empty".to_string(),
            apply: |c, v| {
                set_if(
                    &mut c.certificate_check_exceptions,
                    parse_certificate_check_exceptions(v),
                    |_| true,
                )
            },
        },
        OptInfo {
            name: "show-control-bar",
            val_spec: "YES/NO",
            desc: || {
                "controls whether or not the control bar is shown (disabling is useful with e.g. kiosk software)".to_string()
            },
            default_val_str: || default_str(format_bool(Config::default().show_control_bar)),
            apply: |c, v| set_if(&mut c.show_control_bar, parse_bool(v), |_| true),
        },
    ]);
    opts
}

const OPT_ALIASES: &[(&str, &str)] = &[
    ("default-quality", "vice-opt-default-quality"),
    ("http-auth", "vice-opt-http-auth"),
    ("http-listen-addr", "vice-opt-http-listen-addr"),
];

// ---------------------------------------------------------------------------
// Help line formatting.
// ---------------------------------------------------------------------------

/// Formats one line (possibly wrapped to several physical lines) of `--help`
/// output exactly like `helpLine` in config.cpp. Works on bytes like the C++
/// version.
pub fn help_line(name: &str, val_spec: &str, desc: &str, default_val_str: &str) -> String {
    const DESC_START: usize = 40;
    const DESC_START_INDENTED: usize = 42;
    const MAX_WIDTH: usize = 100;

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(format!("  --{name}={val_spec} ").as_bytes());
    while out.len() < DESC_START {
        out.push(b' ');
    }
    let mut line_pos = out.len();

    let mut write_atom = |out: &mut Vec<u8>, atom: &[u8]| {
        let mut atom = atom;
        if line_pos + atom.len() > MAX_WIDTH && line_pos > DESC_START_INDENTED {
            out.push(b'\n');
            out.extend(std::iter::repeat_n(b' ', DESC_START_INDENTED));
            line_pos = DESC_START_INDENTED;
            let s = atom.iter().take_while(|&&b| is_c_space(b)).count();
            atom = &atom[s..];
        }
        out.extend_from_slice(atom);
        line_pos += atom.len();
    };

    let d = desc.as_bytes();
    let mut a = 0;
    while a < d.len() {
        let mut b = a + 1;
        while b < d.len() && !is_c_space(d[b]) {
            b += 1;
        }
        write_atom(&mut out, &d[a..b]);
        a = b;
    }
    write_atom(&mut out, format!(" [{default_val_str}]").as_bytes());

    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// Command line reading.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Help,
    Version,
    Credits,
}

/// Result of parsing the command line, before any action is taken.
#[derive(Debug, PartialEq)]
pub enum ParseOutcome {
    /// Parsing succeeded; `mode` tells whether to run normally or show info.
    Ok { config: Config, mode: Mode },
    /// Parsing failed; the error text (possibly multiple lines, each ending in
    /// a newline) should be printed to stderr and the program should exit 1.
    Error(String),
}

/// Parses the command line arguments (`args[0]` is the program name).
/// Deprecated alias warnings are logged to stderr as they are encountered.
pub fn parse_args(args: &[String]) -> ParseOutcome {
    assert!(!args.is_empty(), "Requirement 'argc >= 1' failed");
    let argv0 = &args[0];

    let infos = opt_infos();
    let handlers: HashMap<&str, &OptInfo> = infos.iter().map(|i| (i.name, i)).collect();

    let mut config = Config::default();
    let mut opts_seen: BTreeSet<String> = BTreeSet::new();
    let mut mode = Mode::Normal;

    for arg in &args[1..] {
        match arg.as_str() {
            "--help" => {
                mode = Mode::Help;
                continue;
            }
            "--version" => {
                mode = Mode::Version;
                continue;
            }
            "--credits" => {
                mode = Mode::Credits;
                continue;
            }
            _ => {}
        }

        if let Some(body) = arg.strip_prefix("--") {
            if let Some(eq) = body.find('=') {
                let mut opt_name = body[..eq].to_string();
                let opt_val = &body[eq + 1..];

                if let Some(&(_, target)) = OPT_ALIASES.iter().find(|(a, _)| *a == opt_name) {
                    crate::warning_log!(
                        "The command line option --{} is a deprecated alias for --{}",
                        opt_name,
                        target
                    );
                    opt_name = target.to_string();
                }

                if !opts_seen.insert(opt_name.clone()) {
                    return ParseOutcome::Error(format!(
                        "ERROR: Option --{opt_name} specified multiple times\n"
                    ));
                }

                if opt_name.len() > 9 && opt_name.starts_with("vice-opt-") {
                    config
                        .vice_opts
                        .push((opt_name[9..].to_string(), opt_val.to_string()));
                    continue;
                }

                if let Some(info) = handlers.get(opt_name.as_str()) {
                    if !(info.apply)(&mut config, opt_val) {
                        return ParseOutcome::Error(format!(
                            "ERROR: Invalid value '{opt_val}' given for option --{opt_name}\n\
                             See '{argv0} --help' for more information\n"
                        ));
                    }
                    continue;
                }
            }
        }

        let mut err = match arg.strip_prefix("--") {
            Some(name) if handlers.contains_key(name) => {
                format!("ERROR: Value missing for option {arg}\n")
            }
            _ => format!("ERROR: Unrecognized option '{arg}'\n"),
        };
        err.push_str(&format!(
            "Try '{argv0} --help' for list of supported options\n"
        ));
        return ParseOutcome::Error(err);
    }

    ParseOutcome::Ok { config, mode }
}

/// The help text for the options of Browservice itself (not the vice plugin).
pub fn help_text(argv0: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("USAGE: {argv0} [OPTION]...\n"));
    out.push('\n');
    out.push_str("Supported options:\n");

    let mut lines: Vec<String> = opt_infos()
        .iter()
        .map(|i| help_line(i.name, i.val_spec, &(i.desc)(), &(i.default_val_str)()))
        .collect();
    lines.push("  --help                                show this help and exit".to_string());
    lines.push("  --version                             show the version and exit".to_string());
    lines.push(
        "  --credits                             show copyright information and exit".to_string(),
    );
    lines.sort();
    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

pub fn vice_help_lines(plugin: &VicePlugin) -> Vec<String> {
    let mut lines: Vec<String> = plugin
        .option_docs()
        .into_iter()
        .map(|item| {
            help_line(
                &format!("vice-opt-{}", item.name),
                &item.val_spec,
                &item.desc,
                &item.default_val_str,
            )
        })
        .collect();
    lines.sort();
    lines
}

/// Reads the configuration from the command line like `Config::read`. Returns
/// `Ok(config)` if the program should continue running, or `Err(exit_code)`
/// if it should exit (0 after help/version/credits, 1 on errors).
pub fn read(args: &[String]) -> Result<Config, i32> {
    let (config, mode) = match parse_args(args) {
        ParseOutcome::Ok { config, mode } => (config, mode),
        ParseOutcome::Error(err) => {
            let _ = std::io::stderr().write_all(err.as_bytes());
            return Err(1);
        }
    };
    let argv0 = &args[0];
    let mut stdout = std::io::stdout().lock();

    match mode {
        Mode::Normal => return Ok(config),
        Mode::Help => {
            let _ = stdout.write_all(help_text(argv0).as_bytes());
            let _ = write!(
                stdout,
                "\nSupported options for the vice plugin '{}' selected by --vice-plugin:\n",
                config.vice_plugin
            );
            let _ = stdout.flush();
            if let Some(plugin) = VicePlugin::load(&config.vice_plugin) {
                for line in vice_help_lines(&plugin) {
                    let _ = writeln!(stdout, "{line}");
                }
            }
        }
        Mode::Version => {
            let _ = writeln!(
                stdout,
                "Browservice {}, built with CEF {}",
                BROWSERVICE_VERSION,
                cef_version()
            );
            let _ = stdout.flush();
            if let Some(plugin) = VicePlugin::load(&config.vice_plugin) {
                let _ = writeln!(
                    stdout,
                    "Vice plugin {}: {}",
                    config.vice_plugin,
                    plugin.version_string()
                );
            }
            let _ = writeln!(stdout, "For copyright information, use --credits");
        }
        Mode::Credits => {
            let _ = write!(
                stdout,
                "------------------------\nCredits for Browservice:\n------------------------\n\n{CREDITS}"
            );
            let _ = stdout.flush();
            if let Some(plugin) = VicePlugin::load(&config.vice_plugin) {
                let plugin_credits = plugin.credits_string();
                let line = "-".repeat(25 + config.vice_plugin.len());
                let _ = write!(
                    stdout,
                    "\n{line}\nCredits for vice plugin {}:\n{line}\n\n{plugin_credits}",
                    config.vice_plugin
                );
            }
            let _ = writeln!(stdout);
        }
    }
    let _ = stdout.flush();
    Err(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("browservice")
            .chain(list.iter().copied())
            .map(String::from)
            .collect()
    }

    fn ok(list: &[&str]) -> (Config, Mode) {
        match parse_args(&args(list)) {
            ParseOutcome::Ok { config, mode } => (config, mode),
            ParseOutcome::Error(e) => panic!("unexpected error: {e}"),
        }
    }

    fn err(list: &[&str]) -> String {
        match parse_args(&args(list)) {
            ParseOutcome::Ok { .. } => panic!("expected error for {list:?}"),
            ParseOutcome::Error(e) => e,
        }
    }

    #[test]
    fn defaults() {
        let (c, mode) = ok(&[]);
        assert_eq!(mode, Mode::Normal);
        assert_eq!(c, Config::default());
        #[cfg(not(windows))]
        assert_eq!(c.vice_plugin, "retrojsvice.so");
        #[cfg(windows)]
        assert_eq!(c.vice_plugin, "retrojsvice.dll");
        assert_eq!(c.start_page, "about:blank");
        assert_eq!(c.window_limit, 32);
        assert_eq!(c.initial_zoom, 1.0);
        assert!(c.block_file_scheme);
        #[cfg(not(windows))]
        assert!(c.use_dedicated_xvfb);
        assert!(c.show_control_bar);
        assert_eq!(c.show_soft_navigation_buttons, None);
        assert_eq!(
            c.browser_font_render_mode,
            BrowserFontRenderMode::AntiAliasing
        );
    }

    #[test]
    fn modes_last_wins() {
        assert_eq!(ok(&["--help"]).1, Mode::Help);
        assert_eq!(ok(&["--help", "--version"]).1, Mode::Version);
        assert_eq!(ok(&["--version", "--credits"]).1, Mode::Credits);
    }

    #[test]
    fn errors_take_precedence_over_help() {
        let e = err(&["--help", "--bogus"]);
        assert_eq!(
            e,
            "ERROR: Unrecognized option '--bogus'\nTry 'browservice --help' for list of supported options\n"
        );
    }

    #[test]
    fn value_missing_and_unrecognized() {
        assert_eq!(
            err(&["--start-page"]),
            "ERROR: Value missing for option --start-page\nTry 'browservice --help' for list of supported options\n"
        );
        assert!(err(&["foo"]).starts_with("ERROR: Unrecognized option 'foo'\n"));
        assert!(err(&["--foo=bar"]).starts_with("ERROR: Unrecognized option '--foo=bar'\n"));
        assert!(
            err(&["--vice-opt-=x"]).starts_with("ERROR: Unrecognized option '--vice-opt-=x'\n")
        );
        assert!(
            err(&["--vice-opt-foo"]).starts_with("ERROR: Unrecognized option '--vice-opt-foo'\n")
        );
    }

    #[test]
    fn invalid_value() {
        assert_eq!(
            err(&["--window-limit=0"]),
            "ERROR: Invalid value '0' given for option --window-limit\nSee 'browservice --help' for more information\n"
        );
        assert!(err(&["--start-page="]).starts_with("ERROR: Invalid value '' given"));
        assert!(err(&["--vice-plugin="]).starts_with("ERROR: Invalid value '' given"));
        assert!(err(&["--user-agent="]).starts_with("ERROR: Invalid value '' given"));
        assert!(err(&["--initial-zoom=0"]).starts_with("ERROR: Invalid value"));
        assert!(err(&["--initial-zoom=-1"]).starts_with("ERROR: Invalid value"));
        assert!(err(&["--initial-zoom=1e999"]).starts_with("ERROR: Invalid value"));
        assert!(err(&["--block-file-scheme=maybe"]).starts_with("ERROR: Invalid value"));
        assert!(err(&["--browser-font-render-mode=x"]).starts_with("ERROR: Invalid value"));
    }

    #[test]
    fn duplicates() {
        assert_eq!(
            err(&["--start-page=a", "--start-page=b"]),
            "ERROR: Option --start-page specified multiple times\n"
        );
        assert_eq!(
            err(&["--http-listen-addr=a", "--vice-opt-http-listen-addr=b"]),
            "ERROR: Option --vice-opt-http-listen-addr specified multiple times\n"
        );
    }

    #[test]
    fn vice_opts_and_aliases() {
        let (c, _) = ok(&[
            "--vice-opt-http-listen-addr=0.0.0.0:8080",
            "--default-quality=PNG",
            "--vice-opt-x=a=b",
        ]);
        assert_eq!(
            c.vice_opts,
            vec![
                ("http-listen-addr".to_string(), "0.0.0.0:8080".to_string()),
                ("default-quality".to_string(), "PNG".to_string()),
                ("x".to_string(), "a=b".to_string()),
            ]
        );
    }

    #[test]
    fn all_options() {
        let list = [
            "--vice-plugin=/x/y.so",
            "--user-agent=UA",
            "--block-file-scheme=disabled",
            "--start-page=http://example.com/?a=b",
            "--data-dir=/tmp/d",
            "--window-limit=+5",
            "--chromium-args=--foo,bar=1,-baz=a=b",
            "--show-soft-navigation-buttons=AUTO",
            "--initial-zoom=1.5",
            "--browser-font-render-mode=Antialiasing-Subpixel-RGB",
            "--certificate-check-exceptions=b.com,a.com,a.com",
            "--show-control-bar=0",
            // --use-dedicated-xvfb does not exist on Windows.
            #[cfg(not(windows))]
            "--use-dedicated-xvfb=No",
        ];
        let (c, _) = ok(&list);
        assert_eq!(c.vice_plugin, "/x/y.so");
        assert_eq!(c.user_agent, "UA");
        #[cfg(not(windows))]
        assert!(!c.use_dedicated_xvfb);
        assert!(!c.block_file_scheme);
        assert_eq!(c.start_page, "http://example.com/?a=b");
        assert_eq!(c.data_dir, "/tmp/d");
        assert_eq!(c.window_limit, 5);
        assert_eq!(
            c.chromium_args,
            vec![
                ("foo".to_string(), None),
                ("bar".to_string(), Some("1".to_string())),
                ("baz".to_string(), Some("a=b".to_string())),
            ]
        );
        assert_eq!(c.show_soft_navigation_buttons, None);
        assert_eq!(c.initial_zoom, 1.5);
        assert_eq!(
            c.browser_font_render_mode,
            BrowserFontRenderMode::AntiAliasingSubpixelRGB
        );
        assert_eq!(
            c.certificate_check_exceptions.iter().collect::<Vec<_>>(),
            vec!["a.com", "b.com"]
        );
        assert!(!c.show_control_bar);
        let (c, _) = ok(&["--show-soft-navigation-buttons=yes", "--data-dir="]);
        assert_eq!(c.show_soft_navigation_buttons, Some(true));
        assert_eq!(c.data_dir, "");
    }

    #[test]
    fn bool_parser() {
        for s in ["1", "yes", "TRUE", "Enable", "enabled"] {
            assert_eq!(parse_bool(s), Some(true), "{s}");
        }
        for s in ["0", "NO", "false", "disable", "Disabled"] {
            assert_eq!(parse_bool(s), Some(false), "{s}");
        }
        for s in ["", "auto", "y", " yes"] {
            assert_eq!(parse_bool(s), None, "{s}");
        }
        assert_eq!(parse_optional_bool("autodetect"), Some(None));
        assert_eq!(parse_optional_bool("automatic"), Some(None));
        assert_eq!(parse_optional_bool("no"), Some(Some(false)));
        assert_eq!(parse_optional_bool("x"), None);
    }

    #[test]
    fn int_parser() {
        assert_eq!(parse_int("42"), Some(42));
        assert_eq!(parse_int("  42"), Some(42));
        assert_eq!(parse_int("+42"), Some(42));
        assert_eq!(parse_int("-3"), Some(-3));
        assert_eq!(parse_int("2147483647"), Some(i32::MAX));
        assert_eq!(parse_int("2147483648"), None);
        assert_eq!(parse_int("42 "), None);
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_int("+"), None);
        assert_eq!(parse_int("0x10"), None);
        assert_eq!(parse_int("1.0"), None);
    }

    #[test]
    fn double_parser() {
        assert_eq!(parse_double("1"), Some(1.0));
        assert_eq!(parse_double(" 1.25"), Some(1.25));
        assert_eq!(parse_double(".5"), Some(0.5));
        assert_eq!(parse_double("5."), Some(5.0));
        assert_eq!(parse_double("-2e-1"), Some(-0.2));
        assert_eq!(parse_double("1E3"), Some(1000.0));
        assert_eq!(parse_double("1e"), None);
        assert_eq!(parse_double("."), None);
        assert_eq!(parse_double("inf"), None);
        assert_eq!(parse_double("nan"), None);
        assert_eq!(parse_double("1.0x"), None);
        assert_eq!(parse_double(""), None);
        assert_eq!(parse_double("1e999"), None);
    }

    #[test]
    fn list_parsers() {
        assert_eq!(parse_chromium_args(""), Some(vec![]));
        assert_eq!(parse_chromium_args(","), None);
        assert_eq!(parse_chromium_args("a,"), None);
        assert_eq!(parse_chromium_args("--"), None);
        assert_eq!(parse_chromium_args("=x"), None);
        assert_eq!(
            parse_chromium_args("---a"),
            Some(vec![("-a".to_string(), None)])
        );
        assert_eq!(
            parse_chromium_args("a="),
            Some(vec![("a".to_string(), Some(String::new()))])
        );
        assert_eq!(
            parse_certificate_check_exceptions(""),
            Some(BTreeSet::new())
        );
        assert_eq!(parse_certificate_check_exceptions("a,,b"), None);
        assert_eq!(parse_certificate_check_exceptions("a,"), None);
    }

    #[test]
    fn double_formatting() {
        assert_eq!(format_double(1.0), "1");
        assert_eq!(format_double(1.5), "1.5");
        assert_eq!(format_double(0.1), "0.1");
        assert_eq!(format_double(1234567.0), "1.23457e+06");
        assert_eq!(format_double(0.0001), "0.0001");
        assert_eq!(format_double(0.00001), "1e-05");
        assert_eq!(format_double(100000.0), "100000");
    }

    #[test]
    fn help_line_formatting() {
        assert_eq!(
            help_line(
                "window-limit",
                "COUNT",
                "maximum number of browser windows",
                "default: 32"
            ),
            "  --window-limit=COUNT                  maximum number of browser windows [default: 32]"
        );
        // Long name: description starts right after the single space.
        let l = help_line("a-very-long-option-name-here", "SOME_LONG_VALUE", "x", "d");
        assert_eq!(l, "  --a-very-long-option-name-here=SOME_LONG_VALUE x [d]");
        // Wrapping: continuation lines are indented by 42 spaces and lines do
        // not exceed 100 columns unless a single word is longer.
        let desc = "word ".repeat(40);
        let l = help_line("opt", "V", desc.trim_end(), "default: 1");
        let lines: Vec<&str> = l.split('\n').collect();
        assert!(lines.len() > 1);
        for (i, line) in lines.iter().enumerate() {
            assert!(line.len() <= 100, "{line:?}");
            if i > 0 {
                assert!(line.starts_with(&" ".repeat(42)));
                assert!(!line[42..].starts_with(' '));
            }
        }
    }

    // The reference text was captured from the Linux C++ build (the Windows
    // build has no --use-dedicated-xvfb and other defaults).
    #[cfg(not(windows))]
    #[test]
    fn help_output_matches_cpp() {
        // Reference produced by the C++ build (the Browservice part of
        // `browservice --help`, with argv[0] = "browservice").
        let expected = include_str!("../tests/data/help_browservice_part.txt");
        assert_eq!(help_text("browservice"), expected);
    }
}
