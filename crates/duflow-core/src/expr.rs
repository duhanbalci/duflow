//! Guard ifadesi tokenizer'ı. İfade yorumlanmaz; yalnız içindeki değişken adları çıkarılır.
//! Kabul edilen: tanımlayıcılar (`a.b_c`), sayı, `"string"`, `== != < <= > >= && || ! ( )`, `true false null`.

pub fn idents(expr: &str) -> Vec<String> {
    let mut out = vec![];
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            // string literal atla
            i += 1;
            while i < chars.len() && chars[i] != c {
                i += 1;
            }
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.' || chars[i] == ':') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if !matches!(word.as_str(), "true" | "false" | "null" | "and" | "or" | "not") {
                out.push(word);
            }
            continue;
        }
        if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn extracts_idents() {
        assert_eq!(super::idents(r#"deploy.attempts >= 3 && role == "org_admin" || !service.frozen"#), vec!["deploy.attempts", "role", "service.frozen"]);
        assert_eq!(super::idents("true"), Vec::<String>::new());
    }
}
