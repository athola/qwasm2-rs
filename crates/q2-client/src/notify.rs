//! Text the server sends: printed lines shown briefly at the top of the
//! screen, the centred message, and the server layout (scoreboard, help
//! computer). C ref: `CL_ParsePrint`, `CL_ParseLayout`
//! (client/cl_parse.c), `Con_Print`/`Con_DrawNotify` (client/cl_console.c)
//! and `SCR_CenterPrint`/`SCR_DrawCenterString` (client/cl_screen.c), at
//! HUD scale 1. The console itself is not ported.

use std::collections::VecDeque;

use q2_common::net_msg::NetMsg;
use q2_shared::constants::PRINT_CHAT;
use q2_shared::protocol::SvcOp;

use crate::layout::{wire_bytes, HudDraw};

/// Notify lines kept on screen (C `NUM_CON_TIMES`).
const NUM_CON_TIMES: usize = 4;
/// How long a printed line shows (C `con_notifytime` 3 s).
const CON_NOTIFYTIME_MS: i32 = 3000;
/// How long a centre print shows (C `scr_centertime` 2.5 s).
const SCR_CENTERTIME_MS: i32 = 2500;
/// Characters per centre-print line (C `SCR_DrawCenterString`).
const CENTER_LINE: usize = 40;

/// The server's text for the HUD: recent printed lines, the centre
/// print and the layout.
#[derive(Debug, Clone, Default)]
pub struct ServerText {
    /// The last few console lines with when each started (bit 7 of a
    /// byte selects the alternate colour), newest last.
    lines: VecDeque<(Vec<u8>, i32)>,
    /// Whether the newest line is still being printed.
    line_open: bool,
    /// A `\r` was printed and the next character replaces the newest line
    /// (C `Con_Print`'s static `cr`).
    carriage_return: bool,
    /// The centre print's lines as wire bytes, at most 40 each.
    center: Vec<Vec<u8>>,
    center_until: i32,
    /// The last `svc_layout` (C `cl.layout`), drawn while `STAT_LAYOUTS`
    /// bit 1 is set.
    pub layout: String,
}

impl ServerText {
    /// Take a print, centre print or layout message at `now` ms, wrapping
    /// printed text at `linewidth` characters (C `con.linewidth`). True
    /// when it was chat, which C greets with `misc/talk.wav`.
    pub fn message(&mut self, bytes: &[u8], now: i32, linewidth: usize) -> bool {
        let mut msg = NetMsg::from_bytes(bytes);
        msg.begin_reading();
        match SvcOp::try_from(msg.read_byte() as u8) {
            Ok(SvcOp::Print) => {
                let level = msg.read_byte();
                let text = msg.read_string();
                let chat = level == PRINT_CHAT;
                self.con_print(
                    &wire_bytes(&text).collect::<Vec<_>>(),
                    if chat { 0x80 } else { 0 },
                    now,
                    linewidth.max(1),
                );
                chat
            }
            Ok(SvcOp::CenterPrint) => {
                self.center_print(&msg.read_string(), now);
                false
            }
            Ok(SvcOp::Layout) => {
                self.layout = msg.read_string();
                false
            }
            _ => false,
        }
    }

    /// C `Con_Print`: words wrap whole unless longer than a line. A first
    /// byte of 1 or 2 is dropped and colours the whole print; `\r` makes
    /// the next character, in this print or a later one, start the line
    /// over.
    fn con_print(&mut self, text: &[u8], ormask: u8, now: i32, linewidth: usize) {
        let (text, ormask) = match text {
            [1 | 2, rest @ ..] => (rest, ormask | 0x80),
            _ => (text, ormask),
        };
        for (i, &c) in text.iter().enumerate() {
            // Count the word's length to wrap it whole. C tests
            // `txt[l] <= ' '` on a signed char, so 0x80-0xFF end it too.
            let word = text[i..]
                .iter()
                .take_while(|&&b| b > b' ' && b < 0x80)
                .take(linewidth)
                .count();
            let x = self.line_len();
            if word != linewidth && self.line_open && x + word > linewidth {
                self.line_open = false;
            }
            // C `con.current--`: the line Con_Linefeed opens below is the
            // one the `\r` ended, cleared and stamped with a new time.
            if self.carriage_return {
                self.carriage_return = false;
                self.lines.pop_back();
            }
            if !self.line_open {
                self.lines.push_back((Vec::new(), now));
                if self.lines.len() > NUM_CON_TIMES {
                    self.lines.pop_front();
                }
                self.line_open = true;
            }
            match c {
                b'\n' => self.line_open = false,
                b'\r' => {
                    self.line_open = false;
                    self.carriage_return = true;
                }
                _ => {
                    if let Some((line, _)) = self.lines.back_mut() {
                        line.push(c | ormask);
                        if line.len() >= linewidth {
                            self.line_open = false;
                        }
                    }
                }
            }
        }
    }

