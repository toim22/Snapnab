//! The source formatting of a copied text (src/editor/Clip.cpp): a text object has a single style, so "keeping the
//! formatting" takes the font, size, color and bold / italic / underline of the first run of the copied text, from
//! "Rich Text Format" (Word, WordPad) or "HTML Format" (browsers, VS Code).

use std::collections::BTreeMap;

use snapcore::doc::Style;

fn trim(s: &str) -> &str {
    s.trim_matches(|c| " \t\r\n".contains(c))
}

// ---------------------------------------------------------------- RTF
#[derive(Clone, Copy, PartialEq)]
enum Dest {
    Normal,
    FontTable,
    ColorTable,
    Skip,
}

#[derive(Clone, Copy)]
struct State {
    dest: Dest,
    b: bool,
    i: bool,
    ul: bool,
    fs: i32,
    f: i32,
    cf: i32,
}

const SKIPPED: [&str; 16] = ["stylesheet", "info", "pict", "header", "footer", "listtable", "listoverridetable", "rsidtbl", "generator",
                             "themedata", "colorschememapping", "latentstyles", "datastore", "xmlnstbl", "mmathPr", "fldinst"];

pub fn parse_rtf(rtf: &[u8], out: &mut Style) -> bool {
    if !rtf.starts_with(b"{\\rtf") {
        return false;
    }
    let mut stack = vec![State { dest: Dest::Normal, b: false, i: false, ul: false, fs: 24, f: 0, cf: 0 }];
    let mut fonts: BTreeMap<i32, Vec<u8>> = BTreeMap::new();
    let mut colors: Vec<u32> = Vec::new();
    let (mut cur_font, mut red, mut green, mut blue, mut deff) = (0, -1, -1, -1, 0);
    let mut font_name: Vec<u8> = Vec::new();
    let mut group_start = false;
    let mut i = 0;
    let text = |st: &State, fonts: &BTreeMap<i32, Vec<u8>>, colors: &[u32], deff: i32, out: &mut Style| {
        if let Some(name) = fonts.get(&st.f).or_else(|| fonts.get(&deff)) {
            let n = String::from_utf8_lossy(name);
            let n = trim(&n);
            if !n.is_empty() {
                out.font = n.to_owned();
            }
        }
        out.font_size = (st.fs as f32 / 2.0 * 4.0 / 3.0).clamp(4.0, 400.0);
        if st.cf > 0 && (st.cf as usize) < colors.len() && colors[st.cf as usize] != 0 {
            out.text_color = colors[st.cf as usize];
        }
        out.bold = st.b;
        out.italic = st.i;
        out.underline = st.ul;
    };
    while i < rtf.len() {
        let c = rtf[i];
        let st = *stack.last().unwrap();
        if c == b'{' {
            stack.push(st);
            group_start = true;
            i += 1;
            continue;
        }
        if c == b'}' {
            let n = stack.len();
            if stack[n - 1].dest == Dest::FontTable && n >= 2 && stack[n - 2].dest == Dest::FontTable && !font_name.is_empty() {
                fonts.insert(cur_font, std::mem::take(&mut font_name));
            }
            if stack.len() > 1 {
                stack.pop();
            }
            group_start = false;
            i += 1;
            continue;
        }
        if c == b'\\' {
            i += 1;
            if i >= rtf.len() {
                break;
            }
            let n = rtf[i];
            if n == b'*' {
                stack.last_mut().unwrap().dest = Dest::Skip;
                i += 1;
                continue;
            }
            if n == b'\'' {
                i += 3;
                if st.dest == Dest::Normal {
                    text(&st, &fonts, &colors, deff, out);
                    return true;
                }
                continue;
            }
            if !n.is_ascii_alphabetic() {
                i += 1;
                if st.dest == Dest::FontTable {
                    font_name.push(n);
                } else if st.dest == Dest::Normal && matches!(n, b'\\' | b'{' | b'}' | b'~') {
                    text(&st, &fonts, &colors, deff, out);
                    return true;
                }
                continue;
            }
            let ws = i;
            while i < rtf.len() && rtf[i].is_ascii_alphabetic() {
                i += 1;
            }
            let word = std::str::from_utf8(&rtf[ws..i]).unwrap_or("");
            let (mut has_num, mut num, mut sign) = (false, 0i32, 1);
            if i < rtf.len() && rtf[i] == b'-' {
                sign = -1;
                i += 1;
            }
            while i < rtf.len() && rtf[i].is_ascii_digit() {
                num = num.saturating_mul(10).saturating_add((rtf[i] - b'0') as i32);
                has_num = true;
                i += 1;
            }
            num *= sign;
            if i < rtf.len() && rtf[i] == b' ' {
                i += 1;
            }
            let first = group_start;
            group_start = false;
            let s = stack.last_mut().unwrap();
            if word == "fonttbl" {
                s.dest = Dest::FontTable;
                continue;
            }
            if word == "colortbl" {
                s.dest = Dest::ColorTable;
                continue;
            }
            if first && SKIPPED.contains(&word) {
                s.dest = Dest::Skip;
                continue;
            }
            match s.dest {
                Dest::FontTable => {
                    if word == "f" {
                        cur_font = num;
                    }
                    continue;
                }
                Dest::ColorTable => {
                    match word {
                        "red" => red = num,
                        "green" => green = num,
                        "blue" => blue = num,
                        _ => {}
                    }
                    continue;
                }
                Dest::Skip => continue,
                Dest::Normal => {}
            }
            let on = !has_num || num != 0;
            match word {
                "deff" => deff = num,
                "plain" => {
                    (s.b, s.i, s.ul, s.fs, s.f, s.cf) = (false, false, false, 24, deff, 0);
                }
                "b" => s.b = on,
                "i" => s.i = on,
                "ul" => s.ul = on,
                "ulnone" => s.ul = false,
                "fs" if has_num => s.fs = num,
                "f" if has_num => s.f = num,
                "cf" if has_num => s.cf = num,
                _ => {}
            }
            continue;
        }
        if c == b'\r' || c == b'\n' {
            i += 1;
            continue;
        }
        if st.dest == Dest::FontTable {
            if c == b';' {
                fonts.insert(cur_font, std::mem::take(&mut font_name));
            } else {
                font_name.push(c);
            }
            i += 1;
            continue;
        }
        if st.dest == Dest::ColorTable {
            if c == b';' {
                colors.push(if red < 0 {
                    0
                } else {
                    0xFF00_0000 | (red.clamp(0, 255) as u32) << 16 | (green.clamp(0, 255) as u32) << 8 | blue.clamp(0, 255) as u32
                });
                (red, green, blue) = (-1, -1, -1);
            }
            i += 1;
            continue;
        }
        if st.dest != Dest::Normal || c == b' ' {
            i += 1;
            continue;
        }
        text(&st, &fonts, &colors, deff, out);
        return true;
    }
    false
}

