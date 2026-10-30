//! zsh prompt lowering and escaping.

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::config::TerminalConfig;
use crate::render::LoweredPrompt;
use crate::segments::{SegmentContent, SegmentPart, Style};

const PROMPT_START: &str = "%{\x1b]133;A\x07%}";
const INPUT_START: &str = "%{\x1b]133;B\x07%}";

/// Adds OSC 133 prompt marks and the OSC 7 working directory for a live zsh
/// session. The shell adapter emits the matching command marks (C and D).
pub fn with_terminal_integration(
    output: LoweredPrompt,
    config: &TerminalConfig,
    cwd: &Path,
    host: Option<&str>,
) -> LoweredPrompt {
    let cwd_report = if config.report_cwd {
        // Percent-encoding produces `%`, which zsh would expand even inside `%{ %}`.
        wrap_non_printing(&escape_prompt_text(&format!(
            "\x1b]7;file://{}{}\x07",
            host.map(percent_encode_host).unwrap_or_default(),
            percent_encode_path(cwd)
        )))
    } else {
        String::new()
    };
    let mark = |prompt: String| {
        if config.semantic_prompts && !prompt.is_empty() {
            format!("{PROMPT_START}{prompt}{INPUT_START}")
        } else {
            prompt
        }
    };

    LoweredPrompt {
        prompt: format!("{cwd_report}{}", mark(output.prompt)),
        rprompt: output.rprompt,
        transient: mark(output.transient),
    }
}

fn percent_encode_path(path: &Path) -> String {
    percent_encode(path.as_os_str().as_bytes(), |byte| byte == b'/')
}

fn percent_encode_host(host: &str) -> String {
    percent_encode(host.as_bytes(), |_byte| false)
}

