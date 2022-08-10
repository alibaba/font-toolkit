use std::collections::VecDeque;

use textwrap::Options;
use textwrap::WordSplitter::NoHyphenation;
use unicode_normalization::UnicodeNormalization;

use crate::{FontKey, TextMetrics};

#[derive(Debug, Clone)]
pub struct Line<T> {
    pub spans: Vec<Span<T>>,
    pub hard_break: bool,
}

impl<T> Line<T> {
    pub fn width(&self) -> f32 {
        self.spans
            .iter()
            .fold(0.0, |current, span| current + span.width())
    }

    pub fn height(&self) -> f32 {
        self.spans
            .iter()
            .fold(0.0, |current, span| current.max(span.height()))
    }

    pub fn spans(&self) -> &[Span<T>] {
        &self.spans
    }

    pub fn new(span: Span<T>) -> Self {
        Line {
            spans: vec![span],
            hard_break: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Span<T> {
    pub font_key: FontKey,
    pub letter_spacing: f32,
    pub line_height: Option<f32>,
    pub size: f32,
    pub broke_from_prev: bool,
    pub metrics: TextMetrics,
    pub swallow_leading_space: bool,
    pub additional: T,
}

impl<T> Span<T> {
    fn width(&self) -> f32 {
        let mut width = self.metrics.width(self.size, self.letter_spacing);
        if self.swallow_leading_space {
            let c = &self.metrics.positions[0];
            width -=
                c.metrics.advanced_x as f32 / c.metrics.units * self.size + self.letter_spacing;
        }
        width
    }

    fn height(&self) -> f32 {
        self.metrics.height(self.size, self.line_height)
    }
}

/// Metrics of an area of rich-content text
#[derive(Debug, Clone)]
pub struct Area<T> {
    pub lines: Vec<Line<T>>,
}

impl<T> Area<T>
where
    T: Clone,
{
    pub fn new() -> Area<T> {
        Area { lines: vec![] }
    }

    // The height of a text area
    pub fn height(&self) -> f32 {
        self.lines
            .iter()
            .fold(0.0, |current, line| current + line.height())
    }

    /// The width of a text area
    pub fn width(&self) -> f32 {
        self.lines
            .iter()
            .fold(0.0_f32, |current, line| current.max(line.width()))
    }

    pub fn unwrap_text(&mut self) {
        let has_soft_break = self.lines.iter().any(|line| !line.hard_break);
        if !has_soft_break {
            return;
        }
        let lines = std::mem::replace(&mut self.lines, Vec::new());
        for line in lines {
            if line.hard_break {
                self.lines.push(line);
            } else {
                let last_line = &mut self.lines.last_mut().unwrap().spans;
                for mut span in line.spans {
                    span.swallow_leading_space = false;
                    if span.broke_from_prev {
                        if let Some(last_span) = last_line.last_mut() {
                            last_span.metrics.value.push_str(&span.metrics.value);
                            last_span
                                .metrics
                                .positions
                                .append(&mut span.metrics.positions);
                        } else {
                            last_line.push(span);
                        }
                    } else {
                        last_line.push(span);
                    }
                }
            }
        }
    }

    pub fn wrap_text(&mut self, width: f32) {
        let mut lines = self.lines.clone().into_iter().collect::<VecDeque<_>>();
        let mut result = vec![];
        let mut current_line = Line {
            hard_break: true,
            spans: Vec::new(),
        };
        let mut current_line_width = 0.0;
        let mut is_first_line = true;
        let mut failed_with_no_acception = false;
        while let Some(mut line) = lines.pop_front() {
            if line.hard_break && !is_first_line {
                // Start a new line
                result.push(std::mem::replace(
                    &mut current_line,
                    Line {
                        hard_break: true,
                        spans: Vec::new(),
                    },
                ));
                current_line_width = 0.0;
            }
            is_first_line = false;
            let line_width = line.width();
            if width - (line_width + current_line_width) >= -0.01 {
                // Current line fits, push all of its spans into current line
                current_line_width += line_width;
                current_line.spans.append(&mut line.spans);
            } else {
                // Set line letter-spacing to min(zero, letter-spacing)
                for span in &mut line.spans {
                    span.letter_spacing = span.letter_spacing.min(0.0)
                }
                // Go through spans to get the first not-fitting span
                let index = line.spans.iter().position(|span| {
                    let span_width = span.width();
                    if span_width + current_line_width <= width {
                        current_line_width += span_width;
                        false
                    } else {
                        true
                    }
                });
                let index = match index {
                    Some(index) => index,
                    None => {
                        // after shrinking letter-spacing, the line fits
                        current_line_width += line.width();
                        current_line.spans.append(&mut line.spans);
                        continue;
                    }
                };
                // put all spans before this into the line
                let mut approved_spans = line.spans.split_off(index);
                std::mem::swap(&mut approved_spans, &mut line.spans);
                if approved_spans.is_empty() {
                    if failed_with_no_acception {
                        // Failed to fit a span twice, fail
                        return;
                    } else {
                        failed_with_no_acception = true;
                    }
                } else {
                    failed_with_no_acception = false;
                }
                current_line.spans.append(&mut approved_spans);
                let mut dropped_metrics = vec![];
                let span = &mut line.spans[0];
                let fixed_value = span.metrics.value().to_string();
                // Try to find a naive break point
                let mut naive_break_index = 0;
                // Textwrap cannot find a good break point, we directly drop chars
                while let Some(m) = span.metrics.positions.pop() {
                    dropped_metrics.push(m);
                    let span_width = span.width();
                    if span_width + current_line_width <= width {
                        naive_break_index = span.metrics.positions.len();
                        dropped_metrics.reverse();
                        span.metrics.positions.append(&mut dropped_metrics);
                        break;
                    }
                }
                let options = Options::new(textwrap::core::display_width(
                    &fixed_value
                        .nfc()
                        .take(naive_break_index)
                        .collect::<String>(),
                ))
                .word_splitter(NoHyphenation);
                let wrapped = textwrap::wrap(&*fixed_value, options);
                log::trace!("{:?}", wrapped);
                let mut real_index = 0;
                log::debug!(
                    "wrapped nfc count {}, metrics {}",
                    wrapped.iter().map(|span| span.nfc().count()).sum::<usize>(),
                    span.metrics.positions.len()
                );
                for seg in wrapped {
                    let count = seg.nfc().count();
                    if count == 0 {
                        continue;
                    }
                    let span_values = seg.nfc().collect::<Vec<_>>();
                    while span.metrics.positions()[real_index].metrics.c == ' '
                        && span_values
                            .get(real_index)
                            .map(|c| *c != ' ')
                            .unwrap_or(true)
                    {
                        real_index += 1;
                    }
                    let factor = span.size / span.metrics.units() as f32;
                    let acc_seg_width = (0..(real_index + count))
                        .map(|index| span.metrics.positions.get(index).unwrap())
                        .fold(0.0, |current, p| {
                            current
                                + p.kerning as f32 * factor
                                + p.metrics.advanced_x as f32 * factor
                                + span.letter_spacing
                        });
                    if current_line_width + acc_seg_width <= width {
                        real_index += count;
                    } else {
                        break;
                    }
                }
                if real_index == 0 {
                    real_index = naive_break_index
                }

                // Split here, create a new span
                let mut new_span = span.clone();
                new_span.broke_from_prev = true;
                new_span.metrics.positions = span.metrics.positions.split_off(real_index);
                let mut chars = fixed_value.nfc().collect::<Vec<_>>();
                log::trace!("real_index {} index {}, {:?}", real_index, index, chars);
                let new_chars = chars.split_off(real_index);
                span.metrics.value = chars.into_iter().collect::<String>();
                new_span.metrics.value = new_chars.into_iter().collect::<String>();
                if !span.metrics.value.is_empty() {
                    current_line.spans.push(span.clone());
                }
                assert_eq!(
                    span.metrics.value.nfc().count(),
                    span.metrics.positions.len()
                );
                assert_eq!(
                    new_span.metrics.value.nfc().count(),
                    new_span.metrics.positions.len()
                );
                // Create a new line
                result.push(std::mem::replace(
                    &mut current_line,
                    Line {
                        hard_break: false,
                        spans: Vec::new(),
                    },
                ));
                // Add new_span to next line
                let mut new_line = Line {
                    hard_break: false,
                    spans: vec![new_span],
                };
                // Check for swallowed leading space
                if new_line.spans[0].metrics.value.starts_with(" ") {
                    new_line.spans[0].swallow_leading_space = true;
                }
                for span in line.spans.into_iter().skip(1) {
                    new_line.spans.push(span);
                }
                lines.push_front(new_line);
                current_line_width = 0.0;
            }
        }
        if !current_line.spans.is_empty() {
            result.push(current_line);
        }
        if result.is_empty() || result[0].spans.is_empty() {
            return;
        }
        self.lines = result;
    }

    pub fn valid(&self) -> bool {
        let metrics_empty = self.lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.metrics.positions.is_empty())
        });
        let content_empty = self.value_string().is_empty();
        !metrics_empty || content_empty
    }

