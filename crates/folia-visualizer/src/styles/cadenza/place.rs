// Cadenza placement: wrap a line, pick one hero word, then push the others apart.
// Wrapping measures graphemes with `measure` and breaks at spaces. The original uses pretext.

use super::envelopes::{self, FONT_SCALE, WIDTH_RATIO};
use folia_core::{AnimationIntensity, Line};

/// One glyph of a placed fragment, with its left edge and sung interval.
pub struct Glyph {
    pub text: String,
    pub offset: f32,
    pub start: f64,
    pub end: f64,
}

/// A word fragment after collision. Coordinates are pixels from the cloud's origin.
pub struct Placement {
    pub text: String,
    pub word_index: usize,
    pub start: f64,
    pub end: f64,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub passed_rotate: f64,
    pub passed_drift_x: f64,
    pub passed_drift_y: f64,
    pub emphasis: f64,
    pub glyphs: Vec<Glyph>,
    pub interlude: bool,
    pub cjk: bool,
}

struct Range {
    index: usize,
    start: usize,
    end: usize,
    word_start: f64,
    word_end: f64,
    word_text: String,
}

struct Fragment {
    word_index: usize,
    line_index: usize,
    fragment_index: usize,
    text: String,
    word_text: String,
    start_in_word: usize,
    word_len: usize,
    word_start: f64,
    word_end: f64,
    start_x: f64,
    end_x: f64,
    split: bool,
    primary: bool,
}

struct Plan {
    fragment: usize,
    line_index: usize,
    emphasis: f64,
    base_x: f64,
    base_y: f64,
    width: f64,
    height: f64,
    scale: f64,
    collision_width: f64,
    collision_height: f64,
    padding: f64,
    step: f64,
}

struct Spot {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

pub fn font_px(viewport_width: f32, grapheme_count: usize, word_count: usize, lyrics_scale: f32) -> f32 {
    let width_base = (viewport_width * 0.086).clamp(34.0, 94.0);
    let length_penalty = if grapheme_count > 12 { ((grapheme_count - 12) as f32 * 1.8).min(34.0) } else { 0.0 };
    let density = if word_count > 7 { ((word_count - 7) as f32 * 1.5).min(18.0) } else { 0.0 };
    let chosen = (width_base - length_penalty - density).clamp(28.0, 104.0);
    (chosen * FONT_SCALE * lyrics_scale).clamp(24.0, 132.0)
}

pub fn layout(
    line: &Line,
    width: f32,
    height: f32,
    lyrics_scale: f32,
    intensity: AnimationIntensity,
) -> Vec<Placement> {
    let chars = envelopes::graphemes(&line.text);
    if chars.is_empty() || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let px = font_px(width, chars.len(), line.words.len().max(1), lyrics_scale);
    let widths = measure_widths(&chars, px);
    let rows = wrap(&chars, &widths, max_width(width, chars.len()));
    let ranges = word_ranges(line, &chars);
    let fragments = fragments(&chars, &widths, &rows, &ranges);
    scatter(line, &fragments, px, max_width(width, chars.len()), intensity)
}

fn measure_widths(chars: &[String], font_px: f32) -> Vec<f32> {
    chars
        .iter()
        .map(|glyph| {
            let measured = crate::measure(glyph, font_px, crate::BOLD, 10_000.0).width;
            if measured > 0.0 {
                measured
            } else if envelopes::is_cjk_text(glyph) {
                font_px
            } else {
                font_px * 0.5
            }
        })
        .collect()
}

fn max_width(viewport: f32, grapheme_count: usize) -> f32 {
    let available = (viewport - 48.0).max(120.0);
    let min_width = 220.0_f32.min(available);
    let compression =
        if grapheme_count > 12 { (0.92 - (grapheme_count - 12) as f64 * 0.018).clamp(0.62, 0.92) } else { 0.92 };
    ((viewport as f64) * WIDTH_RATIO * compression).min(820.0).clamp(f64::from(min_width), f64::from(available)) as f32
}

fn wrap(chars: &[String], widths: &[f32], max_width: f32) -> Vec<(usize, usize)> {
    let mut rows = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        index = skip_spaces(chars, index);
        if index >= chars.len() {
            break;
        }
        let start = index;
        let (mut end, break_at) = take_row(chars, widths, start, max_width);
        if end < chars.len()
            && let Some(at) = break_at.filter(|&at| at > start)
        {
            end = at;
        }
        while end > start && is_space(&chars[end - 1]) {
            end -= 1;
        }
        if end == start {
            end = (start + 1).min(chars.len());
        }
        rows.push((start, end));
        index = end;
    }
    rows
}

