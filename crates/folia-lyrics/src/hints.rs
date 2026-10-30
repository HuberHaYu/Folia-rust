// Per-line render hints shared by every visualizer: how fast a line enters, reveals its words and leaves.

use folia_core::Line;

const MICRO_LINE: f64 = 0.10;
const SHORT_LINE: f64 = 0.18;
const MICRO_RENDER_FLOOR: f64 = 0.067;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Normal,
    Fast,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reveal {
    Normal,
    Fast,
    Instant,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderHints {
    /// The latest time a visualizer may keep the line on screen.
    pub render_end: f64,
    pub transition: Transition,
    pub reveal: Reveal,
}

impl RenderHints {
    pub fn of(line: &Line) -> RenderHints {
        let duration = (line.end - line.start).max(0.0);
        let (transition, reveal) = if duration < MICRO_LINE {
            (Transition::None, Reveal::Instant)
        } else if duration < SHORT_LINE {
            (Transition::Fast, Reveal::Fast)
        } else {
            (Transition::Normal, Reveal::Normal)
        };
        RenderHints { render_end: render_end(line, duration, transition, reveal), transition, reveal }
    }

    /// How early a word lights up before its start time.
    pub fn word_lookahead(self) -> f64 {
        match self.reveal {
            Reveal::Instant => 0.03,
            Reveal::Fast => 0.08,
            Reveal::Normal => 0.15,
        }
    }

    /// When a word stops being the active one.
    pub fn word_active_end(self, start: f64, end: f64) -> f64 {
        match self.reveal {
            Reveal::Instant => self.render_end,
            Reveal::Fast => self.render_end.min(end.max(start + 0.12)),
            Reveal::Normal => end,
        }
    }
}

/// Index of the latest line whose render window contains `time`.
pub fn active_line(lines: &[Line], time: f64) -> Option<usize> {
    lines.iter().rposition(|line| time >= line.start && time <= RenderHints::of(line).render_end)
}

fn render_end(line: &Line, duration: f64, transition: Transition, reveal: Reveal) -> f64 {
    if transition == Transition::None {
        return line.end.max(line.start + MICRO_RENDER_FLOOR);
    }
    let base = duration.max(0.12);
    let (enter, exit, hold) = match transition {
        Transition::Fast => {
            ((duration * 0.45).clamp(0.045, 0.06), (duration * 0.22).clamp(0.03, 0.04), fast_hold(reveal))
        }
        _ => ((base * 0.34).clamp(0.22, 0.42), (base * 0.18).clamp(0.18, 0.32), normal_hold(reveal)),
    };
    let last_word_end = line.words.last().map_or(line.end, |word| word.end);
    let pass_start = last_word_end.max(line.start) + hold;
    let exit_start = match transition {
        Transition::Fast => (line.start + enter + 0.01).max(pass_start).max(line.end - exit),
        _ => pass_start.max(line.end - exit),
    };
    line.end.max(exit_start + exit)
}

fn fast_hold(reveal: Reveal) -> f64 {
    if reveal == Reveal::Instant { 0.0 } else { 0.03 }
}

fn normal_hold(reveal: Reveal) -> f64 {
    if reveal == Reveal::Instant { 0.0 } else { 0.06 }
}
