//! Minimal XML (the role of libxml2 / MSXML in the original): presets, the .snap index, document objects, Names.xml.
//! An exact port of the reader and writer in Xml.cpp, so the files Snapnab writes keep the same bytes:
//!
//! - written with a two-space indent, `<a/>` for empty elements and attributes in double quotes; `& < > " '`, line
//!   breaks and tabs are written as entities;
//! - read leniently: comments, `<?…?>` and `<!…>` are skipped, CDATA is taken as text, text made only of white space is
//!   dropped, and a closing tag ends the element whatever its name. On malformed input the reader stops and reports
//!   a failure, but keeps what it had read (the C++ callers see the same partial tree).
//!
//! Text is UTF-8: what Snapnab writes always is; invalid bytes in a foreign file become U+FFFD.

use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    pub text: String,
}

impl Node {
    pub fn new(name: impl Into<String>) -> Self {
        Node { name: name.into(), ..Default::default() }
    }

    /// The value of the first attribute named `k`.
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    /// Replaces the value of attribute `k`, or adds it at the end.
    pub fn set(&mut self, k: &str, v: impl Into<String>) -> &mut Self {
        let v = v.into();
        match self.attrs.iter_mut().find(|(n, _)| n == k) {
            Some(a) => a.1 = v,
            None => self.attrs.push((k.to_owned(), v)),
        }
        self
    }

    /// Adds a child element and returns it.
    pub fn add(&mut self, name: impl Into<String>) -> &mut Node {
        self.children.push(Node::new(name));
        self.children.last_mut().unwrap()
    }

    /// The first child element named `n`.
    pub fn child(&self, n: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == n)
    }

    pub fn child_mut(&mut self, n: &str) -> Option<&mut Node> {
        self.children.iter_mut().find(|c| c.name == n)
    }
}

// ---------------------------------------------------------------- writing

fn escape(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(c),
        }
    }
}

fn write(out: &mut String, n: &Node, depth: usize) {
    out.extend(std::iter::repeat_n(' ', depth * 2));
    out.push('<');
    out.push_str(&n.name);
    for (k, v) in &n.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        escape(out, v);
        out.push('"');
    }
    if n.children.is_empty() && n.text.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push('>');
    escape(out, &n.text);
    if !n.children.is_empty() {
        out.push('\n');
        for c in &n.children {
            write(out, c, depth + 1);
        }
        out.extend(std::iter::repeat_n(' ', depth * 2));
    }
    out.push_str("</");
    out.push_str(&n.name);
    out.push_str(">\n");
}

/// The document as text, with `<?xml version="1.0" encoding="UTF-8"?>` first if `declaration`.
pub fn serialize(root: &Node, declaration: bool) -> String {
    let mut out = String::new();
    if declaration {
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    }
    write(&mut out, root, 0);
    out
}

/// Writes the document (with the declaration); false if the file cannot be written.
pub fn save_file(path: &Path, root: &Node) -> bool {
    std::fs::write(path, serialize(root, true)).is_ok()
}

// ---------------------------------------------------------------- reading

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// strtoul over a nul-terminated string (MSVC): leading white space, a sign, for base 16 an optional 0x, then digits;
/// ULONG_MAX (32 bits) on overflow, the negated value for '-'.
fn strtoul(s: &[u8], base: u32) -> u32 {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
        i += 1;
    }
    let neg = i < s.len() && s[i] == b'-';
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }
    if base == 16 && s.len() >= i + 3 && s[i] == b'0' && (s[i + 1] | 0x20) == b'x' && (s[i + 2] as char).is_ascii_hexdigit()
    {
        i += 2;
    }
    let (mut v, mut overflow, mut any) = (0u32, false, false);
    while let Some(d) = s.get(i).and_then(|&c| (c as char).to_digit(base)) {
        any = true;
        match v.checked_mul(base).and_then(|x| x.checked_add(d)) {
            Some(x) => v = x,
            None => overflow = true,
        }
        i += 1;
    }
    if !any {
        0
    } else if overflow {
        u32::MAX
    } else if neg {
        v.wrapping_neg()
    } else {
        v
    }
}