fn take_row(chars: &[String], widths: &[f32], start: usize, max_width: f32) -> (usize, Option<usize>) {
    let mut end = start;
    let mut width = 0.0_f32;
    let mut break_at = None;
    while end < chars.len() {
        if width + widths[end] > max_width && end > start {
            break;
        }
        width += widths[end];
        end += 1;
        if is_space(&chars[end - 1]) {
            break_at = Some(end);
        }
    }
    (end, break_at)
}

fn skip_spaces(chars: &[String], mut index: usize) -> usize {
    while index < chars.len() && is_space(&chars[index]) {
        index += 1;
    }
    index
}

fn is_space(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

fn word_ranges(line: &Line, chars: &[String]) -> Vec<Range> {
    let mut cursor = 0;
    line.words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let target = envelopes::graphemes(&word.text);
            let found = find_sequence(chars, &target, cursor);
            let start = found.unwrap_or(cursor.min(chars.len()));
            let end = (start + target.len()).min(chars.len()).max(start);
            cursor = end;
            Range { index, start, end, word_start: word.start, word_end: word.end, word_text: word.text.clone() }
        })
        .collect()
}

fn find_sequence(chars: &[String], target: &[String], from: usize) -> Option<usize> {
    let last = chars.len().checked_sub(target.len())?;
    if target.is_empty() || from > last {
        return None;
    }
    (from..=last).find(|&index| chars[index..index + target.len()] == *target)
}

fn fragments(chars: &[String], widths: &[f32], rows: &[(usize, usize)], ranges: &[Range]) -> Vec<Fragment> {
    let mut raw = Vec::new();
    let mut per_line = vec![0; rows.len()];
    for (line_index, &(row_start, row_end)) in rows.iter().enumerate() {
        for range in ranges {
            if range.end <= row_start || range.start >= row_end {
                continue;
            }
            let start = range.start.max(row_start);
            let end = range.end.min(row_end);
            if start >= end {
                continue;
            }
            let fragment_index = per_line[line_index];
            per_line[line_index] += 1;
            raw.push(Fragment {
                word_index: range.index,
                line_index,
                fragment_index,
                text: chars[start..end].join(""),
                word_text: range.word_text.clone(),
                start_in_word: start - range.start,
                word_len: (range.end - range.start).max(1),
                word_start: range.word_start,
                word_end: range.word_end,
                start_x: sum_widths(widths, row_start, start),
                end_x: sum_widths(widths, row_start, end),
                split: false,
                primary: false,
            });
        }
    }
    mark_splits(&mut raw);
    raw
}

fn sum_widths(widths: &[f32], from: usize, to: usize) -> f64 {
    widths[from..to].iter().map(|width| f64::from(*width)).sum()
}

fn mark_splits(raw: &mut [Fragment]) {
    let mut counts = Vec::new();
    for fragment in raw.iter() {
        if counts.len() <= fragment.word_index {
            counts.resize(fragment.word_index + 1, 0);
        }
        counts[fragment.word_index] += 1;
    }
    let mut seen = vec![0; counts.len()];
    for fragment in raw.iter_mut() {
        let count = counts.get(fragment.word_index).copied().unwrap_or(1);
        let index = seen.get(fragment.word_index).copied().unwrap_or(0);
        if fragment.word_index < seen.len() {
            seen[fragment.word_index] += 1;
        }
        fragment.split = count > 1;
        fragment.primary = index == 0;
    }
}

