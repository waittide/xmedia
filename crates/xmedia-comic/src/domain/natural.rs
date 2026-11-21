use std::cmp::Ordering;

pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ia = a.chars().peekable();
    let mut ib = b.chars().peekable();

    loop {
        match (ia.peek(), ib.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&ca), Some(&cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let na = take_number_tokens(&mut ia);
                    let nb = take_number_tokens(&mut ib);
                    match cmp_number_tokens(&na, &nb) {
                        Ordering::Equal => continue,
                        non_eq => return non_eq,
                    }
                } else {
                    let la = ca.to_lowercase().next().unwrap_or(ca);
                    let lb = cb.to_lowercase().next().unwrap_or(cb);
                    ia.next();
                    ib.next();
                    match la.cmp(&lb) {
                        Ordering::Equal => continue,
                        non_eq => return non_eq,
                    }
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct NumberToken {
    int_part: u128,
    frac_part: String,
    raw_len: usize,
}

fn take_number_tokens<I>(iter: &mut std::iter::Peekable<I>) -> NumberToken
where
    I: Iterator<Item = char> + Clone,
{
    let mut raw_int = String::new();
    while let Some(&c) = iter.peek() {
        if c.is_ascii_digit() {
            raw_int.push(c);
            iter.next();
        } else {
            break;
        }
    }

    let mut frac_part = String::new();
    let mut clone = iter.clone();
    if clone.next() == Some('.') && clone.peek().is_some_and(|c| c.is_ascii_digit()) {
        iter.next();
        while let Some(&c) = iter.peek() {
            if c.is_ascii_digit() {
                frac_part.push(c);
                iter.next();
            } else {
                break;
            }
        }
    }

    let int_part = raw_int.parse::<u128>().unwrap_or(u128::MAX);
    NumberToken {
        int_part,
        frac_part,
        raw_len: raw_int.len(),
    }
}

fn cmp_number_tokens(a: &NumberToken, b: &NumberToken) -> Ordering {
    match a.int_part.cmp(&b.int_part) {
        Ordering::Equal => {}
        non_eq => return non_eq,
    }
    let fa = a.frac_part.trim_end_matches('0');
    let fb = b.frac_part.trim_end_matches('0');
    match fa.cmp(fb) {
        Ordering::Equal => a.raw_len.cmp(&b.raw_len),
        non_eq => non_eq,
    }
}

fn natural_title_sort_key(title: &str) -> String {
    let mut out = String::with_capacity(title.len() + 16);
    let mut chars = title.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut digits = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    digits.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            let trimmed = digits.trim_start_matches('0');
            let normalized = if trimmed.is_empty() { "0" } else { trimmed };
            if normalized.len() < 18 {
                for _ in 0..(18 - normalized.len()) {
                    out.push('0');
                }
            }
            out.push_str(normalized);
        } else {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            chars.next();
        }
    }
    out
}

pub fn build_chapter_sort_key(
    kind_rank: u8,
    volume: Option<f64>,
    number: f64,
    title: &str,
) -> String {
    let vol = volume.filter(|v| v.is_finite()).unwrap_or(0.0).max(0.0);
    let vol_scaled = (vol * 1000.0).round().min(9_999_999_999_999_999.0) as u64;
    let num = if number.is_finite() {
        number.max(0.0)
    } else {
        0.0
    };
    let num_scaled = (num * 1000.0).round().min(9_999_999_999_999_999.0) as u64;
    format!(
        "{kind_rank:02}:{vol_scaled:016}:{num_scaled:016}:{}",
        natural_title_sort_key(title)
    )
}