// ---------------------------------------------------------------- HTML
fn parse_css_color(v: &str) -> Option<u32> {
    let v = trim(v).to_lowercase();
    if let Some(h) = v.strip_prefix('#') {
        let h: String = if h.len() == 3 { h.chars().flat_map(|c| [c, c]).collect() } else { h.to_owned() };
        if h.len() < 6 {
            return None;
        }
        return u32::from_str_radix(&h[..6], 16).ok().map(|c| 0xFF00_0000 | c);
    }
    if v.starts_with("rgb") {
        let p = v.find('(')?;
        let nums: Vec<i32> = v[p + 1..].trim_end_matches(')').split(',').filter_map(|x| trim(x).parse().ok()).collect();
        if nums.len() < 3 {
            return None;
        }
        return Some(0xFF00_0000 | (nums[0].clamp(0, 255) as u32) << 16 | (nums[1].clamp(0, 255) as u32) << 8 | nums[2].clamp(0, 255) as u32);
    }
    Some(match v.as_str() {
        "black" => 0xFF000000,
        "white" => 0xFFFFFFFF,
        "red" => 0xFFFF0000,
        "green" => 0xFF008000,
        "blue" => 0xFF0000FF,
        "gray" | "grey" => 0xFF808080,
        "orange" => 0xFFFFA500,
        "purple" => 0xFF800080,
        "yellow" => 0xFFFFFF00,
        _ => return None,
    })
}

fn atof(s: &str) -> f32 {
    let n: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    n.parse().unwrap_or(0.0)
}

fn apply_css(css: &str, s: &mut Style, any: &mut bool) {
    for decl in css.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let (k, v) = (trim(k).to_lowercase(), trim(v));
        let lv = v.to_lowercase();
        match k.as_str() {
            "font-family" => {
                let fam: String = v.split(',').next().unwrap_or("").chars().filter(|c| *c != '\'' && *c != '"').collect();
                let fam = trim(&fam);
                let generic = ["monospace", "serif", "sans-serif", "cursive", "fantasy", "system-ui"];
                if !fam.is_empty() && !generic.contains(&fam.to_lowercase().as_str()) {
                    s.font = fam.to_owned();
                    *any = true;
                }
            }
            "font-size" => {
                let n = atof(v);
                if n > 0.0 {
                    s.font_size = (if lv.contains("pt") { n * 4.0 / 3.0 } else { n }).clamp(4.0, 400.0);
                    *any = true;
                }
            }
            "color" => {
                if let Some(c) = parse_css_color(v) {
                    s.text_color = c;
                    *any = true;
                }
            }
            "font-weight" => {
                s.bold = lv == "bold" || lv == "bolder" || atof(&lv) >= 600.0;
                *any = true;
            }
            "font-style" => {
                s.italic = lv.contains("italic");
                *any = true;
            }
            "text-decoration" | "text-decoration-line" => {
                s.underline = lv.contains("underline");
                *any = true;
            }
            _ => {}
        }
    }
}

