//! temporary docs tool: replay a captured terminal stream and dump the final
//! screen as json, so the readme screenshots can be rendered offline.
use std::io::Read;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: vtdump <capture> [rows] [cols]");
    let rows: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(30);
    let cols: u16 = args.next().and_then(|s| s.parse().ok()).unwrap_or(110);
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let mut p = vt100::Parser::new(rows, cols, 0);
    p.process(&bytes);
    let screen = p.screen();
    let mut out = String::from("{\"rows\":[");
    for r in 0..rows {
        if r > 0 {
            out.push(',');
        }
        out.push('[');
        let mut first = true;
        for c in 0..cols {
            let Some(cell) = screen.cell(r, c) else {
                continue;
            };
            let txt = cell.contents();
            if txt.is_empty() {
                continue;
            }
            let (fg, bg) = (color(cell.fgcolor()), color(cell.bgcolor()));
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str(&format!(
                "{{\"c\":{c},\"t\":{},\"fg\":{fg},\"bg\":{bg},\"b\":{}}}",
                json(&txt),
                cell.bold() as u8
            ));
        }
        out.push(']');
    }
    out.push_str("]}");
    println!("{out}");
}

fn color(c: vt100::Color) -> String {
    match c {
        vt100::Color::Default => "\"d\"".into(),
        vt100::Color::Idx(i) => format!("\"i{i}\""),
        vt100::Color::Rgb(r, g, b) => format!("\"#{r:02x}{g:02x}{b:02x}\""),
    }
}

fn json(s: &str) -> String {
    let mut o = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}