fn percent_encode(bytes: &[u8], keep: impl Fn(u8) -> bool) -> String {
    bytes
        .iter()
        .map(|&byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) || keep(byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub fn lower_segment(segment: &SegmentContent) -> String {
    if segment.uses_parts() {
        return segment
            .parts
            .iter()
            .map(lower_part)
            .collect::<Vec<_>>()
            .join("");
    }

    let text = escape_prompt_text(&segment.text);
    lower_text(&text, &segment.style)
}

/// Columns a capsule adds around a segment: two caps and one space of padding
/// on each side.
pub const CAPSULE_DECORATION_WIDTH: usize = 4;
const CAPSULE_LEFT: &str = "\u{e0b6}";
const CAPSULE_RIGHT: &str = "\u{e0b4}";

/// Draws a segment as a rounded pill. The pill takes the segment's `bg`
/// (`default_bg` when unset); each part keeps its own colors on top of it.
pub fn lower_capsule(segment: &SegmentContent, default_bg: &str) -> String {
    let parts = if segment.uses_parts() {
        segment.parts.clone()
    } else {
        vec![SegmentPart {
            text: segment.text.clone(),
            style: segment.style.clone(),
        }]
    };
    // A part without its own bg (e.g. a duration prefix) must not turn the pill gray.
    let bg = parts
        .iter()
        .find_map(|part| part.style.bg.clone())
        .or_else(|| segment.style.bg.clone())
        .unwrap_or_else(|| default_bg.to_string());
    let cap = Style {
        fg: Some(bg.clone()),
        bg: None,
        bold: false,
    };
    let padding = Style {
        fg: None,
        bg: Some(bg.clone()),
        bold: false,
    };
    let body = parts
        .iter()
        .map(|part| {
            lower_text(
                &escape_prompt_text(&part.text),
                &Style {
                    bg: part.style.bg.clone().or_else(|| Some(bg.clone())),
                    ..part.style.clone()
                },
            )
        })
        .collect::<String>();

    format!(
        "{}{}{body}{}{}",
        lower_text(CAPSULE_LEFT, &cap),
        lower_text(" ", &padding),
        lower_text(" ", &padding),
        lower_text(CAPSULE_RIGHT, &cap)
    )
}

fn lower_part(part: &SegmentPart) -> String {
    let text = escape_prompt_text(&part.text);
    lower_text(&text, &part.style)
}

fn lower_text(text: &str, style: &Style) -> String {
    let Some(start) = start_sgr(style) else {
        return text.to_string();
    };

    format!(
        "{}{}{}",
        wrap_non_printing(&start),
        text,
        wrap_non_printing("\x1b[0m")
    )
}

pub fn escape_prompt_text(input: &str) -> String {
    input.replace('%', "%%")
}

fn start_sgr(style: &Style) -> Option<String> {
    let mut codes = Vec::new();

    if style.bold {
        codes.push("1".to_string());
    }

    if let Some(code) = style
        .fg
        .as_deref()
        .and_then(|color| color_sgr(color, false))
    {
        codes.push(code);
    }

    if let Some(code) = style.bg.as_deref().and_then(|color| color_sgr(color, true)) {
        codes.push(code);
    }

    if codes.is_empty() {
        None
    } else {
        Some(format!("\x1b[{}m", codes.join(";")))
    }
}

const RIGHT_ALIGN_PREFIX: &str = "%{\x1b[999C\x1b[";
const RIGHT_ALIGN_SUFFIX: &str = "D%}";

/// Moves the cursor so the next `width` columns end one column before the
/// right edge. The terminal clamps `999C` to its current width, so the
/// alignment survives a resize without re-rendering, and zsh counts the
/// sequence as zero width, so it never sees a wrapped line.
pub fn right_align(width: usize) -> String {
    format!("{RIGHT_ALIGN_PREFIX}{width}{RIGHT_ALIGN_SUFFIX}")
}

/// Replaces [`right_align`] sequences with spaces for a fixed `columns`
/// width, for previews that are not drawn by zsh.
pub fn expand_right_align(prompt: &str, columns: usize) -> String {
    prompt
        .split('\n')
        .map(|line| {
            let Some((left, rest)) = line.split_once(RIGHT_ALIGN_PREFIX) else {
                return line.to_string();
            };
            let Some((width, right)) = rest.split_once(RIGHT_ALIGN_SUFFIX) else {
                return line.to_string();
            };
            let width = width.parse::<usize>().unwrap_or(0);
            let left_width = super::visible_prompt_width(left);
            let padding = columns.saturating_sub(left_width + width + 1).max(1);
            format!("{left}{}{right}", " ".repeat(padding))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn wrap_non_printing(input: &str) -> String {
    format!("%{{{input}%}}")
}

fn color_sgr(color: &str, bg: bool) -> Option<String> {
    if let Some(code) = named_color_sgr(color, bg) {
        return Some(code.to_string());
    }

    if let Ok(index) = color.parse::<u8>() {
        let prefix = if bg { "48" } else { "38" };
        return Some(format!("{prefix};5;{index}"));
    }

    let (red, green, blue) = parse_truecolor(color)?;
    let prefix = if bg { "48" } else { "38" };
    Some(format!("{prefix};2;{red};{green};{blue}"))
}

fn named_color_sgr(color: &str, bg: bool) -> Option<&'static str> {
    match (color, bg) {
        ("black", false) => Some("30"),
        ("red", false) => Some("31"),
        ("green", false) => Some("32"),
        ("yellow", false) => Some("33"),
        ("blue", false) => Some("34"),
        ("magenta", false) => Some("35"),
        ("cyan", false) => Some("36"),
        ("white", false) => Some("37"),
        ("bright_black", false) => Some("90"),
        ("bright_red", false) => Some("91"),
        ("bright_green", false) => Some("92"),
        ("bright_yellow", false) => Some("93"),
        ("bright_blue", false) => Some("94"),
        ("bright_magenta", false) => Some("95"),
        ("bright_cyan", false) => Some("96"),
        ("bright_white", false) => Some("97"),
        ("black", true) => Some("40"),
        ("red", true) => Some("41"),
        ("green", true) => Some("42"),
        ("yellow", true) => Some("43"),
        ("blue", true) => Some("44"),
        ("magenta", true) => Some("45"),
        ("cyan", true) => Some("46"),
        ("white", true) => Some("47"),
        ("bright_black", true) => Some("100"),
        ("bright_red", true) => Some("101"),
        ("bright_green", true) => Some("102"),
        ("bright_yellow", true) => Some("103"),
        ("bright_blue", true) => Some("104"),
        ("bright_magenta", true) => Some("105"),
        ("bright_cyan", true) => Some("106"),
        ("bright_white", true) => Some("107"),
        _ => None,
    }
}

fn parse_truecolor(color: &str) -> Option<(u8, u8, u8)> {
    if color.len() != 7
        || !color.starts_with('#')
        || !color[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return None;
    }

    let red = u8::from_str_radix(&color[1..3], 16).ok()?;
    let green = u8::from_str_radix(&color[3..5], 16).ok()?;
    let blue = u8::from_str_radix(&color[5..7], 16).ok()?;
    Some((red, green, blue))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lowered(prompt: &str, transient: &str) -> LoweredPrompt {
        LoweredPrompt {
            prompt: prompt.to_string(),
            rprompt: "12:00".to_string(),
            transient: transient.to_string(),
        }
    }

    #[test]
    fn terminal_integration_marks_prompts_and_reports_an_encoded_cwd() {
        let output = with_terminal_integration(
            lowered("~/nova\n❯ ", "❯ "),
            &TerminalConfig::default(),
            Path::new("/tmp/my dir/100%"),
            Some("mac book"),
        );

        assert_eq!(
            output.prompt,
            "%{\x1b]7;file://mac%%20book/tmp/my%%20dir/100%%25\x07%}\
             %{\x1b]133;A\x07%}~/nova\n❯ %{\x1b]133;B\x07%}"
        );
        assert_eq!(output.transient, "%{\x1b]133;A\x07%}❯ %{\x1b]133;B\x07%}");
        assert_eq!(output.rprompt, "12:00");
        assert_eq!(super::super::visible_prompt_width(&output.transient), 2);
    }

    #[test]
    fn terminal_integration_can_be_disabled_and_keeps_transient_off() {
        let config = TerminalConfig {
            semantic_prompts: false,
            report_cwd: false,
        };
        let output = with_terminal_integration(lowered("❯ ", ""), &config, Path::new("/tmp"), None);
        assert_eq!(output, lowered("❯ ", ""));

        let output = with_terminal_integration(
            lowered("❯ ", ""),
            &TerminalConfig::default(),
            Path::new("/tmp"),
            None,
        );
        assert_eq!(
            output.transient, "",
            "a disabled transient prompt stays empty"
        );
        assert!(output.prompt.starts_with("%{\x1b]7;file:///tmp\x07%}"));
    }

    #[test]
    fn capsule_takes_its_color_from_any_part_with_a_background() {
        let segment = SegmentContent::from_parts(
            "duration",
            vec![
                SegmentPart::new("+", Style::default()),
                SegmentPart::new(
                    "2s",
                    Style {
                        bg: Some("blue".to_string()),
                        ..Style::default()
                    },
                ),
            ],
        );

        let lowered = lower_capsule(&segment, "238");

        assert!(lowered.starts_with("%{\x1b[34m%}\u{e0b6}"), "{lowered:?}");
        assert!(
            lowered.contains("%{\x1b[44m%}+"),
            "the prefix sits on the pill: {lowered:?}"
        );
        assert!(!lowered.contains("238"));
    }

    #[test]
    fn right_align_is_a_zero_width_cursor_move() {
        assert_eq!(right_align(8), "%{\x1b[999C\x1b[8D%}");
        assert_eq!(super::super::visible_prompt_width(&right_align(8)), 0);
    }

    #[test]
    fn expands_right_align_into_padding_that_keeps_the_last_column_free() {
        let prompt = format!(
            "%{{\x1b[32m%}}~/nova%{{\x1b[0m%}}{}12:00\n❯ ",
            right_align(5)
        );

        assert_eq!(
            expand_right_align(&prompt, 20),
            format!(
                "%{{\x1b[32m%}}~/nova%{{\x1b[0m%}}{}12:00\n❯ ",
                " ".repeat(8)
            )
        );
        assert_eq!(expand_right_align("plain\nline", 20), "plain\nline");
    }

    #[test]
    fn escapes_percent_and_wraps_ansi_sequences() {
        let segment = SegmentContent::new(
            "test",
            "100%",
            Style {
                fg: Some("red".to_string()),
                bg: None,
                bold: true,
            },
        );

        assert_eq!(
            lower_segment(&segment),
            "%{\u{1b}[1;31m%}100%%%{\u{1b}[0m%}"
        );
    }

    #[test]
    fn lowers_styled_parts_without_inserting_spaces() {
        let segment = SegmentContent::from_parts(
            "user_host",
            vec![
                SegmentPart::new(
                    "nova",
                    Style {
                        fg: Some("green".to_string()),
                        bg: None,
                        bold: false,
                    },
                ),
                SegmentPart::new("@M4Pro", Style::default()),
            ],
        );

        assert_eq!(
            lower_segment(&segment),
            "%{\u{1b}[32m%}nova%{\u{1b}[0m%}@M4Pro"
        );
    }

    #[test]
    fn lowers_bright_ansi_colors() {
        let segment = SegmentContent::new(
            "test",
            "bright",
            Style {
                fg: Some("bright_white".to_string()),
                bg: Some("bright_blue".to_string()),
                bold: false,
            },
        );

        assert_eq!(
            lower_segment(&segment),
            "%{\u{1b}[97;104m%}bright%{\u{1b}[0m%}"
        );
    }

    #[test]
    fn lowers_256_color_styles() {
        let segment = SegmentContent::new(
            "test",
            "indexed",
            Style {
                fg: Some("202".to_string()),
                bg: Some("17".to_string()),
                bold: false,
            },
        );

        assert_eq!(
            lower_segment(&segment),
            "%{\u{1b}[38;5;202;48;5;17m%}indexed%{\u{1b}[0m%}"
        );
    }

    #[test]
    fn lowers_truecolor_styles() {
        let segment = SegmentContent::new(
            "test",
            "rgb",
            Style {
                fg: Some("#ff8800".to_string()),
                bg: Some("#102030".to_string()),
                bold: true,
            },
        );

        assert_eq!(
            lower_segment(&segment),
            "%{\u{1b}[1;38;2;255;136;0;48;2;16;32;48m%}rgb%{\u{1b}[0m%}"
        );
    }

    #[test]
    fn ignores_invalid_truecolor_styles() {
        let invalid_color =
            String::from_utf8(vec![b'#', b'1', 0xc3, 0xa9, b'2', b'3', b'4']).unwrap();
        let segment = SegmentContent::new(
            "test",
            "invalid",
            Style {
                fg: Some(invalid_color),
                bg: Some("#gggggg".to_string()),
                bold: false,
            },
        );

        assert_eq!(lower_segment(&segment), "invalid");
    }
}