fn scatter(
    line: &Line,
    fragments: &[Fragment],
    font_px: f32,
    max_width: f32,
    intensity: AnimationIntensity,
) -> Vec<Placement> {
    let line_height = (f64::from(font_px) * if envelopes::is_cjk_text(&line.text) { 1.22 } else { 1.1 }).round();
    let total_height = (fragments.iter().map(|f| f.line_index).max().unwrap_or(0) + 1) as f64 * line_height;
    let (base_scale, margin) = base_scale(intensity);
    let emphasis = emphasis_of(fragments, line.is_interlude());
    let mut plans = build_plans(fragments, &emphasis, font_px, line_height, total_height, base_scale, margin);
    plans.sort_by(|a, b| {
        b.emphasis
            .total_cmp(&a.emphasis)
            .then(a.line_index.cmp(&b.line_index))
            .then_with(|| fragments[a.fragment].start_x.total_cmp(&fragments[b.fragment].start_x))
    });
    let hero = hero_metrics(&plans);
    let mut occupied = Vec::new();
    let mut placed = Vec::with_capacity(plans.len());
    for plan in &plans {
        let (x, y) = choose_spot(plan, fragments, &hero, max_width, total_height, line_height, &occupied);
        occupied.push(rect(x, y, plan));
        placed.push(finish(line, &fragments[plan.fragment], plan, x, y, font_px, intensity));
    }
    placed
}

fn base_scale(intensity: AnimationIntensity) -> (f64, f64) {
    match intensity {
        AnimationIntensity::Calm => (1.0, 4.0),
        AnimationIntensity::Chaotic => (1.02, 6.0),
        AnimationIntensity::Normal => (1.01, 6.0),
    }
}

fn emphasis_of(fragments: &[Fragment], interlude: bool) -> Vec<f64> {
    let mut emphasis = vec![1.0; fragments.iter().map(|f| f.word_index).max().map_or(0, |i| i + 1)];
    if interlude {
        return emphasis;
    }
    let primary: Vec<_> = fragments.iter().filter(|fragment| fragment.primary).collect();
    let mut best: Option<(usize, f64)> = None;
    for fragment in primary.iter().filter(|fragment| !fragment.split && !fragment.text.trim().is_empty()) {
        let count = fragment.word_len.max(envelopes::graphemes(&fragment.word_text).len()).max(1);
        let semantic = if envelopes::is_cjk_text(&fragment.word_text) { 0.18 } else { (count as f64 * 0.08).min(0.36) };
        let center =
            1.0 - (fragment.word_index as f64 - (primary.len() - 1) as f64 / 2.0).abs() / primary.len().max(1) as f64;
        let score = semantic + center * 0.18;
        if best.is_none_or(|(_, held)| score > held) {
            best = Some((fragment.word_index, score));
        }
    }
    if let Some((index, score)) = best {
        let boost = 1.0 + envelopes::clamp(score - 0.48, 0.0, 0.52);
        if index < emphasis.len() {
            emphasis[index] = 1.46 * boost;
        }
    }
    emphasis
}

fn build_plans(
    fragments: &[Fragment],
    emphasis: &[f64],
    font_px: f32,
    line_height: f64,
    total_height: f64,
    base_scale: f64,
    margin: f64,
) -> Vec<Plan> {
    fragments
        .iter()
        .enumerate()
        .map(|(index, fragment)| {
            let weight = emphasis.get(fragment.word_index).copied().unwrap_or(1.0);
            let width = (fragment.end_x - fragment.start_x).max(f64::from(font_px) * 0.18);
            let scale = if weight > 1.0 { base_scale * weight } else { base_scale };
            let height = f64::from(font_px) * scale * 0.95;
            let line_left = -sum_row_width(fragments, fragment.line_index) / 2.0;
            Plan {
                fragment: index,
                line_index: fragment.line_index,
                emphasis: weight,
                base_x: line_left + fragment.start_x,
                base_y: -total_height / 2.0 + f64::from(font_px) + fragment.line_index as f64 * line_height,
                width,
                height,
                scale,
                collision_width: width * scale * if weight > 1.0 { 1.48 } else { 1.26 },
                collision_height: height * if weight > 1.0 { 1.36 } else { 1.24 },
                padding: margin + if weight > 1.0 { 10.0 } else { 2.0 },
                step: 10.0_f64.max((f64::from(font_px) * 0.14).round()),
            }
        })
        .collect()
}

fn sum_row_width(fragments: &[Fragment], line_index: usize) -> f64 {
    fragments
        .iter()
        .filter(|fragment| fragment.line_index == line_index)
        .map(|fragment| fragment.end_x)
        .fold(0.0, f64::max)
}