/// Decodes the five named entities and &#N; / &#xN; (anything else after '&' is kept as it is).
fn unescape(v: &[u8]) -> Vec<u8> {
    let mut r = Vec::with_capacity(v.len());
    let mut k = 0;
    while k < v.len() {
        if v[k] != b'&' {
            r.push(v[k]);
            k += 1;
            continue;
        }
        let Some(off) = v[k..].iter().position(|&c| c == b';') else {
            r.push(b'&');
            k += 1;
            continue;
        };
        let e = k + off;
        let ent = &v[k + 1..e];
        match ent {
            b"amp" => r.push(b'&'),
            b"lt" => r.push(b'<'),
            b"gt" => r.push(b'>'),
            b"quot" => r.push(b'"'),
            b"apos" => r.push(b'\''),
            [b'#', ..] => {
                // the code point goes through UTF-16 as in the C++ (wchar_t), so out-of-range values give the same text
                let cp = if ent.len() > 1 && (ent[1] | 0x20) == b'x' { strtoul(&ent[2..], 16) } else { strtoul(&ent[1..], 10) };
                let units: Vec<u16> = if cp < 0x10000 {
                    vec![cp as u16]
                } else {
                    let c = cp - 0x10000;
                    vec![0xD800u32.wrapping_add(c >> 10) as u16, (0xDC00 + (c & 0x3FF)) as u16]
                };
                r.extend_from_slice(String::from_utf16_lossy(&units).as_bytes());
            }
            _ => {
                r.push(b'&');
                r.extend_from_slice(ent);
                r.push(b';');
            }
        }
        k = e + 1;
    }
    r
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn eof(&self) -> bool {
        self.i >= self.s.len()
    }

    fn cur(&self) -> u8 {
        self.s[self.i]
    }

    fn ws(&mut self) {
        while !self.eof() && self.cur() <= b' ' {
            self.i += 1;
        }
    }

    fn starts(&self, p: &[u8]) -> bool {
        self.s.get(self.i..).is_some_and(|r| r.starts_with(p))
    }

    fn find(&self, pat: &[u8], from: usize) -> Option<usize> {
        let rest = self.s.get(from..)?;
        rest.windows(pat.len()).position(|w| w == pat).map(|p| from + p)
    }

    /// Moves past `pat` if found, else to the end.
    fn skip_past(&mut self, pat: &[u8]) {
        self.i = self.find(pat, self.i).map_or(self.s.len(), |e| e + pat.len());
    }

    fn skip_misc(&mut self) {
        loop {
            self.ws();
            if self.starts(b"<?") {
                self.skip_past(b"?>");
            } else if self.starts(b"<!--") {
                self.skip_past(b"-->");
            } else if self.starts(b"<!") {
                self.skip_past(b">");
            } else {
                break;
            }
        }
    }

    fn name(&mut self) -> String {
        let b = self.i;
        while !self.eof() && !b" \t\r\n/>=\0".contains(&self.cur()) {
            self.i += 1;
        }
        lossy(&self.s[b..self.i])
    }

    fn element(&mut self, n: &mut Node) -> bool {
        self.skip_misc();
        if self.eof() || self.cur() != b'<' {
            return false;
        }
        self.i += 1;
        n.name = self.name();
        loop {
            self.ws();
            if self.eof() {
                return false;
            }
            if self.cur() == b'/' {
                self.i += 2; // "/>" (the '>' is not checked, as in the C++)
                return true;
            }
            if self.cur() == b'>' {
                self.i += 1;
                break;
            }
            let k = self.name();
            self.ws();
            if self.eof() || self.cur() != b'=' {
                return false;
            }
            self.i += 1;
            self.ws();
            if self.eof() {
                return false;
            }
            let q = self.cur(); // any character opens the value and the next one like it closes it
            self.i += 1;
            let Some(e) = self.find(&[q], self.i) else { return false };
            n.attrs.push((k, lossy(&unescape(&self.s[self.i..e]))));
            self.i = e + 1;
        }
        loop {
            let b = self.i;
            if b > self.s.len() {
                return false; // after "<x/" at the very end (the C++ throws here)
            }
            while !self.eof() && self.cur() != b'<' {
                self.i += 1;
            }
            let t = &self.s[b..self.i];
            if t.iter().any(|c| !b" \t\r\n".contains(c)) {
                n.text.push_str(&lossy(&unescape(t)));
            }
            if self.eof() {
                return false;
            }
            if self.starts(b"</") {
                self.skip_past(b">");
                return true;
            }
            if self.starts(b"<!--") || self.starts(b"<?") {
                self.skip_misc();
                continue;
            }
            if self.starts(b"<![CDATA[") {
                let Some(e) = self.find(b"]]>", self.i) else { return false };
                n.text.push_str(&lossy(&self.s[self.i + 9..e]));
                self.i = e + 3;
                continue;
            }
            let mut c = Node::default();
            if !self.element(&mut c) {
                return false;
            }
            n.children.push(c);
        }
    }
}

/// Reads the root element; the bool is false on malformed input, with the tree read up to that point.
pub fn parse_partial(text: &[u8]) -> (Node, bool) {
    let mut p = Parser { s: text, i: 0 };
    if text.len() >= 3 && text[0] == 0xEF {
        p.i = 3; // UTF-8 BOM
    }
    let mut root = Node::default();
    let ok = p.element(&mut root);
    (root, ok)
}

