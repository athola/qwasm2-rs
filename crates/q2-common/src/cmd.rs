//! Command buffer and parser system.
//!
//! Replaces C's `cmdparser.c` + `cbuf.c` with a safe, `HashMap`-based command
//! registry and a string-based command buffer. Commands are registered with
//! handlers, buffered text is tokenized and dispatched line-by-line.

use std::collections::HashMap;

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

    /// Register a console command with its handler.
    pub fn add_command(&mut self, name: &str, handler: impl Fn(&[&str]) + 'static) {
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
    pub fn cbuf_add_text(&mut self, text: &str) {
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
            let trimmed = line.trim();
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

/// Tokenize a command string, handling quoted strings.
///
/// `"say \"hello world\""` becomes `["say", "hello world"]`.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();

    loop {
        // Skip whitespace
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }

        if chars.peek().is_none() {
            break;
        }

        if chars.peek() == Some(&'"') {
            // Quoted token
            chars.next(); // consume opening quote
            let mut token = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                token.push(c);
            }
            tokens.push(token);
        } else {
            // Bare word token
            let mut token = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                token.push(c);
                chars.next();
            }
            tokens.push(token);
        }
    }

    tokens
}

/// Split a command buffer string into individual commands.
/// Splits on `\n` and `;`, but not semicolons inside quotes.
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
            '\n' | ';' if !in_quotes => {
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

    #[test]
    fn unknown_command_no_panic() {
        let mut cmd = CmdSystem::new();
        // Should not panic on unknown command
        cmd.execute_string("nonexistent_command arg1 arg2");
    }
}