fn attr(tag: &str, name: &str) -> String {
    let lt = tag.to_lowercase();
    let key = format!("{name}=");
    let mut p = 0;
    while let Some(f) = lt[p..].find(&key) {
        let at = p + f;
        if at > 0 && !lt.as_bytes()[at - 1].is_ascii_whitespace() {
            p = at + 1;
            continue;
        }
        let v = at + key.len();
        let b = tag.as_bytes();
        if v >= b.len() {
            return String::new();
        }
        let q = b[v];
        if q == b'"' || q == b'\'' {
            let e = tag[v + 1..].find(q as char).map_or(tag.len(), |e| v + 1 + e);
            return tag[v + 1..e].to_owned();
        }
        let mut e = v;
        while e < b.len() && !b[e].is_ascii_whitespace() && b[e] != b'>' {
            e += 1;
        }
        return tag[v..e].to_owned();
    }
    String::new()
}

pub fn parse_html(html: &str, out: &mut Style) -> bool {
    let frag = match html.find("<!--StartFragment-->") {
        Some(a) => {
            let s = &html[a + 20..];
            match s.find("<!--EndFragment-->") {
                Some(b) => &s[..b],
                None => s,
            }
        }
        None => html,
    };
    // the open tags up to the first visible text; the innermost ones win
    let mut open: Vec<&str> = Vec::new();
    let b = frag.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' {
            let Some(e) = frag[i..].find('>').map(|e| i + e) else { break };
            let tag = &frag[i + 1..e];
            i = e + 1;
            if tag.is_empty() || tag.starts_with('!') || tag.starts_with('?') {
                continue;
            }
            if tag.starts_with('/') {
                open.pop();
                continue;
            }
            let name = tag.split(|c: char| " \t\r\n/".contains(c)).next().unwrap_or("").to_lowercase();
            if matches!(name.as_str(), "br" | "img" | "meta" | "hr") || tag.ends_with('/') {
                continue;
            }
            if name == "style" || name == "script" {
                i = frag[i..].to_lowercase().find(&format!("</{name}")).map_or(frag.len(), |c| i + c);
                continue;
            }
            open.push(tag);
            continue;
        }
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let mut s = out.clone();
        let mut any = false;
        for tag in &open {
            let name = tag.split(|c: char| " \t\r\n".contains(c)).next().unwrap_or("").to_lowercase();
            match name.as_str() {
                "b" | "strong" => {
                    s.bold = true;
                    any = true;
                }
                "i" | "em" => {
                    s.italic = true;
                    any = true;
                }
                "u" => {
                    s.underline = true;
                    any = true;
                }
                "font" => {
                    let (face, col) = (attr(tag, "face"), attr(tag, "color"));
                    if !face.is_empty() {
                        s.font = trim(face.split(',').next().unwrap_or("")).to_owned();
                        any = true;
                    }
                    if let Some(c) = parse_css_color(&col) {
                        s.text_color = c;
                        any = true;
                    }
                }
                _ => {}
            }
            let css = attr(tag, "style");
            if !css.is_empty() {
                apply_css(&css, &mut s, &mut any);
            }
        }
        if any {
            *out = s;
        }
        return any;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtf_takes_the_first_run() {
        let mut s = Style::default();
        let rtf = br"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}{\f1 Georgia;}}{\colortbl;\red255\green0\blue0;}\f1\fs32\cf1\b Hello}";
        assert!(parse_rtf(rtf, &mut s));
        assert_eq!(s.font, "Georgia");
        assert!(s.bold && (s.font_size - 21.333).abs() < 0.01);
        assert_eq!(s.text_color, 0xFFFF0000);
    }

    #[test]
    fn html_takes_the_innermost_style() {
        let mut s = Style::default();
        let html = "<html><body><!--StartFragment--><span style=\"font-family:'Consolas', monospace; font-size: 12pt; color:#00f\"><b>x</b></span><!--EndFragment--></body></html>";
        assert!(parse_html(html, &mut s));
        assert_eq!(s.font, "Consolas");
        assert!(s.bold && (s.font_size - 16.0).abs() < 0.01);
        assert_eq!(s.text_color, 0xFF0000FF);
    }
}