struct Hero {
    center_x: f64,
    center_y: f64,
    width: f64,
}

fn hero_metrics(plans: &[Plan]) -> Option<Hero> {
    let hero = plans.iter().max_by(|a, b| a.emphasis.total_cmp(&b.emphasis)).filter(|plan| plan.emphasis > 1.0)?;
    Some(Hero {
        center_x: hero.width * hero.scale / 2.0,
        center_y: -hero.height * 0.46,
        width: hero.width * hero.scale,
    })
}

fn choose_spot(
    plan: &Plan,
    fragments: &[Fragment],
    hero: &Option<Hero>,
    max_width: f32,
    total_height: f64,
    line_height: f64,
    occupied: &[Spot],
) -> (f64, f64) {
    let fragment = &fragments[plan.fragment];
    let mut preferred_x = if plan.emphasis > 1.0 { -plan.width / 2.0 } else { plan.base_x };
    let mut preferred_y = if plan.emphasis > 1.0 { 0.0 } else { plan.base_y };
    if plan.emphasis <= 1.0
        && let Some(hero) = hero
    {
        push_from_hero(&mut preferred_x, &mut preferred_y, plan, fragment.line_index, hero);
    }
    let field = Field { has_hero: hero.is_some(), max_width, total_height, line_height };
    search(plan, preferred_x, preferred_y, &field, occupied)
}

fn push_from_hero(x: &mut f64, y: &mut f64, plan: &Plan, line_index: usize, hero: &Hero) {
    let mut dx = *x + plan.width / 2.0 - hero.center_x;
    let mut dy = *y - plan.height * 0.46 - hero.center_y;
    if dx.hypot(dy) < 1.0 {
        dx = if *x >= 0.0 { 1.0 } else { -1.0 };
        dy = if line_index.is_multiple_of(2) { -0.65 } else { 0.65 };
    }
    let separation = hero.width * 0.34 + plan.width * 0.52 + plan.padding * 2.0;
    let distance = dx.hypot(dy);
    if distance < separation {
        let push = separation - distance;
        let length = distance.max(1.0);
        *x += dx / length * push;
        *y += dy / length * push * 0.92;
    }
}

struct Field {
    has_hero: bool,
    max_width: f32,
    total_height: f64,
    line_height: f64,
}

fn search(plan: &Plan, preferred_x: f64, preferred_y: f64, field: &Field, occupied: &[Spot]) -> (f64, f64) {
    let Field { has_hero, max_width, total_height, line_height } = *field;
    let max_radius = if plan.emphasis > 1.0 {
        20.0_f64.max(line_height * 0.5)
    } else {
        (line_height * 2.2).max(plan.collision_width * 0.75).max(56.0)
    };
    let horizontal_min = -f64::from(max_width) / 2.0 - 72.0;
    let horizontal_max = f64::from(max_width) / 2.0 + 72.0;
    let vertical_min = -(total_height * 0.9).max(line_height * 1.6);
    let vertical_max = (total_height * 0.9).max(line_height * 1.45);
    let base_angle =
        if has_hero && plan.emphasis <= 1.0 { preferred_y.atan2(preferred_x + plan.width / 2.0) } else { 0.0 };
    let mut best = (preferred_x, preferred_y, f64::INFINITY);
    let mut radius = 0.0;
    while radius <= max_radius {
        let samples = sample_count(radius, plan.emphasis, plan.step);
        for sample in 0..samples {
            let (dx, dy) = offset(radius, sample, samples, base_angle, plan.emphasis);
            let spot = rect(preferred_x + dx, preferred_y + dy, plan);
            if spot.left < horizontal_min
                || spot.right > horizontal_max
                || spot.top < vertical_min
                || spot.bottom > vertical_max
            {
                continue;
            }
            let area: f64 = occupied.iter().map(|held| overlap(&spot, held)).sum();
            let score = area * 2.2 + dx.hypot(dy);
            if score < best.2 {
                best = (preferred_x + dx, preferred_y + dy, score);
            }
            if area == 0.0 {
                return (preferred_x + dx, preferred_y + dy);
            }
        }
        if radius == 0.0 {
            radius = plan.step;
        } else {
            radius += plan.step;
        }
    }
    (best.0, best.1)
}

