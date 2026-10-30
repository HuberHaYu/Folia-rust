// LRC-style time tags such as `[01:23.45]` and `<01:23.456>`.

/// A time tag found in a string; `start..end` is its byte range including the brackets.
pub struct Tag {
    pub time: f64,
    pub start: usize,
    pub end: usize,
}

/// Parses `mm:ss.xx`: two or three digit minutes, two digit seconds, a `.` or `:`, then two or three digits.
pub fn parse_clock(s: &str) -> Option<f64> {
    let (minutes, rest) = s.split_once(':')?;
    let (seconds, fraction) = rest.split_at_checked(2)?;
    let fraction = fraction.strip_prefix(['.', ':'])?;
    let valid = |part: &str, len: std::ops::RangeInclusive<usize>| {
        len.contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit())
    };
    if !valid(minutes, 2..=3) || !valid(seconds, 2..=2) || !valid(fraction, 2..=3) {
        return None;
    }

    let number = |part: &str| part.parse::<f64>().ok();
    let fraction = number(fraction)? / 10f64.powi(fraction.len() as i32);
    Some(number(minutes)? * 60.0 + number(seconds)? + fraction)
}

/// Finds every `open`…`close` tag whose body is a clock, in order.
pub fn scan(s: &str, open: char, close: char) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mut from = 0;
    while let Some(offset) = s[from..].find(open) {
        let start = from + offset;
        let Some(len) = s[start + 1..].find(close) else { break };
        let end = start + 1 + len + 1;
        match parse_clock(&s[start + 1..end - 1]) {
            Some(time) => {
                tags.push(Tag { time, start, end });
                from = end;
            }
            None => from = start + 1,
        }
    }
    tags
}

/// Splits a line that opens with one or more `[mm:ss.xx]` tags into the first tag's time and the text after the tags.
pub fn leading(line: &str) -> Option<(f64, &str)> {
    let mut time = None;
    let mut rest = line;
    while let Some(body) = rest.strip_prefix('[') {
        let Some((clock, after)) = body.split_once(']') else { break };
        let Some(tag_time) = parse_clock(clock) else { break };
        time.get_or_insert(tag_time);
        rest = after;
    }
    Some((time?, rest))
}