    fn line_len(&self) -> usize {
        self.lines
            .back()
            .filter(|_| self.line_open)
            .map_or(0, |(l, _)| l.len())
    }

    /// C `SCR_CenterPrint`: show `text` centred and clear the notify
    /// lines (C `Con_ClearNotify`). One entry per `'\n'`-separated line,
    /// so the count is C's `scr_center_lines`; C `SCR_DrawCenterString`
    /// draws at most 40 characters of each and skips the rest.
    fn center_print(&mut self, text: &str, now: i32) {
        self.center = text
            .split('\n')
            .map(|line| wire_bytes(line).take(CENTER_LINE).collect())
            .collect();
        self.center_until = now + SCR_CENTERTIME_MS;
        self.lines.clear();
        self.line_open = false;
    }

    /// The notify lines (C `Con_DrawNotify`, one character in from the
    /// left, down from the top) and the centre print (C
    /// `SCR_DrawCenterString`: from 35% down for up to four lines, else
    /// from 48).
    pub fn draws(&self, width: i32, height: i32, now: i32) -> Vec<HudDraw> {
        let mut out = Vec::new();
        let mut v = 0;
        for (line, start) in &self.lines {
            if now - start > CON_NOTIFYTIME_MS {
                continue;
            }
            for (x, &ch) in line.iter().enumerate() {
                out.push(HudDraw::Char {
                    x: (x as i32 + 1) * 8,
                    y: v,
                    ch,
                });
            }
            v += 8;
        }
        if now < self.center_until {
            let mut y = if self.center.len() <= 4 {
                // C `(viddef.height * 0.35) / scale`, a double product.
                (f64::from(height) * 0.35) as i32
            } else {
                48
            };
            for line in &self.center {
                let x = (width - line.len() as i32 * 8) / 2;
                for (i, &ch) in line.iter().enumerate() {
                    out.push(HudDraw::Char {
                        x: x + 8 * i as i32,
                        y,
                        ch,
                    });
                }
                y += 8;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use q2_shared::constants::{PRINT_CHAT, PRINT_HIGH};
    use q2_shared::protocol::SvcOp;

    fn print(level: i32, text: &str) -> Vec<u8> {
        let mut m = vec![SvcOp::Print as u8, level as u8];
        m.extend_from_slice(text.as_bytes());
        m.push(0);
        m
    }

    fn center(text: &str) -> Vec<u8> {
        let mut m = vec![SvcOp::CenterPrint as u8];
        m.extend_from_slice(text.as_bytes());
        m.push(0);
        m
    }

    fn rows(draws: &[HudDraw]) -> Vec<(i32, String)> {
        let mut out: Vec<(i32, String)> = Vec::new();
        for d in draws {
            if let HudDraw::Char { y, ch, .. } = d {
                match out.last_mut() {
                    Some((ly, text)) if ly == y => text.push((ch & 0x7f) as char),
                    _ => out.push((*y, ((ch & 0x7f) as char).to_string())),
                }
            }
        }
        out
    }

    #[test]
    fn printed_lines_show_at_the_top_for_three_seconds() {
        let mut text = ServerText::default();
        text.message(&print(PRINT_HIGH, "Ann died.\n"), 1000, 80);
        let draws = text.draws(640, 480, 1000);
        assert_eq!(rows(&draws), [(0, "Ann died.".to_string())]);
        assert!(
            matches!(draws[0], HudDraw::Char { x: 8, .. }),
            "one char in"
        );
        assert!(text.draws(640, 480, 4001).is_empty(), "gone after 3 s");
    }

    #[test]
    fn only_the_last_four_lines_show_and_long_lines_wrap() {
        let mut text = ServerText::default();
        for n in 1..=5 {
            text.message(&print(PRINT_HIGH, &format!("line {n}\n")), 0, 80);
        }
        let shown: Vec<String> = rows(&text.draws(640, 480, 0))
            .into_iter()
            .map(|r| r.1)
            .collect();
        assert_eq!(shown, ["line 2", "line 3", "line 4", "line 5"]);

        let mut text = ServerText::default();
        text.message(&print(PRINT_HIGH, "abcdefghij\n"), 0, 4);
        let shown: Vec<String> = rows(&text.draws(640, 480, 0))
            .into_iter()
            .map(|r| r.1)
            .collect();
        // C counts the rest of the word at every character, so the tail
        // "hij" no longer fits after "efg" and wraps.
        assert_eq!(shown, ["abcd", "efg", "hij"]);
    }

    #[test]
    fn a_line_without_a_newline_waits_for_the_rest() {
        let mut text = ServerText::default();
        text.message(&print(PRINT_HIGH, "Ann "), 0, 80);
        // The open line shows as far as it has been printed.
        assert_eq!(rows(&text.draws(640, 480, 0))[0].1.trim_end(), "Ann");
        text.message(&print(PRINT_HIGH, "died.\n"), 0, 80);
        assert_eq!(
            rows(&text.draws(640, 480, 0)).last().unwrap().1,
            "Ann died."
        );
    }

    #[test]
    fn chat_plays_the_talk_sound_and_shows_in_the_alternate_colour() {
        let mut text = ServerText::default();
        assert!(!text.message(&print(PRINT_HIGH, "x\n"), 0, 80));
        assert!(text.message(&print(PRINT_CHAT, "Bob: hi\n"), 0, 80));
        let draws = text.draws(640, 480, 0);
        let last = draws.last().unwrap();
        assert!(matches!(last, HudDraw::Char { ch, .. } if ch & 0x80 != 0));
    }

    #[test]
    fn a_centre_print_shows_centred_for_two_and_a_half_seconds() {
        let mut text = ServerText::default();
        text.message(&print(PRINT_HIGH, "old\n"), 0, 80);
        text.message(&center("The door is locked\nUse the key"), 0, 80);
        let draws = text.draws(640, 480, 100);
        let r = rows(&draws);
        // Notify lines are cleared; two centred lines from 35% down.
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], (168, "The door is locked".to_string()));
        assert_eq!(r[1], (176, "Use the key".to_string()));
        assert!(matches!(draws[0], HudDraw::Char { x, .. } if x == (640 - 18 * 8) / 2));
        assert!(text.draws(640, 480, 2600).is_empty());
    }

    #[test]
    fn a_centre_print_line_is_cut_at_40_characters() {
        let mut text = ServerText::default();
        let long = "x".repeat(60);
        text.message(&center(&format!("{long}\na\nb\nc")), 0, 80);
        // C draws the first 40 characters, drops the rest and, counting
        // four lines, still starts from 35% down.
        let r = rows(&text.draws(640, 480, 0));
        assert_eq!(r.len(), 4);
        assert_eq!(r[0], (168, "x".repeat(40)));
        assert_eq!(r[1], (176, "a".to_string()));
    }

    #[test]
    fn the_centre_print_row_is_35_percent_in_double() {
        let mut text = ServerText::default();
        text.message(&center("hi"), 0, 80);
        // C `viddef.height * 0.35` is a double: 251.99... at 720, not the
        // 252 an f32 product rounds to.
        assert_eq!(rows(&text.draws(640, 720, 0))[0].0, 251);
        assert_eq!(rows(&text.draws(640, 360, 0))[0].0, 125);
    }

    #[test]
    fn a_high_byte_draws_as_one_glyph_of_its_value() {
        // NetMsg::read_string keeps byte 0xE1 as the char U+00E1, two
        // bytes in UTF-8; C draws the one byte.
        let mut text = ServerText::default();
        text.message(
            &[SvcOp::Print as u8, PRINT_HIGH as u8, 0xE1, b'\n', 0],
            0,
            80,
        );
        let draws = text.draws(640, 480, 0);
        assert_eq!(
            draws,
            [HudDraw::Char {
                x: 8,
                y: 0,
                ch: 0xE1
            }]
        );

        text.message(&[SvcOp::CenterPrint as u8, 0xE1, 0], 0, 80);
        let draws = text.draws(640, 480, 0);
        assert_eq!(
            draws,
            [HudDraw::Char {
                x: (640 - 8) / 2,
                y: 168,
                ch: 0xE1
            }]
        );
    }

    fn line_bytes(text: &ServerText) -> Vec<Vec<u8>> {
        text.lines.iter().map(|(line, _)| line.clone()).collect()
    }

    #[test]
    fn a_high_byte_ends_a_word_for_wrapping_like_signed_char_c() {
        // C counts the word with `txt[l] <= ' '` on a signed char, so
        // 0xE1 stops the count: "ab\xE1" fits, then "cd" wraps.
        let mut text = ServerText::default();
        text.message(
            &[
                SvcOp::Print as u8,
                PRINT_HIGH as u8,
                b'a',
                b'b',
                0xE1,
                b'c',
                b'd',
                b'\n',
                0,
            ],
            0,
            4,
        );
        assert_eq!(line_bytes(&text), [vec![b'a', b'b', 0xE1], b"cd".to_vec()]);
    }

    #[test]
    fn a_carriage_return_replaces_the_line_with_what_follows() {
        // C Con_Print: `\r` sets con.x = 0 and cr, so the next character
        // steps back a line and Con_Linefeed clears it.
        let mut text = ServerText::default();
        text.message(&print(PRINT_HIGH, "abc\rxy\n"), 0, 80);
        assert_eq!(line_bytes(&text), [b"xy".to_vec()]);
    }

    #[test]
    fn a_trailing_carriage_return_waits_for_the_next_print_and_restarts_the_time() {
        // C keeps `cr` in a static, so it carries to the next Con_Print.
        let mut text = ServerText::default();
        text.message(&print(PRINT_HIGH, "abc\r"), 0, 80);
        text.message(&print(PRINT_HIGH, "x\n"), 1000, 80);
        assert_eq!(line_bytes(&text), [b"x".to_vec()]);
        assert_eq!(rows(&text.draws(640, 480, 3500)), [(0, "x".to_string())]);
    }

    #[test]
    fn a_leading_control_byte_one_or_two_colours_the_whole_print() {
        // C Con_Print: `txt[0] == 1 || txt[0] == 2` sets mask 128 and
        // skips that byte.
        for lead in ["\u{1}", "\u{2}"] {
            let mut text = ServerText::default();
            text.message(&print(PRINT_HIGH, &format!("{lead}hi\n")), 0, 80);
            assert_eq!(line_bytes(&text), [vec![b'h' | 0x80, b'i' | 0x80]]);
        }
        // Only the first byte switches colour.
        let mut text = ServerText::default();
        text.message(&print(PRINT_HIGH, "a\u{1}\n"), 0, 80);
        assert_eq!(line_bytes(&text), [vec![b'a', 1]]);
    }

    #[test]
    fn a_long_centre_print_starts_near_the_top() {
        let mut text = ServerText::default();
        text.message(&center("1\n2\n3\n4\n5"), 0, 80);
        assert_eq!(rows(&text.draws(640, 480, 0))[0].0, 48);
    }

    #[test]
    fn a_layout_message_replaces_the_layout() {
        let mut text = ServerText::default();
        let mut m = vec![SvcOp::Layout as u8];
        m.extend_from_slice(b"xv 0 yv 0 string hi\0");
        text.message(&m, 0, 80);
        assert_eq!(text.layout, "xv 0 yv 0 string hi");
    }

    #[test]
    fn other_messages_are_left_alone() {
        let mut text = ServerText::default();
        assert!(!text.message(&[SvcOp::MuzzleFlash as u8, 1, 0, 0], 0, 80));
        assert!(!text.message(&[], 0, 80));
        assert!(text.draws(640, 480, 0).is_empty());
    }
}