pub fn parse(text: &[u8]) -> Option<Node> {
    match parse_partial(text) {
        (n, true) => Some(n),
        _ => None,
    }
}

pub fn load_file(path: &Path) -> Option<Node> {
    parse(&std::fs::read(path).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Node {
        let mut root = Node::new("Index");
        root.set("Version", "1").set("Note", "a & b <c> \"d\" 'e'\r\n\tf");
        let list = root.add("FrameList");
        list.add("Frame").set("Name", "Frame0");
        list.add("Frame").set("Name", "Frame1");
        root.add("Title").text = "Região ç → 中文 😀".into();
        root.add("Empty");
        root
    }

    #[test]
    fn writes_the_same_bytes_as_the_cpp() {
        let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
            <Index Version=\"1\" Note=\"a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos;&#13;&#10;&#9;f\">\n\
            \x20 <FrameList>\n\
            \x20   <Frame Name=\"Frame0\"/>\n\
            \x20   <Frame Name=\"Frame1\"/>\n\
            \x20 </FrameList>\n\
            \x20 <Title>Região ç → 中文 😀</Title>\n\
            \x20 <Empty/>\n\
            </Index>\n";
        assert_eq!(serialize(&sample(), true), expected);
        assert_eq!(serialize(&Node::new("a"), false), "<a/>\n");
    }

    #[test]
    fn round_trip() {
        let s = sample();
        assert_eq!(parse(serialize(&s, true).as_bytes()), Some(s.clone()));
        assert_eq!(parse(serialize(&s, false).as_bytes()), Some(s));
    }

    #[test]
    fn lenient_reading() {
        let t = b"\xEF\xBB\xBF<?xml version='1.0'?>\n<!DOCTYPE x>\n<!-- c -->\n<a k='v' j=\"&#65;&#x42;&#x1F600;&bogus;&\">\n \
                  <b/><!-- in --><?pi?>text &lt;1&gt;<![CDATA[<raw & ;>]]><c></wrong>  \n</a>";
        let n = parse(t).unwrap();
        assert_eq!(n.name, "a");
        assert_eq!(n.attrs, [("k".into(), "v".into()), ("j".into(), "AB😀&bogus;&".into())]);
        assert_eq!(n.children.len(), 2);
        assert_eq!((n.children[0].name.as_str(), n.children[1].name.as_str()), ("b", "c"));
        // text chunks with something other than white space are kept whole, CDATA as it is
        assert_eq!(n.text, "text <1><raw & ;>");
    }

    #[test]
    fn malformed_input_keeps_the_partial_tree() {
        let (n, ok) = parse_partial(b"<a x='1'><b/><c y='2'>");
        assert!(!ok);
        assert_eq!(n.name, "a");
        assert_eq!(n.children.len(), 1);
        for bad in [&b""[..], b"text", b"<a", b"<a x", b"<a x=", b"<a x='1", b"<a><b/", b"<a><![CDATA[x", b"<a>t"] {
            assert!(parse(bad).is_none(), "{:?}", String::from_utf8_lossy(bad));
        }
        assert!(parse(b"<a/").is_some()); // as in the C++: "/" ends the element without looking at the next character
    }

    #[test]
    fn character_references() {
        assert_eq!(unescape(b"&#0;&#x0x41;&# 66;&#-1;&#xD800;&#99999999999;"), {
            let mut v = vec![0u8, b'A', b'B'];
            // -1 wraps to 0xFFFFFFFF; the pair computed from it is U+D7BF (the high unit wraps) and a lone low surrogate
            v.extend("\u{D7BF}\u{FFFD}".as_bytes());
            v.extend("\u{FFFD}".as_bytes()); // a lone surrogate
            v.extend("\u{D7BF}\u{FFFD}".as_bytes()); // overflow: ULONG_MAX, the same value
            v
        });
    }

    #[test]
    fn node_helpers() {
        let mut n = Node::new("x");
        n.set("a", "1").set("b", "2").set("a", "3");
        assert_eq!(n.attrs, [("a".into(), "3".into()), ("b".into(), "2".into())]);
        assert_eq!(n.attr("b"), Some("2"));
        assert_eq!(n.attr("c"), None);
        n.add("k").add("deep");
        assert!(n.child("k").and_then(|k| k.child("deep")).is_some());
        n.child_mut("k").unwrap().text = "t".into();
        assert_eq!(n.child("k").unwrap().text, "t");
    }

    #[test]
    fn files() {
        let p = std::env::temp_dir().join(format!("snapcore-xml-{}.xml", std::process::id()));
        let s = sample();
        assert!(save_file(&p, &s));
        assert_eq!(load_file(&p), Some(s));
        let _ = std::fs::remove_file(&p);
        assert_eq!(load_file(&p), None);
    }
}
