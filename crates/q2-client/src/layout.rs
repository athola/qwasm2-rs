//! HUD layout programs: the status bar (`CS_STATUSBAR`) and server layouts
//! are small programs over the player's stats. C ref:
//! `SCR_ExecuteLayoutString` (client/cl_screen.c), at HUD scale 1.

use q2_shared::constants::{
    CS_IMAGES, CS_PLAYERSKINS, MAX_CLIENTS, STAT_AMMO, STAT_ARMOR, STAT_FLASHES, STAT_HEALTH,
};

/// One 2D draw the renderer performs, in screen pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HudDraw {
    /// `Draw_Pic`: a pic name as C passes it (`num_0`, `i_health`, ...).
    Pic { x: i32, y: i32, name: String },
    /// `Draw_Char`: a conchars glyph (bit 7 selects the alternate colour).
    Char { x: i32, y: i32, ch: u8 },
}

/// What a layout program reads.
pub struct LayoutInput<'a> {
    pub width: i32,
    pub height: i32,
    pub stats: &'a [i16; 32],
    /// The client's configstrings, indexed like `cl.configstrings`.
    pub configstrings: &'a dyn Fn(usize) -> &'a str,
    /// `cl.frame.serverframe`, for the low-value flash.
    pub serverframe: i32,
}

/// `sb_nums`: big digits, normal and alternate colour.
const SB_NUMS: [[&str; 11]; 2] = [
    [
        "num_0",
        "num_1",
        "num_2",
        "num_3",
        "num_4",
        "num_5",
        "num_6",
        "num_7",
        "num_8",
        "num_9",
        "num_minus",
    ],
    [
        "anum_0",
        "anum_1",
        "anum_2",
        "anum_3",
        "anum_4",
        "anum_5",
        "anum_6",
        "anum_7",
        "anum_8",
        "anum_9",
        "anum_minus",
    ],
];
const CHAR_WIDTH: i32 = 16;
const STAT_MINUS: usize = 10;

/// C COM_Parse over a whole program: whitespace-separated tokens, `"`
/// quoted strings, `//` comments. Unlike `cmd::tokenize` (Cmd_TokenizeString)
/// it has no 80-token limit, as server layouts can be longer.
fn tokens(program: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = program;
    loop {
        rest = rest.trim_start_matches(|c: char| c <= ' ');
        if rest.is_empty() {
            return out;
        }
        if let Some(comment) = rest.strip_prefix("//") {
            rest = comment.split_once('\n').map_or("", |(_, r)| r);
            continue;
        }
        if let Some(quoted) = rest.strip_prefix('"') {
            let (token, r) = quoted.split_once('"').unwrap_or((quoted, ""));
            out.push(token);
            rest = r;
            continue;
        }
        let end = rest.find(|c: char| c <= ' ').unwrap_or(rest.len());
        out.push(&rest[..end]);
        rest = &rest[end..];
    }
}

/// C strtol(token, NULL, 10): leading sign and digits, 0 when none.
fn strtol(token: &str) -> i32 {
    let digits_end = token
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))))
        .map_or(token.len(), |(i, _)| i);
    token[..digits_end].parse().unwrap_or(0)
}

/// A stat by a program-supplied index; out-of-range indexes read 0 (C
/// drops the connection for some and reads past the array for others).
fn stat(stats: &[i16; 32], index: i32) -> i32 {
    usize::try_from(index)
        .ok()
        .and_then(|i| stats.get(i))
        .map_or(0, |&v| i32::from(v))
}

/// C SCR_DrawField.
fn draw_field(out: &mut Vec<HudDraw>, x: i32, y: i32, color: usize, width: i32, value: i32) {
    if width < 1 {
        return;
    }
    let width = width.min(5);
    let num = value.to_string();
    let l = (num.len() as i32).min(width);
    let mut x = x + 2 + CHAR_WIDTH * (width - l);
    for ch in num.bytes().take(l as usize) {
        let frame = if ch == b'-' {
            STAT_MINUS
        } else {
            usize::from(ch - b'0')
        };
        out.push(HudDraw::Pic {
            x,
            y,
            name: SB_NUMS[color][frame].to_string(),
        });
        x += CHAR_WIDTH;
    }
}

