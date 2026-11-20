/// Escapes XML 1.0 special characters (`&`, `<`, `>`, `"`, `'`).
pub fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
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