fn sample_count(radius: f64, emphasis: f64, step: f64) -> usize {
    if radius == 0.0 {
        1
    } else if emphasis > 1.0 {
        8
    } else {
        12.max(((std::f64::consts::PI * 2.0 * radius) / (step * 1.1).max(10.0)).round() as usize)
    }
}

fn offset(radius: f64, sample: usize, samples: usize, base_angle: f64, emphasis: f64) -> (f64, f64) {
    if radius == 0.0 {
        return (0.0, 0.0);
    }
    let angle = base_angle + sample as f64 / samples as f64 * std::f64::consts::PI * 2.0;
    let ellipse = if emphasis > 1.0 { 0.8 } else { 0.92 };
    (angle.cos() * radius, angle.sin() * radius * ellipse)
}

fn overlap(a: &Spot, b: &Spot) -> f64 {
    let width = (a.right.min(b.right) - a.left.max(b.left)).max(0.0);
    let height = (a.bottom.min(b.bottom) - a.top.max(b.top)).max(0.0);
    if width <= 0.0 || height <= 0.0 { 0.0 } else { width * height }
}

fn rect(x: f64, y: f64, plan: &Plan) -> Spot {
    Spot {
        left: x - plan.padding,
        top: y - plan.collision_height - plan.padding,
        right: x + plan.collision_width + plan.padding,
        bottom: y + plan.padding,
    }
}

fn finish(
    line: &Line,
    fragment: &Fragment,
    plan: &Plan,
    x: f64,
    y: f64,
    font_px: f32,
    intensity: AnimationIntensity,
) -> Placement {
    let seed = line.start * 1000.0
        + fragment.word_index as f64 * 17.0
        + fragment.line_index as f64 * 31.0
        + fragment.fragment_index as f64 * 13.0;
    let random = |offset: f64| {
        let value = (seed + offset).sin() * 10000.0;
        value - value.floor()
    };
    let outward_x = x + plan.width / 2.0;
    let outward_y = y - plan.height * 0.46;
    let length = outward_x.hypot(outward_y).max(1.0);
    let drift = if line.is_interlude() {
        3.0 + random(6.0) * 3.0
    } else if plan.emphasis > 1.0 {
        4.0 + random(6.0) * 4.0
    } else if intensity == AnimationIntensity::Chaotic {
        8.0 + random(6.0) * 9.0
    } else {
        5.0 + random(6.0) * 6.0
    };
    Placement {
        text: fragment.text.clone(),
        word_index: fragment.word_index,
        start: fragment.word_start,
        end: fragment.word_end,
        x,
        y,
        width: plan.width,
        height: plan.height,
        scale: plan.scale,
        passed_rotate: (random(3.0) - 0.5) * if intensity == AnimationIntensity::Chaotic { 20.0 } else { 12.0 },
        passed_drift_x: outward_x / length * drift + (random(7.0) - 0.5) * 2.4,
        passed_drift_y: outward_y / length * drift * 0.72 + (random(8.0) - 0.5) * 2.0,
        emphasis: plan.emphasis,
        glyphs: glyphs(fragment, font_px),
        interlude: line.is_interlude(),
        cjk: envelopes::is_cjk_text(&fragment.text),
    }
}

fn glyphs(fragment: &Fragment, font_px: f32) -> Vec<Glyph> {
    let chars: Vec<char> = fragment.text.chars().collect();
    let count = fragment.word_len.max(1);
    let unit = (fragment.word_end - fragment.word_start).max(0.0) / count as f64;
    let mut offset = 0.0_f32;
    chars
        .iter()
        .enumerate()
        .map(|(local, ch)| {
            let index = fragment.start_in_word + local;
            let start = fragment.word_start + unit * index as f64;
            let end =
                if index + 1 == count { fragment.word_end } else { fragment.word_start + unit * (index as f64 + 1.0) };
            let text = ch.to_string();
            let glyph = Glyph { text: text.clone(), offset, start, end };
            offset += crate::measure(&text, font_px, crate::BOLD, 10_000.0).width;
            glyph
        })
        .collect()
}