/// C DrawString / DrawAltString (`xor` 0x80).
fn draw_string(out: &mut Vec<HudDraw>, x: i32, y: i32, text: &str, xor: u8) {
    for (i, ch) in text.bytes().enumerate() {
        out.push(HudDraw::Char {
            x: x + 8 * i as i32,
            y,
            ch: ch ^ xor,
        });
    }
}

/// C DrawHUDString: lines centred in `centerwidth` (0 = left aligned).
fn draw_hud_string(out: &mut Vec<HudDraw>, x: i32, y: i32, centerwidth: i32, text: &str, xor: u8) {
    for (n, line) in text.split('\n').enumerate() {
        let lx = if centerwidth != 0 {
            x + (centerwidth - line.len() as i32 * 8) / 2
        } else {
            x
        };
        draw_string(out, lx, y + 8 * n as i32, line, xor);
    }
}

/// A player's name and icon from its `name\\model/skin` configstring,
/// the base `male/grunt` icon without a skin. C ref: `CL_LoadClientinfo`
/// (cl_parse.c), without its check that the icon file exists.
fn client_info(s: &str) -> (String, String) {
    let (name, skin) = s.split_once('\\').unwrap_or((s, ""));
    let icon = match skin.split_once('/') {
        Some((model, skin)) if !model.is_empty() && !skin.is_empty() => {
            format!("/players/{model}/{skin}_i.pcx")
        }
        _ => "/players/male/grunt_i.pcx".to_string(),
    };
    (name.to_string(), icon)
}

/// The next token, or "" at the end (C COM_Parse returns "").
fn arg<'a>(toks: &mut impl Iterator<Item = &'a str>) -> &'a str {
    toks.next().unwrap_or("")
}

