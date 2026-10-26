//! Command buffer and parser system.
//!
//! Replaces C's `cmdparser.c` + `cbuf.c` with a safe, `HashMap`-based command
//! registry and a string-based command buffer. Commands are registered with
//! handlers, buffered text is tokenized and dispatched line-by-line.

use std::collections::HashMap;

/// Command buffer capacity (Qwasm2 `cmd_text_buf[32768]`; id's original
/// engine used 8192). Text that would fill it is dropped, as in C.
const CBUF_MAX_SIZE: usize = 32768;
/// C `Cbuf_Execute` copies each command into `char line[1024]`.
const MAX_LINE_CHARS: usize = 1023;
/// C `MAX_STRING_TOKENS`: tokens past this count are dropped.
const MAX_STRING_TOKENS: usize = 80;
/// C `MAX_TOKEN_CHARS`: a token reaching this length becomes empty.
const MAX_TOKEN_CHARS: usize = 1024;

/// A command handler function. Takes a slice of argument strings.
pub type CmdHandler = Box<dyn Fn(&[&str])>;

/// Central command system: registration, buffering, tokenization, and dispatch.
pub struct CmdSystem {
    /// Registered commands: name -> handler.
    commands: HashMap<String, CmdHandler>,
    /// Command text buffer waiting to be executed.
    buffer: String,
    /// Deferred buffer (saved during map loads).
    defer_buffer: String,
}

impl CmdSystem {
    /// Create a new, empty command system.
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            buffer: String::new(),
            defer_buffer: String::new(),
        }
    }

    /// Register a console command with its handler. Like C
    /// `Cmd_AddCommand`, an existing command is kept and the new one refused.
    pub fn add_command(&mut self, name: &str, handler: impl Fn(&[&str]) + 'static) {
        if self.commands.contains_key(name) {
            tracing::warn!("Cmd_AddCommand: {name} already defined");
            return;
        }
        self.commands.insert(name.to_owned(), Box::new(handler));
    }

    /// Remove a registered command.
    pub fn remove_command(&mut self, name: &str) {
        self.commands.remove(name);
    }

    /// Check if a command exists.
    pub fn exists(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    /// Tab-complete a partial command name.
    /// Returns the first alphabetically matching command.
    pub fn complete(&self, partial: &str) -> Option<String> {
        let mut matches: Vec<&str> = self
            .commands
            .keys()
            .filter(|name| name.starts_with(partial))
            .map(String::as_str)
            .collect();
        matches.sort();
        matches.first().map(|s| (*s).to_owned())
    }

    /// Add text to the end of the command buffer.
    /// Text should be newline-terminated for proper execution.
    /// Text that would fill the buffer is dropped whole (C `Cbuf_AddText`),
    /// so remote stufftext cannot grow it without bound.
    pub fn cbuf_add_text(&mut self, text: &str) {
        if self.buffer.len() + text.len() >= CBUF_MAX_SIZE {
            tracing::warn!("Cbuf_AddText: overflow");
            return;
        }
        self.buffer.push_str(text);
    }

    /// Insert text at the beginning of the command buffer.
    pub fn cbuf_insert_text(&mut self, text: &str) {
        self.buffer.insert_str(0, text);
    }

    /// Execute all commands in the buffer.
    /// Pulls `\n`-terminated lines and runs them through `execute_string`.
    pub fn cbuf_execute(&mut self) {
        // Take ownership of the buffer so handlers that add text via
        // references work cleanly. In practice the C engine does this
        // line-by-line too.
        let text = std::mem::take(&mut self.buffer);
        for line in split_commands(&text) {
            let mut end = line.len().min(MAX_LINE_CHARS);
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            let trimmed = line[..end].trim();
            if trimmed.is_empty() {
                continue;
            }
            self.execute_string(trimmed);
        }
    }

    /// Save the current buffer to the defer buffer, clearing the main buffer.
    pub fn cbuf_copy_to_defer(&mut self) {
        self.defer_buffer = std::mem::take(&mut self.buffer);
    }

    /// Restore the defer buffer to the main buffer.
    pub fn cbuf_insert_from_defer(&mut self) {
        let deferred = std::mem::take(&mut self.defer_buffer);
        self.cbuf_insert_text(&deferred);
    }

    /// Parse and execute a single command string.
    pub fn execute_string(&mut self, text: &str) {
        let tokens = tokenize(text);
        if tokens.is_empty() {
            return;
        }

        let name = &tokens[0];
        let arg_strings: Vec<&str> = tokens[1..].iter().map(String::as_str).collect();

        if let Some(handler) = self.commands.get(name.as_str()) {
            handler(&arg_strings);
        }
        // If not found, do nothing (forward to cvar/server comes later).
    }
}

