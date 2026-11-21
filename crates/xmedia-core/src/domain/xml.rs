/// Returns whether `ch` is a valid XML 1.0 `Char` (Fifth Edition §2.2).
fn is_valid_xml_1_0_char(ch: char) -> bool {
    let cp = ch as u32;
    matches!(
        cp,
        0x09 | 0x0A | 0x0D | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF
    )
}

/// Escapes XML 1.0 special characters (`&`, `<`, `>`, `"`, `'`) and strips illegal XML 1.0
/// control characters so generated Atom/OPDS/OpenSubsonic XML always remains well-formed.
pub fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if !is_valid_xml_1_0_char(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_handles_all_five_entities() {
        assert_eq!(
            xml_escape(r#"A & B <C> "D" 'E'"#),
            "A &amp; B &lt;C&gt; &quot;D&quot; &apos;E&apos;"
        );
        assert_eq!(xml_escape("plain text 123 中文"), "plain text 123 中文");
    }
}
