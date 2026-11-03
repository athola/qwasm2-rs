//! HUD layout programs: the status bar (`CS_STATUSBAR`) and server layouts
//! are small programs over the player's stats. C ref:
//! `SCR_ExecuteLayoutString` (client/cl_screen.c), at HUD scale 1.

use q2_common::cmd::com_parse;
use q2_shared::constants::{
    CS_IMAGES, CS_PLAYERSKINS, MAX_CLIENTS, MAX_QPATH, STAT_AMMO, STAT_ARMOR, STAT_FLASHES,
    STAT_HEALTH,
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
    /// Whether a game file exists, standing in for the `R_RegisterModel`,
    /// `R_RegisterSkin` and `Draw_FindPic` loads that `CL_LoadClientinfo`
    /// checks before it trusts a player's icon.
    pub file_exists: &'a dyn Fn(&str) -> bool,
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

/// C COM_Parse over a whole program. Unlike `cmd::tokenize`
/// (Cmd_TokenizeString) it crosses newlines and has no 80-token limit, as
/// server layouts can be longer.
fn tokens(program: &str) -> Vec<&str> {
    let mut rest = program;
    std::iter::from_fn(|| {
        let (token, after) = com_parse(rest)?;
        rest = after;
        Some(token)
    })
    .collect()
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

/// The wire bytes of a string `NetMsg::read_string` returned, one char
/// per byte (U+0000..U+00FF), mapped back the way `NetMsg::write_string`
/// maps them. C ref: the raw `char`s C passes to `Draw_Char`.
pub(crate) fn wire_bytes(text: &str) -> impl Iterator<Item = u8> + '_ {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
}

/// C DrawString / DrawAltString (`xor` 0x80).
fn draw_string(out: &mut Vec<HudDraw>, x: i32, y: i32, text: &str, xor: u8) {
    for (i, ch) in wire_bytes(text).enumerate() {
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
            x + (centerwidth - line.chars().count() as i32 * 8) / 2
        } else {
            x
        };
        draw_string(out, lx, y + 8 * n as i32, line, xor);
    }
}

/// `cl.baseclientinfo`'s icon, loaded from `unnamed\male/grunt`. C ref:
/// CL_PrepRefresh (client/cl_view.c).
const BASE_ICON: &str = "/players/male/grunt_i.pcx";

/// `s` cut at `MAX_QPATH - 1` chars, as C `Q_strlcpy` / `Com_sprintf` cut
/// a string into a `[MAX_QPATH]` buffer.
fn qpath(s: &str) -> &str {
    let end = s
        .char_indices()
        .nth(MAX_QPATH - 1)
        .map_or(s.len(), |(i, _)| i);
    &s[..end]
}