impl Default for CmdSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// C COM_Parse whitespace: every byte up to and including space.
fn is_separator(c: char) -> bool {
    c <= ' '
}

/// Tokenize a command string, handling quoted strings and `//` comments
/// like C `Cmd_TokenizeString`/`COM_Parse`.
///
/// `"say \"hello world\""` becomes `["say", "hello world"]`.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();

    loop {
        // Skip whitespace
        while let Some(&c) = chars.peek() {
            if is_separator(c) {
                chars.next();
            } else {
                break;
            }
        }

        if chars.peek().is_none() {
            break;
        }

        // `//` at the start of a token comments out the rest of the line.
        let mut lookahead = chars.clone();
        if lookahead.next() == Some('/') && lookahead.next() == Some('/') {
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
        }

        let mut token = String::new();
        if chars.peek() == Some(&'"') {
            // Quoted token
            chars.next(); // consume opening quote
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                token.push(c);
            }
        } else {
            // Bare word token
            while let Some(&c) = chars.peek() {
                if is_separator(c) {
                    break;
                }
                token.push(c);
                chars.next();
            }
        }

        if token.len() >= MAX_TOKEN_CHARS {
            token.clear();
        }
        if tokens.len() < MAX_STRING_TOKENS {
            tokens.push(token);
        }
    }

    tokens
}

