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

use crate::layout::HudDraw;

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
    center: Vec<String>,
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
                    text.as_bytes(),
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

    /// C `Con_Print`: words wrap whole unless longer than a line.
    fn con_print(&mut self, text: &[u8], ormask: u8, now: i32, linewidth: usize) {
        let mut i = 0;
        while i < text.len() {
            let c = text[i];
            // Count the word's length to wrap it whole.
            let word = text[i..]
                .iter()
                .take_while(|&&b| b > b' ')
                .take(linewidth)
                .count();
            let x = self.line_len();
            if word != linewidth && self.line_open && x + word > linewidth {
                self.line_open = false;
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
                b'\r' => {}
                _ => {
                    if let Some((line, _)) = self.lines.back_mut() {
                        line.push(c | ormask);
                        if line.len() >= linewidth {
                            self.line_open = false;
                        }
                    }
                }
            }
            i += 1;
        }
    }

    fn line_len(&self) -> usize {
        self.lines
            .back()
            .filter(|_| self.line_open)
            .map_or(0, |(l, _)| l.len())
    }

    /// C `SCR_CenterPrint`: show `text` centred and clear the notify
    /// lines (C `Con_ClearNotify`).
    fn center_print(&mut self, text: &str, now: i32) {
        self.center = text
            .split('\n')
            .flat_map(|line| {
                let chars: Vec<char> = line.chars().collect();
                if chars.is_empty() {
                    vec![String::new()]
                } else {
                    chars
                        .chunks(CENTER_LINE)
                        .map(|c| c.iter().collect())
                        .collect()
                }
            })
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
                (height as f32 * 0.35) as i32
            } else {
                48
            };
            for line in &self.center {
                let x = (width - line.chars().count() as i32 * 8) / 2;
                for (i, ch) in line.bytes().enumerate() {
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