/// A player's name and scoreboard icon from its `name\model/skin`
/// configstring, given which game files exist.
///
/// C ref: `CL_LoadClientinfo` (client/cl_parse.c): the model and skin split
/// on `/`, else `\`; a model without `tris.md2` becomes `male`; a skin
/// missing for a model other than `male` (any case) switches the model to
/// `male`; a skin still missing falls back to that model's `grunt.pcx`; a
/// `cyborg` without `weapon.md2` borrows the male one. The icon name uses
/// the final model and the original skin. `cl_noskins` is not ported (C
/// default 0). When the skin, icon, model or weapon model is missing,
/// `SCR_ExecuteLayoutString` (client/cl_screen.c) draws [`BASE_ICON`].
fn client_info(s: &str, exists: &dyn Fn(&str) -> bool) -> (String, String) {
    let s = qpath(s);
    // C leaves `s` at the start of the string when there is no backslash.
    let (name, s) = s.split_once('\\').unwrap_or((s, s));
    if s.is_empty() {
        return (name.to_string(), BASE_ICON.to_string());
    }
    // C cuts model_name at the separator, or at its start when there is
    // none, and takes skin_name from one char past that cut.
    let (mut model, skin) = match s.find('/').or_else(|| s.find('\\')) {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => ("", &s[s.chars().next().map_or(0, char::len_utf8)..]),
    };
    let file = |path: &str| exists(qpath(path));
    let mut model_ok = file(&format!("players/{model}/tris.md2"));
    if !model_ok {
        model = "male";
        model_ok = file("players/male/tris.md2");
    }
    let mut skin_ok = file(&format!("players/{model}/{skin}.pcx"));
    if !skin_ok && !model.eq_ignore_ascii_case("male") {
        model = "male";
        model_ok = file("players/male/tris.md2");
        skin_ok = file(&format!("players/male/{skin}.pcx"));
    }
    if !skin_ok {
        skin_ok = file(&format!("players/{model}/grunt.pcx"));
    }
    let weapon_ok = file(&format!("players/{model}/weapon.md2"))
        || (model == "cyborg" && file("players/male/weapon.md2"));
    let icon = qpath(&format!("/players/{model}/{skin}_i.pcx")).to_string();
    // Draw_FindPic loads a name with a leading '/' as the path without it.
    let icon_ok = exists(&icon[1..]);
    if model_ok && skin_ok && weapon_ok && icon_ok {
        (name.to_string(), icon)
    } else {
        (name.to_string(), BASE_ICON.to_string())
    }
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
                x = input.width / 2 - 160 + strtol(arg(&mut toks));
                y = input.height / 2 - 120 + strtol(arg(&mut toks));
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
                let (name, icon) = client_info(
                    (input.configstrings)(CS_PLAYERSKINS + index),
                    input.file_exists,
                );
                draw_string(&mut out, x + 32, y, &name, 0x80);
                draw_string(&mut out, x + 32, y + 8, "Score: ", 0x80);
                draw_string(&mut out, x + 32 + 7 * 8, y + 8, &score.to_string(), 0x80);
                draw_string(&mut out, x + 32, y + 16, &format!("Ping:  {ping}"), 0);
                draw_string(&mut out, x + 32, y + 24, &format!("Time:  {time}"), 0);
                out.push(HudDraw::Pic { x, y, name: icon });
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
        run_with_files(program, stats, strings, &[])
    }

    fn run_with_files(
        program: &str,
        stats: &[i16; 32],
        strings: &[(usize, &'static str)],
        files: &[&str],
    ) -> Vec<HudDraw> {
        let lookup =
            |i: usize| -> &str { strings.iter().find(|(k, _)| *k == i).map_or("", |(_, v)| v) };
        let exists = |path: &str| files.contains(&path);
        execute_layout(
            program,
            &LayoutInput {
                width: W,
                height: H,
                stats,
                configstrings: &lookup,
                file_exists: &exists,
                serverframe: 0,
            },
        )
    }

    /// The files a complete player loads: model, skin, weapon model and
    /// icon. C ref: CL_LoadClientinfo.
    fn player_files(model: &str, skin: &str) -> Vec<String> {
        vec![
            format!("players/{model}/tris.md2"),
            format!("players/{model}/{skin}.pcx"),
            format!("players/{model}/weapon.md2"),
            format!("players/{model}/{skin}_i.pcx"),
        ]
    }

    fn strings(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    fn icon_with(s: &str, files: &[String]) -> String {
        client_info(s, &|path: &str| files.iter().any(|f| f == path)).1
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
        // C DrawAltString: the name and the whole score row are in the
        // alternate colour, Ping and Time are not.
        let alt = |row: i32| {
            draws
                .iter()
                .filter_map(|d| match d {
                    HudDraw::Char { y: cy, ch, .. } if *cy == row => Some(ch & 0x80 != 0),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert!(alt(y).iter().all(|&a| a));
        assert!(alt(y + 8).iter().all(|&a| a), "Score: row");
        assert!(alt(y + 16).iter().all(|&a| !a));
        assert!(alt(y + 24).iter().all(|&a| !a));
    }

    #[test]
    fn a_client_block_moves_the_origin_for_the_draws_after_it() {
        // C SCR_ExecuteLayoutString assigns the running x and y.
        let draws = run("client 10 32 1 7 50 3 string hi", &[0; 32], &[]);
        let (x, y) = (W / 2 - 160 + 10, H / 2 - 120 + 32);
        assert_eq!(
            draws[draws.len() - 2..],
            [
                HudDraw::Char { x, y, ch: b'h' },
                HudDraw::Char {
                    x: x + 8,
                    y,
                    ch: b'i'
                }
            ]
        );
    }

    #[test]
    fn a_backslash_also_separates_the_model_from_the_skin() {
        // C CL_LoadClientinfo tries "/" and then "\\".
        let files = player_files("female", "athena");
        let (name, icon) = client_info("Bob\\female\\athena", &|p: &str| {
            files.iter().any(|f| f == p)
        });
        assert_eq!(name, "Bob");
        assert_eq!(icon, "/players/female/athena_i.pcx");
    }

    #[test]
    fn a_player_with_all_its_files_uses_its_own_icon() {
        let files = player_files("female", "athena");
        assert_eq!(
            icon_with("Bob\\female/athena", &files),
            "/players/female/athena_i.pcx"
        );
    }

    #[test]
    fn a_model_without_tris_md2_becomes_male() {
        // C: `if (!ci->model) strcpy(model_name, "male")`, before the skin
        // and icon names are built.
        let files = player_files("male", "athena");
        assert_eq!(
            icon_with("Bob\\x/athena", &files),
            "/players/male/athena_i.pcx"
        );
    }

    #[test]
    fn a_skin_missing_for_a_non_male_model_switches_the_model_to_male() {
        // C: "see if the male has it (this is for CTF's skins)".
        let mut files = player_files("male", "ctf_r");
        files.push("players/female/tris.md2".into());
        files.push("players/female/weapon.md2".into());
        files.push("players/female/ctf_r_i.pcx".into());
        assert_eq!(
            icon_with("Bob\\female/ctf_r", &files),
            "/players/male/ctf_r_i.pcx"
        );
    }

    #[test]
    fn a_skin_missing_for_male_falls_back_to_grunt_but_keeps_the_skin_icon_name() {
        // C loads players/male/grunt.pcx as the skin, and builds the icon
        // name from the original skin_name.
        let files = strings(&[
            "players/male/tris.md2",
            "players/male/grunt.pcx",
            "players/male/weapon.md2",
            "players/male/foo_i.pcx",
        ]);
        assert_eq!(
            icon_with("Bob\\male/foo", &files),
            "/players/male/foo_i.pcx"
        );
    }

    #[test]
    fn the_male_check_ignores_case_like_q_stricmp() {
        // A "MALE" model with no such skin does not switch to "male".
        let files = strings(&[
            "players/MALE/tris.md2",
            "players/MALE/grunt.pcx",
            "players/MALE/weapon.md2",
            "players/MALE/foo_i.pcx",
        ]);
        assert_eq!(
            icon_with("Bob\\MALE/foo", &files),
            "/players/MALE/foo_i.pcx"
        );
    }

    #[test]
    fn a_player_with_no_skin_and_no_grunt_skin_uses_the_base_icon() {
        let files = strings(&[
            "players/male/tris.md2",
            "players/male/weapon.md2",
            "players/male/foo_i.pcx",
        ]);
        assert_eq!(icon_with("Bob\\male/foo", &files), BASE_ICON);
    }

    #[test]
    fn a_player_with_no_model_at_all_uses_the_base_icon() {
        let mut files = player_files("female", "athena");
        files.retain(|f| f != "players/female/tris.md2");
        assert_eq!(icon_with("Bob\\female/athena", &files), BASE_ICON);
    }

    #[test]
    fn a_player_with_no_icon_uses_the_base_icon() {
        let mut files = player_files("female", "athena");
        files.retain(|f| f != "players/female/athena_i.pcx");
        assert_eq!(icon_with("Bob\\female/athena", &files), BASE_ICON);
    }

    #[test]
    fn a_player_with_no_weapon_model_uses_the_base_icon() {
        let mut files = player_files("female", "athena");
        files.retain(|f| f != "players/female/weapon.md2");
        assert_eq!(icon_with("Bob\\female/athena", &files), BASE_ICON);
    }

    #[test]
    fn a_cyborg_without_a_weapon_model_borrows_the_male_one() {
        let mut files = player_files("cyborg", "oni911");
        files.retain(|f| f != "players/cyborg/weapon.md2");
        files.push("players/male/weapon.md2".into());
        assert_eq!(
            icon_with("Bob\\cyborg/oni911", &files),
            "/players/cyborg/oni911_i.pcx"
        );
    }

    #[test]
    fn a_configstring_without_a_backslash_parses_the_whole_string_as_model_and_skin() {
        // C keeps `s` at the start: no separator leaves model_name "" and
        // skin_name `s + 1`, so the model becomes male and the skin "ob".
        let files = strings(&[
            "players/male/tris.md2",
            "players/male/grunt.pcx",
            "players/male/weapon.md2",
            "players/male/ob_i.pcx",
        ]);
        let (name, icon) = client_info("Bob", &|p: &str| files.iter().any(|f| f == p));
        assert_eq!(name, "Bob");
        assert_eq!(icon, "/players/male/ob_i.pcx");
    }

    #[test]
    fn file_names_are_cut_at_max_qpath_minus_one_chars_like_com_sprintf() {
        let skin = "s".repeat(59);
        let cut = |path: String| path.chars().take(MAX_QPATH - 1).collect::<String>();
        let icon = cut(format!("/players/m/{skin}_i.pcx"));
        let files = vec![
            "players/m/tris.md2".to_string(),
            cut(format!("players/m/{skin}.pcx")),
            "players/m/weapon.md2".to_string(),
            icon[1..].to_string(),
        ];
        assert_eq!(icon_with(&format!("B\\m/{skin}"), &files), icon);
    }

    #[test]
    fn a_client_block_draws_the_base_icon_for_a_player_whose_files_are_missing() {
        // C SCR_ExecuteLayoutString: `if (!ci->icon) ci = &cl.baseclientinfo`.
        let draws = run(
            "client 0 0 1 0 0 0",
            &[0; 32],
            &[(CS_PLAYERSKINS + 1, "Bob\\female/athena")],
        );
        assert!(draws.contains(&pic(W / 2 - 160, H / 2 - 120, BASE_ICON)));
    }

    #[test]
    fn a_client_block_draws_the_icon_of_a_player_whose_files_exist() {
        let files = player_files("female", "athena");
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let draws = run_with_files(
            "client 0 0 1 0 0 0",
            &[0; 32],
            &[(CS_PLAYERSKINS + 1, "Bob\\female/athena")],
            &files,
        );
        assert!(draws.contains(&pic(
            W / 2 - 160,
            H / 2 - 120,
            "/players/female/athena_i.pcx"
        )));
    }

    #[test]
    fn a_player_configstring_is_cut_at_max_qpath_minus_one_chars() {
        // C copies it into `ci->cinfo[MAX_QPATH]` before splitting it, so
        // a long name loses its own tail and the model and skin.
        let (name, icon) = client_info(
            &format!("{}\\female/athena", "\u{e1}".repeat(70)),
            &|_: &str| false,
        );
        assert_eq!(name, "\u{e1}".repeat(63));
        assert_eq!(icon, "/players/male/grunt_i.pcx");
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
    fn a_high_byte_draws_as_one_glyph_of_its_value() {
        // A layout or configstring as NetMsg::read_string returns it: byte
        // 0xE1 is the char U+00E1. C draws, and centres, the one byte. It
        // is quoted, as an unquoted high byte is whitespace to COM_Parse.
        let stats = [0; 32];
        assert_eq!(
            run("string \"\u{e1}\"", &stats, &[]),
            [HudDraw::Char {
                x: 0,
                y: 0,
                ch: 0xE1
            }]
        );
        assert_eq!(
            run("cstring \"\u{e1}\"", &stats, &[]),
            [HudDraw::Char {
                x: (320 - 8) / 2,
                y: 0,
                ch: 0xE1
            }]
        );
    }

    #[test]
    fn unquoted_high_bytes_separate_tokens_like_com_parse() {
        // C COM_Parse compares a signed char with ' ', so bytes 0x80-0xFF
        // (U+0080-U+00FF from read_string) are whitespace outside quotes.
        assert_eq!(tokens("a\u{a0}b\u{ff}c"), ["a", "b", "c"]);
        assert_eq!(tokens("\u{80}\"\u{e1}\"\u{80}"), ["\u{e1}"]);
        assert_eq!(tokens("a\u{2003}b"), ["a\u{2003}b"]);
    }

    #[test]
    fn a_token_reaching_max_token_chars_is_blanked_like_com_parse() {
        // C COM_Parse: `if (len == MAX_TOKEN_CHARS) len = 0;`.
        let long = "a".repeat(1024);
        assert!(run(&format!("string {long}"), &[0; 32], &[]).is_empty());
        assert_eq!(
            run(&format!("string {}", &long[1..]), &[0; 32], &[]).len(),
            1023
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