/// Split a command buffer string into individual commands like C
/// `Cbuf_Execute`: `;` splits only outside quotes, `\n` always splits and
/// resets quote parity, so an unbalanced quote cannot eat later lines.
fn split_commands(text: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in text.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                current.push(c);
            }
            '\n' => {
                commands.push(std::mem::take(&mut current));
                in_quotes = false;
            }
            ';' if !in_quotes => {
                commands.push(std::mem::take(&mut current));
            }
            _ => {
                current.push(c);
            }
        }
    }

    // Push any trailing command text
    if !current.is_empty() {
        commands.push(current);
    }

    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn tokenize_simple() {
        let tokens = tokenize("map q2dm1");
        assert_eq!(tokens, vec!["map", "q2dm1"]);
    }

    #[test]
    fn tokenize_quoted() {
        let tokens = tokenize("say \"hello world\"");
        assert_eq!(tokens, vec!["say", "hello world"]);
    }

    #[test]
    fn tokenize_empty() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_extra_whitespace() {
        let tokens = tokenize("  map   q2dm1  ");
        assert_eq!(tokens, vec!["map", "q2dm1"]);
    }

    #[test]
    fn command_registration_and_execution() {
        let mut cmd = CmdSystem::new();
        let called = Rc::new(Cell::new(false));
        let called2 = called.clone();
        cmd.add_command("test", move |_args| {
            called2.set(true);
        });
        cmd.execute_string("test");
        assert!(called.get());
    }

    #[test]
    fn command_receives_args() {
        let mut cmd = CmdSystem::new();
        let received = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let received2 = received.clone();
        cmd.add_command("echo", move |args| {
            received2
                .borrow_mut()
                .extend(args.iter().map(|s| s.to_string()));
        });
        cmd.execute_string("echo hello world");
        assert_eq!(*received.borrow(), vec!["hello", "world"]);
    }

    #[test]
    fn cbuf_add_and_execute() {
        let mut cmd = CmdSystem::new();
        let log = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let log2 = log.clone();
        cmd.add_command("echo", move |args| {
            if let Some(first) = args.first() {
                log2.borrow_mut().push(first.to_string());
            }
        });
        cmd.cbuf_add_text("echo first\necho second\n");
        cmd.cbuf_execute();
        assert_eq!(*log.borrow(), vec!["first", "second"]);
    }

    #[test]
    fn cbuf_insert_runs_first() {
        let mut cmd = CmdSystem::new();
        let log = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let log2 = log.clone();
        cmd.add_command("log", move |args| {
            if let Some(first) = args.first() {
                log2.borrow_mut().push(first.to_string());
            }
        });
        cmd.cbuf_add_text("log second\n");
        cmd.cbuf_insert_text("log first\n");
        cmd.cbuf_execute();
        assert_eq!(*log.borrow(), vec!["first", "second"]);
    }

    #[test]
    fn semicolon_separates_commands() {
        let mut cmd = CmdSystem::new();
        let count = Rc::new(Cell::new(0u32));
        let count2 = count.clone();
        cmd.add_command("inc", move |_| {
            count2.set(count2.get() + 1);
        });
        cmd.cbuf_add_text("inc;inc;inc\n");
        cmd.cbuf_execute();
        assert_eq!(count.get(), 3);
    }

    #[test]
    fn remove_command() {
        let mut cmd = CmdSystem::new();
        let called = Rc::new(Cell::new(false));
        let called2 = called.clone();
        cmd.add_command("test", move |_| {
            called2.set(true);
        });
        cmd.remove_command("test");
        cmd.execute_string("test");
        assert!(!called.get());
    }

    #[test]
    fn exists_check() {
        let mut cmd = CmdSystem::new();
        cmd.add_command("test", |_| {});
        assert!(cmd.exists("test"));
        assert!(!cmd.exists("nonexistent"));
    }

    #[test]
    fn complete_command() {
        let mut cmd = CmdSystem::new();
        cmd.add_command("map", |_| {});
        cmd.add_command("maxclients", |_| {});
        cmd.add_command("quit", |_| {});
        let result = cmd.complete("ma");
        assert!(result.is_some());
        assert!(result.unwrap().starts_with("ma"));
    }

    #[test]
    fn defer_and_restore() {
        let mut cmd = CmdSystem::new();
        let log = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let log2 = log.clone();
        cmd.add_command("log", move |args| {
            if let Some(first) = args.first() {
                log2.borrow_mut().push(first.to_string());
            }
        });
        cmd.cbuf_add_text("log deferred\n");
        cmd.cbuf_copy_to_defer();
        // Buffer should now be empty
        cmd.cbuf_execute();
        assert!(log.borrow().is_empty());
        // Restore and execute
        cmd.cbuf_insert_from_defer();
        cmd.cbuf_execute();
        assert_eq!(*log.borrow(), vec!["deferred"]);
    }

    /// Registers `name` so every call appends `name args...` to the log.
    fn logging_command(
        cmd: &mut CmdSystem,
        name: &'static str,
    ) -> Rc<std::cell::RefCell<Vec<String>>> {
        let log = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let log2 = log.clone();
        cmd.add_command(name, move |args| {
            let mut line = name.to_string();
            for a in args {
                line.push(' ');
                line.push_str(a);
            }
            log2.borrow_mut().push(line);
        });
        log
    }

    #[test]
    fn newline_terminates_command_even_inside_unbalanced_quote() {
        // C Cbuf_Execute breaks at every '\n' regardless of quote parity, so
        // a stray quote cannot swallow the following lines.
        let mut cmd = CmdSystem::new();
        let say = logging_command(&mut cmd, "say");
        let quit = logging_command(&mut cmd, "quit");
        cmd.cbuf_add_text("say \"oops\nquit\n");
        cmd.cbuf_execute();
        assert_eq!(*say.borrow(), vec!["say oops"]);
        assert_eq!(*quit.borrow(), vec!["quit"]);
    }

    #[test]
    fn semicolon_inside_quotes_does_not_split() {
        let mut cmd = CmdSystem::new();
        let say = logging_command(&mut cmd, "say");
        cmd.cbuf_add_text("say \"a;b\";say c\n");
        cmd.cbuf_execute();
        assert_eq!(*say.borrow(), vec!["say a;b", "say c"]);
    }

    #[test]
    fn overlong_line_is_truncated_to_1023_bytes() {
        let mut cmd = CmdSystem::new();
        let echo = logging_command(&mut cmd, "echo");
        cmd.cbuf_add_text(&format!("echo {}\n", "x".repeat(2000)));
        cmd.cbuf_execute();
        assert_eq!(echo.borrow()[0].len(), 1023);
    }

    #[test]
    fn tokenize_skips_double_slash_comments() {
        assert_eq!(tokenize("echo hi // rest of line"), vec!["echo", "hi"]);
        assert!(tokenize("// only a comment").is_empty());
        assert_eq!(tokenize("echo http://x"), vec!["echo", "http://x"]);
    }

    #[test]
    fn tokenize_splits_on_control_bytes_but_not_unicode_spaces() {
        // C COM_Parse treats every byte <= ' ' as whitespace and nothing else.
        assert_eq!(tokenize("a\u{1}b"), vec!["a", "b"]);
        assert_eq!(tokenize("a\u{a0}b"), vec!["a\u{a0}b"]);
    }

    #[test]
    fn tokenize_keeps_at_most_80_tokens() {
        let text = vec!["t"; 100].join(" ");
        assert_eq!(tokenize(&text).len(), MAX_STRING_TOKENS);
    }

    #[test]
    fn tokenize_token_reaching_max_token_chars_becomes_empty() {
        // C COM_Parse: `if (len == MAX_TOKEN_CHARS) len = 0;`
        let ok = "y".repeat(MAX_TOKEN_CHARS - 1);
        let too_long = "z".repeat(MAX_TOKEN_CHARS);
        assert_eq!(
            tokenize(&format!("{ok} {too_long}")),
            vec![ok, String::new()]
        );
    }

    #[test]
    fn tokenize_empty_quoted_token() {
        assert_eq!(tokenize("set x \"\""), vec!["set", "x", ""]);
    }

    #[test]
    fn cbuf_add_text_drops_text_that_would_overflow() {
        let mut cmd = CmdSystem::new();
        cmd.cbuf_add_text(&"a".repeat(CBUF_MAX_SIZE - 1));
        cmd.cbuf_add_text("b");
        assert_eq!(cmd.buffer.len(), CBUF_MAX_SIZE - 1);
        assert!(!cmd.buffer.contains('b'));
    }

    #[test]
    fn add_command_keeps_first_registration() {
        let mut cmd = CmdSystem::new();
        let first = logging_command(&mut cmd, "dup");
        let second = Rc::new(Cell::new(false));
        let second2 = second.clone();
        cmd.add_command("dup", move |_| second2.set(true));
        cmd.execute_string("dup");
        assert_eq!(first.borrow().len(), 1);
        assert!(!second.get());
    }

    #[test]
    fn copy_to_defer_twice_keeps_latest_buffer() {
        let mut cmd = CmdSystem::new();
        let log = logging_command(&mut cmd, "log");
        cmd.cbuf_add_text("log a\n");
        cmd.cbuf_copy_to_defer();
        cmd.cbuf_add_text("log b\n");
        cmd.cbuf_copy_to_defer();
        cmd.cbuf_insert_from_defer();
        cmd.cbuf_execute();
        assert_eq!(*log.borrow(), vec!["log b"]);
    }

    #[test]
    fn unknown_command_no_panic() {
        let mut cmd = CmdSystem::new();
        // Should not panic on unknown command
        cmd.execute_string("nonexistent_command arg1 arg2");
    }
}