/// Run a layout program and return its draws in order.
pub fn execute_layout(program: &str, input: &LayoutInput) -> Vec<HudDraw> {
    let mut out = Vec::new();
    let stats = input.stats;
    let (mut x, mut y) = (0, 0);
    let mut toks = tokens(program).into_iter();
    while let Some(token) = toks.next() {
        match token {
            "xl" => x = strtol(arg(&mut toks)),
            "xr" => x = input.width + strtol(arg(&mut toks)),
            "xv" => x = input.width / 2 - 160 + strtol(arg(&mut toks)),
            "yt" => y = strtol(arg(&mut toks)),
            "yb" => y = input.height + strtol(arg(&mut toks)),
            "yv" => y = input.height / 2 - 120 + strtol(arg(&mut toks)),
            "pic" => {
                let value = stat(stats, strtol(arg(&mut toks)));
                if let Ok(value) = usize::try_from(value) {
                    let name = (input.configstrings)(CS_IMAGES + value);
                    if !name.is_empty() {
                        out.push(HudDraw::Pic {
                            x,
                            y,
                            name: name.to_string(),
                        });
                    }
                }
            }
            "picn" => out.push(HudDraw::Pic {
                x,
                y,
                name: arg(&mut toks).to_string(),
            }),
            "num" => {
                let width = strtol(arg(&mut toks));
                let value = stat(stats, strtol(arg(&mut toks)));
                draw_field(&mut out, x, y, 0, width, value);
            }
            "hnum" => {
                let value = i32::from(stats[STAT_HEALTH]);
                let color = if value > 25 {
                    0 // green
                } else if value > 0 {
                    ((input.serverframe >> 2) & 1) as usize // flash
                } else {
                    1
                };
                if stats[STAT_FLASHES] & 1 != 0 {
                    out.push(HudDraw::Pic {
                        x,
                        y,
                        name: "field_3".into(),
                    });
                }
                draw_field(&mut out, x, y, color, 3, value);
            }
            "anum" => {
                let value = i32::from(stats[STAT_AMMO]);
                let color = if value > 5 {
                    0
                } else if value >= 0 {
                    ((input.serverframe >> 2) & 1) as usize
                } else {
                    continue; // negative: don't show
                };
                if stats[STAT_FLASHES] & 4 != 0 {
                    out.push(HudDraw::Pic {
                        x,
                        y,
                        name: "field_3".into(),
                    });
                }
                draw_field(&mut out, x, y, color, 3, value);
            }
            "rnum" => {
                let value = i32::from(stats[STAT_ARMOR]);
                if value < 1 {
                    continue;
                }
                if stats[STAT_FLASHES] & 2 != 0 {
                    out.push(HudDraw::Pic {
                        x,
                        y,
                        name: "field_3".into(),
                    });
                }
                draw_field(&mut out, x, y, 0, 3, value);
            }
            "stat_string" => {
                let index = stat(stats, strtol(arg(&mut toks)));
                if let Ok(index) = usize::try_from(index) {
                    let text = (input.configstrings)(index);
                    draw_string(&mut out, x, y, text, 0);
                }
            }
            "cstring" => draw_hud_string(&mut out, x, y, 320, arg(&mut toks), 0),
            "cstring2" => draw_hud_string(&mut out, x, y, 320, arg(&mut toks), 0x80),
            "string" => draw_string(&mut out, x, y, arg(&mut toks), 0),
            "string2" => draw_string(&mut out, x, y, arg(&mut toks), 0x80),
            // The guard consumes the stat index; a true condition falls
            // through to `_` and the block runs.
            "if" if stat(stats, strtol(arg(&mut toks))) == 0 => {
                // Skip to the matching endif.
                toks.by_ref().find(|&t| t == "endif");
            }
            // A deathmatch scoreboard entry; "ctf" is not ported.
            "client" => {
                let bx = input.width / 2 - 160 + strtol(arg(&mut toks));
                let by = input.height / 2 - 120 + strtol(arg(&mut toks));
                let value = strtol(arg(&mut toks));
                let (score, ping, time) = (
                    strtol(arg(&mut toks)),
                    strtol(arg(&mut toks)),
                    strtol(arg(&mut toks)),
                );
                // C drops the connection for a bad client number.
                let Some(index) = usize::try_from(value).ok().filter(|&v| v < MAX_CLIENTS) else {
                    continue;
                };
                let (name, icon) = client_info((input.configstrings)(CS_PLAYERSKINS + index));
                draw_string(&mut out, bx + 32, by, &name, 0x80);
                draw_string(&mut out, bx + 32, by + 8, "Score: ", 0);
                draw_string(&mut out, bx + 32 + 7 * 8, by + 8, &score.to_string(), 0x80);
                draw_string(&mut out, bx + 32, by + 16, &format!("Ping:  {ping}"), 0);
                draw_string(&mut out, bx + 32, by + 24, &format!("Time:  {time}"), 0);
                out.push(HudDraw::Pic {
                    x: bx,
                    y: by,
                    name: icon,
                });
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: i32 = 640;
    const H: i32 = 480;

    fn run(program: &str, stats: &[i16; 32], strings: &[(usize, &'static str)]) -> Vec<HudDraw> {
        let lookup =
            |i: usize| -> &str { strings.iter().find(|(k, _)| *k == i).map_or("", |(_, v)| v) };
        execute_layout(
            program,
            &LayoutInput {
                width: W,
                height: H,
                stats,
                configstrings: &lookup,
                serverframe: 0,
            },
        )
    }

    fn pic(x: i32, y: i32, name: &str) -> HudDraw {
        HudDraw::Pic {
            x,
            y,
            name: name.into(),
        }
    }

    fn chars_at(draws: &[HudDraw], y: i32) -> String {
        draws
            .iter()
            .filter_map(|d| match d {
                HudDraw::Char { y: cy, ch, .. } if *cy == y => Some((ch & 0x7f) as char),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_client_block_draws_the_players_icon_name_score_ping_and_time() {
        use q2_shared::constants::CS_PLAYERSKINS;
        let draws = run(
            "client 0 32 1 7 50 3",
            &[0; 32],
            &[(CS_PLAYERSKINS + 1, "Bob\\male/grunt")],
        );
        let (x, y) = (W / 2 - 160, H / 2 - 120 + 32);
        assert!(draws.contains(&pic(x, y, "/players/male/grunt_i.pcx")));
        assert_eq!(chars_at(&draws, y), "Bob");
        assert_eq!(chars_at(&draws, y + 8), "Score: 7");
        assert_eq!(chars_at(&draws, y + 16), "Ping:  50");
        assert_eq!(chars_at(&draws, y + 24), "Time:  3");
        // The name and the score are in the alternate colour.
        let name = draws
            .iter()
            .find(|d| matches!(d, HudDraw::Char { y: cy, .. } if *cy == y));
        assert!(matches!(name, Some(HudDraw::Char { ch, .. }) if ch & 0x80 != 0));
    }

    #[test]
    fn a_client_block_without_a_skin_uses_the_base_player_icon() {
        let draws = run("client 0 0 3 0 0 0", &[0; 32], &[]);
        assert!(draws.iter().any(
            |d| matches!(d, HudDraw::Pic { name, .. } if name == "/players/male/grunt_i.pcx")
        ));
    }

    #[test]
    fn hnum_draws_right_aligned_big_digits_at_the_xv_yb_origin() {
        let mut stats = [0; 32];
        stats[STAT_HEALTH] = 100;
        let draws = run("yb -24 xv 0 hnum", &stats, &[]);
        let (x, y) = (W / 2 - 160 + 2, H - 24);
        assert_eq!(
            draws,
            vec![
                pic(x, y, "num_1"),
                pic(x + 16, y, "num_0"),
                pic(x + 32, y, "num_0")
            ]
        );
    }

    #[test]
    fn low_health_flashes_and_zero_health_is_alternate_coloured() {
        let mut stats = [0; 32];
        stats[STAT_HEALTH] = -5;
        let draws = run("hnum", &stats, &[]);
        assert_eq!(draws[0], pic(2 + 16, 0, "anum_minus"), "width 3, two chars");
        assert_eq!(draws[1], pic(2 + 32, 0, "anum_5"));
    }

    #[test]
    fn pic_reads_the_image_name_its_stat_indexes() {
        let mut stats = [0; 32];
        stats[0] = 3;
        let draws = run("xl 10 yt 20 pic 0", &stats, &[(CS_IMAGES + 3, "i_health")]);
        assert_eq!(draws, vec![pic(10, 20, "i_health")]);
    }

    #[test]
    fn if_skips_to_endif_when_its_stat_is_zero() {
        let stats = [0; 32];
        assert!(run("if 4 xv 200 rnum picn x endif", &stats, &[]).is_empty());
        let mut stats = [0; 32];
        stats[4] = 1;
        assert_eq!(run("if 4 picn x endif", &stats, &[]), vec![pic(0, 0, "x")]);
    }

    #[test]
    fn rnum_hides_no_armor_and_anum_hides_negative_ammo() {
        let mut stats = [0; 32];
        stats[STAT_AMMO] = -1;
        assert!(run("rnum anum", &stats, &[]).is_empty());
    }

    #[test]
    fn stat_string_draws_the_configstring_the_stat_names() {
        let mut stats = [0; 32];
        stats[8] = 1000;
        let draws = run("stat_string 8", &stats, &[(1000, "Hi")]);
        assert_eq!(
            draws,
            vec![
                HudDraw::Char {
                    x: 0,
                    y: 0,
                    ch: b'H'
                },
                HudDraw::Char {
                    x: 8,
                    y: 0,
                    ch: b'i'
                }
            ]
        );
    }

    #[test]
    fn quoted_strings_and_alt_colour_and_centring() {
        let stats = [0; 32];
        let draws = run("string2 \"a b\" cstring \"ab\"", &stats, &[]);
        assert_eq!(
            draws[0],
            HudDraw::Char {
                x: 0,
                y: 0,
                ch: b'a' ^ 0x80
            }
        );
        assert_eq!(
            draws[1],
            HudDraw::Char {
                x: 8,
                y: 0,
                ch: b' ' ^ 0x80
            }
        );
        assert_eq!(
            draws[3],
            HudDraw::Char {
                x: (320 - 16) / 2,
                y: 0,
                ch: b'a'
            }
        );
    }

    #[test]
    fn the_whole_single_player_status_bar_draws_health_and_its_icon() {
        let bar = include_str!("../tests/fixtures/single_statusbar.txt");
        assert_eq!(tokens(bar).len(), bar.split_whitespace().count());
        let mut stats = [0; 32];
        stats[STAT_HEALTH] = 100;
        stats[0] = 1;
        let draws = run(bar, &stats, &[(CS_IMAGES + 1, "i_health")]);
        assert!(
            draws.contains(&pic(W / 2 - 160 + 50, H - 24, "i_health")),
            "{draws:?}"
        );
    }

    #[test]
    fn field_width_is_capped_at_five_and_values_truncate() {
        let mut out = Vec::new();
        draw_field(&mut out, 0, 0, 0, 9, 1_234_567);
        assert_eq!(out.len(), 5);
        assert_eq!(out[0], pic(2, 0, "num_1"));
    }
}