    pub fn value_string(&self) -> String {
        self.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.metrics.value.clone())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn span_count(&self) -> usize {
        self.lines
            .iter()
            .fold(0, |current, line| line.spans.len() + current)
    }

    pub fn ellipsis(&mut self, width: f32, height: f32, postfix: TextMetrics) {
        // No need to do ellipsis
        if height - self.height() >= -0.01 && width - self.width() >= -0.01 {
            return;
        }
        let mut ellipsis_span = self.lines[0].spans[0].clone();
        let mut lines_height = 0.0;
        self.lines = self
            .lines
            .clone()
            .drain_filter(|line| {
                lines_height += line.height();
                height - lines_height >= -0.01
            })
            .collect();

        for line in &mut self.lines {
            line.hard_break = true;
            if let Some(ref mut first) = line.spans.first_mut() {
                first.metrics.trim_start();
            }
        }

        if let Some(ref mut line) = self.lines.last_mut() {
            while line.width() + postfix.width(ellipsis_span.size, ellipsis_span.letter_spacing)
                - width
                >= 0.01
                && line.width() > 0.0
            {
                let span = line.spans.last_mut().unwrap();
                span.metrics.pop();
                if span.metrics.positions.is_empty() {
                    line.spans.pop();
                }
            }
            ellipsis_span.metrics = postfix;
            line.spans.push(ellipsis_span);
        }
    }
}
