use std::io::{self, Write};

/// A stable exit category and a terminal-safe human explanation.
#[derive(Debug)]
pub struct CliError {
    pub exit: u8,
    pub code: &'static str,
    pub category: &'static str,
    pub message: &'static str,
}

impl CliError {
    pub fn usage(message: &'static str) -> Self {
        Self::new(2, "cli.usage", "usage", message)
    }

    pub fn validation(message: &'static str) -> Self {
        Self::new(3, "cli.validation", "validation", message)
    }

    pub fn capability(message: &'static str) -> Self {
        Self::new(4, "cli.capability-unavailable", "capability", message)
    }

    pub fn permission(message: &'static str) -> Self {
        Self::new(5, "cli.permission-denied", "permission", message)
    }

    pub fn io(message: &'static str) -> Self {
        Self::new(8, "cli.io", "io", message)
    }

    fn new(exit: u8, code: &'static str, category: &'static str, message: &'static str) -> Self {
        Self {
            exit,
            code,
            category,
            message,
        }
    }
}

/// Removes CSI, OSC, DCS, SOS, PM, and APC sequences, including their C1 forms.
/// Newlines and tabs are retained for help/diagnostic layout; other controls and bidi controls
/// are removed. Unterminated control strings consume the rest of the text.
pub fn strip_terminal_controls(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => match chars.next() {
                Some('[') => skip_csi(&mut chars),
                Some(']') => skip_control_string(&mut chars, true),
                Some('P' | 'X' | '^' | '_') => skip_control_string(&mut chars, false),
                Some(next) if ('\u{20}'..='\u{2f}').contains(&next) => {
                    for part in chars.by_ref() {
                        if !('\u{20}'..='\u{2f}').contains(&part) {
                            break;
                        }
                    }
                }
                Some(_) | None => {}
            },
            '\u{9b}' => skip_csi(&mut chars),
            '\u{9d}' => skip_control_string(&mut chars, true),
            '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => skip_control_string(&mut chars, false),
            '\n' | '\t' => output.push(ch),
            '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => {}
            _ if ch.is_control() => {}
            _ => output.push(ch),
        }
    }
    output
}

fn skip_csi(chars: &mut std::str::Chars<'_>) {
    for ch in chars.by_ref() {
        if ('\u{40}'..='\u{7e}').contains(&ch) {
            break;
        }
    }
}

fn skip_control_string(chars: &mut std::str::Chars<'_>, bell_terminates: bool) {
    let mut previous_escape = false;
    for ch in chars.by_ref() {
        if ch == '\u{9c}' || (bell_terminates && ch == '\u{07}') || (previous_escape && ch == '\\')
        {
            break;
        }
        previous_escape = ch == '\u{1b}';
    }
}

pub fn print_error(error: &CliError, json: bool, command: &'static str) -> io::Result<()> {
    let mut message = strip_terminal_controls(error.message);
    if let Some((index, _)) = message.char_indices().nth(4096) {
        message.truncate(index);
    }
    if json {
        let stdout = io::stdout();
        let mut stream = stdout.lock();
        let result = serde_json::json!({
            "schema_version": 1,
            "command": command,
            "status": "error",
            "error": {
                "code": error.code,
                "category": error.category,
                "message": message,
                "retryable": false
            },
            "result": null
        });
        serde_json::to_writer(&mut stream, &result)?;
        writeln!(stream)?;
        stream.flush()
    } else {
        let stderr = io::stderr();
        let mut stream = stderr.lock();
        writeln!(stream, "{message}")?;
        stream.flush()
    }
}

pub fn print_help(text: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut stream = stdout.lock();
    write!(stream, "{}", strip_terminal_controls(text))?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::strip_terminal_controls;

    #[test]
    fn strips_sgr_osc_clipboard_c1_and_bidi_controls() {
        let hostile = concat!(
            "before",
            "\x1b]52;c;secret\x07",
            "after ",
            "\x1b[31m",
            "red",
            "\x1b[0m",
            "\u{9d}0;title\u{9c}",
            "\u{202e}",
            "!"
        );
        assert_eq!(strip_terminal_controls(hostile), "beforeafter red!");
    }

    #[test]
    fn unterminated_control_strings_do_not_leak_their_payload() {
        assert_eq!(strip_terminal_controls("safe\x1b]0;untrusted"), "safe");
        assert_eq!(strip_terminal_controls("safe\u{90}payload"), "safe");
    }
}
