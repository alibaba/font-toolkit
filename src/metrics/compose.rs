use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

use textwrap::Options;
use textwrap::WordSplitter::NoHyphenation;
use unicode_normalization::UnicodeNormalization;

use crate::metrics::TextMetrics;
use crate::{Error, FontKey};

#[derive(Debug, Clone)]
pub struct Line<T, M> {
    pub spans: Vec<Span<T, M>>,
    pub hard_break: bool,
}

impl<T, M: Metrics> Line<T, M> {
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

    pub fn spans(&self) -> &[Span<T, M>] {
        &self.spans
    }

    pub fn new(span: Span<T, M>) -> Self {
        Line {
            spans: vec![span],
            hard_break: true,
        }
    }

    fn is_rtl(&self) -> bool {
        self.spans.iter().all(|span| span.metrics.is_rtl())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Span<T, M> {
    pub font_key: FontKey,
    pub letter_spacing: f32,
    pub line_height: Option<f32>,
    pub size: f32,
    pub broke_from_prev: bool,
    pub metrics: M,
    pub swallow_leading_space: bool,
    pub additional: T,
}

impl<T, M: Metrics> Span<T, M> {
    fn width(&self) -> f32 {
        if self.metrics.count() == 0 {
            return 0.0;
        }
        let mut width = self.metrics.width(self.size, self.letter_spacing);
        if self.swallow_leading_space {
            let metrics = self.metrics.slice(0, 1);
            if metrics.value() == "" {
                width -= metrics.width(self.size, self.letter_spacing);
            }
        }
        width
    }

    fn height(&self) -> f32 {
        self.metrics.height(self.size, self.line_height)
    }
}

/// Metrics of an area of rich-content text
#[derive(Debug, Clone, Default)]
pub struct Area<T, M> {
    pub lines: Vec<Line<T, M>>,
}

impl<T, M: Metrics> Area<T, M>
where
    T: Clone,
{
    pub fn new() -> Area<T, M> {
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
                let last_line = self.lines.last_mut().unwrap();
                let rtl = last_line.is_rtl() && line.is_rtl();
                let last_line = &mut last_line.spans;
                for mut span in line.spans {
                    span.swallow_leading_space = false;
                    if rtl {
                        if span.broke_from_prev {
                            if let Some(first_span) = last_line.first_mut() {
                                span.metrics.append(first_span.metrics.duplicate());
                                std::mem::swap(first_span, &mut span);
                            } else {
                                last_line.insert(0, span);
                            }
                        } else {
                            last_line.insert(0, span);
                        }
                    } else {
                        if span.broke_from_prev {
                            if let Some(last_span) = last_line.last_mut() {
                                last_span.metrics.append(span.metrics.duplicate());
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
    }

    pub fn wrap_text(&mut self, width: f32) -> Result<(), Error> {
        let rtl = self
            .lines
            .iter()
            .all(|line| line.spans.iter().all(|span| span.metrics.is_rtl()));
        let mut lines = self.lines.clone().into_iter().collect::<VecDeque<_>>();
        if rtl {
            lines.make_contiguous().reverse();
        }
        let mut result = vec![];
        let mut current_line = Line {
            hard_break: true,
            spans: Vec::new(),
        };
        let mut current_line_width = 0.0;
        let mut is_first_line = true;
        let mut failed_with_no_acception = false;
        while let Some(mut line) = lines.pop_front() {
            log::trace!(
                "current line {}",
                line.spans
                    .iter()
                    .map(|span| span.metrics.value())
                    .collect::<Vec<_>>()
                    .join("")
            );
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
            if width - (line_width + current_line_width) >= -0.1 {
                // Current line fits, push all of its spans into current line
                current_line_width += line_width;
                current_line.spans.append(&mut line.spans);
            } else {
                if rtl {
                    line.spans.reverse();
                }
                // Go through spans to get the first not-fitting span
                let index = line.spans.iter().position(|span| {
                    let span_width = span.width();
                    if span_width + current_line_width - width <= 0.1 {
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
                        return Ok(());
                    } else {
                        failed_with_no_acception = true;
                    }
                } else {
                    failed_with_no_acception = false;
                }
                current_line.spans.append(&mut approved_spans);
                let span = &mut line.spans[0];
                let new_metrics = span.metrics.split_by_width(
                    span.size,
                    span.letter_spacing,
                    width - current_line_width,
                );
                if new_metrics.count() != 0 {
                    failed_with_no_acception = false;
                }
                let mut new_span = span.clone();
                new_span.metrics = new_metrics;
                new_span.broke_from_prev = true;
                if rtl {
                    std::mem::swap(span, &mut new_span);
                }
                if span.metrics.count() != 0 {
                    current_line.spans.push(span.clone());
                }
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
                    spans: vec![],
                };
                if new_span.metrics.count() != 0 {
                    new_line.spans.push(new_span);
                }
                for span in line.spans.into_iter().skip(1) {
                    new_line.spans.push(span);
                }
                current_line_width = 0.0;
                if new_line.spans.is_empty() {
                    continue;
                }
                // Check for swallowed leading space
                if new_line.spans[0].metrics.value().starts_with(" ") {
                    new_line.spans[0].swallow_leading_space = true;
                }
                lines.push_front(new_line);
            }
        }
        if !current_line.spans.is_empty() {
            result.push(current_line);
        }
        if result.is_empty() || result[0].spans.is_empty() {
            return Ok(());
        }
        self.lines = result;
        log::trace!("adjust result: {}", self.value_string());
        Ok(())
    }

    pub fn valid(&self) -> bool {
        !self
            .lines
            .iter()
            .any(|line| line.spans.iter().any(|span| span.metrics.units() == 0.0))
    }

    pub fn value_string(&self) -> String {
        self.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.metrics.value())
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

    pub fn ellipsis(&mut self, width: f32, height: f32, postfix: M) {
        // No need to do ellipsis
        if height - self.height() >= -0.01 && width - self.width() >= -0.01 {
            return;
        }
        let mut ellipsis_span = self.lines[0].spans[0].clone();
        let mut lines_height = 0.0;
        let index = std::cmp::max(
            1,
            self.lines
                .iter()
                .position(|line| {
                    lines_height += line.height();
                    height - lines_height < -0.01
                })
                .unwrap_or(1),
        );
        let _ = self.lines.split_off(index);

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
                if span.metrics.count() == 0 {
                    line.spans.pop();
                }
            }
            ellipsis_span.metrics = postfix;
            line.spans.push(ellipsis_span);
        }
    }
}

pub trait Metrics: Clone {
    fn new(value: String) -> Self;
    fn duplicate(&self) -> Self;
    fn width(&self, font_size: f32, letter_spacing: f32) -> f32;
    fn height(&self, font_size: f32, line_height: Option<f32>) -> f32;
    fn ascender(&self, font_size: f32) -> f32;
    fn line_gap(&self) -> f32;
    fn slice(&self, start: u32, count: u32) -> Self;
    fn value(&self) -> String;
    fn units(&self) -> f32;
    fn is_rtl(&self) -> bool;
    fn append(&self, other: Self);
    fn count(&self) -> u32;
    /// replace this metrics with another, allowing fallback
    /// logic
    fn replace(&self, other: Self, fallback: bool);
    fn split_by_width(&self, font_size: f32, letter_spacing: f32, width: f32) -> Self;
    fn chars(&self) -> Vec<char>;
    fn trim_start(&self) {
        loop {
            let m = self.slice(0, 1);
            if m.value() == " " {
                self.replace(self.slice(1, self.count() as u32 - 1), false);
            } else {
                break;
            }
        }
    }

    fn pop(&self) {
        self.replace(self.slice(0, self.count() as u32 - 1), false);
    }
}

impl Metrics for TextMetrics {
    fn new(value: String) -> Self {
        TextMetrics::new(value)
    }

    fn duplicate(&self) -> TextMetrics {
        self.clone()
    }

    fn width(&self, font_size: f32, letter_spacing: f32) -> f32 {
        TextMetrics::width(&self, font_size, letter_spacing)
    }

    fn height(&self, font_size: f32, line_height: Option<f32>) -> f32 {
        TextMetrics::height(&self, font_size, line_height)
    }

    fn ascender(&self, font_size: f32) -> f32 {
        let factor = font_size / self.units() as f32;
        (self.ascender() as f32 + self.line_gap() as f32 / 2.0) * factor
    }

    fn line_gap(&self) -> f32 {
        self.line_gap() as f32 / self.units() as f32
    }

    fn slice(&self, start: u32, count: u32) -> TextMetrics {
        TextMetrics::slice(&self, start, count)
    }

    fn value(&self) -> String {
        TextMetrics::value(&self)
    }

    fn is_rtl(&self) -> bool {
        TextMetrics::is_rtl(&self)
    }

    fn append(&self, other: TextMetrics) {
        TextMetrics::append(&self, other)
    }

    fn count(&self) -> u32 {
        TextMetrics::count(&self) as u32
    }

    fn replace(&self, other: TextMetrics, fallback: bool) {
        TextMetrics::replace(&self, other, fallback)
    }

    fn split_by_width(&self, font_size: f32, letter_spacing: f32, width: f32) -> TextMetrics {
        TextMetrics::split_by_width(&self, font_size, letter_spacing, width)
    }

    fn chars(&self) -> Vec<char> {
        let p = self.positions.read().unwrap();
        p.iter().map(|c| c.metrics.c).collect()
    }

    fn units(&self) -> f32 {
        self.units() as f32
    }
}

impl TextMetrics {
    pub(crate) fn split_by_width(&self, font_size: f32, letter_spacing: f32, width: f32) -> Self {
        // Try to find a naive break point
        let total_count = self.count();
        let mut naive_break_index = total_count;
        let rtl = self.is_rtl();
        if rtl {
            self.positions.write().unwrap().reverse();
        }
        // Textwrap cannot find a good break point, we directly drop chars
        loop {
            let span_width = self.width_until(font_size, letter_spacing, naive_break_index);
            if span_width - width <= 0.1 || naive_break_index == 0 {
                break;
            }
            naive_break_index -= 1;
        }

        // NOTE: str.nfc() & textwrap all handles RTL text well, so we do
        // not take extra effort here
        let positions = self.positions.read().unwrap();
        let positions_rev = positions
            .iter()
            .take(naive_break_index)
            .map(|c| c.metrics.c);
        let display_str = if rtl {
            positions_rev.rev().collect::<String>()
        } else {
            positions_rev.collect::<String>()
        };
        let display_width = textwrap::core::display_width(&display_str);
        let options = Options::new(display_width)
            .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit)
            .word_splitter(NoHyphenation);
        let value = self.value();
        let wrapped = textwrap::wrap(&value, options);
        log::trace!("{:?}", wrapped);
        let mut real_index = 0;
        if rtl {
            real_index = total_count - 1;
        }
        let mut current_line_width = 0.0;
        for seg in wrapped {
            let count = seg.nfc().count();
            if count == 0 {
                continue;
            }
            let span_values = seg.nfc().collect::<Vec<_>>();
            let mut current_real_index = real_index;
            while positions[current_real_index].metrics.c == ' '
                && span_values
                    .get(current_real_index - real_index)
                    .map(|c| *c != ' ')
                    .unwrap_or(true)
            {
                if rtl {
                    current_real_index -= 1;
                } else {
                    current_real_index += 1;
                }
            }
            let factor = font_size / self.units() as f32;
            let range = if rtl {
                (current_real_index + 1 - count)..current_real_index
            } else {
                current_real_index..(current_real_index + count)
            };
            let acc_seg_width =
                range
                    .map(|index| positions.get(index).unwrap())
                    .fold(0.0, |current, p| {
                        current
                            + p.kerning as f32 * factor
                            + p.metrics.advanced_x as f32 * factor
                            + letter_spacing
                    });
            let acc_seg_width_with_space = if current_real_index == real_index {
                acc_seg_width
            } else {
                let range = if rtl {
                    (current_real_index + 1 - count)..real_index
                } else {
                    real_index..(current_real_index + count)
                };
                range
                    .map(|index| positions.get(index).unwrap())
                    .fold(0.0, |current, p| {
                        current
                            + p.kerning as f32 * factor
                            + p.metrics.advanced_x as f32 * factor
                            + letter_spacing
                    })
            };
            if current_line_width + acc_seg_width_with_space <= width {
                if rtl {
                    if current_real_index < count {
                        real_index = 0;
                        break;
                    }
                    real_index = current_real_index - count;
                } else {
                    real_index = current_real_index + count;
                }
                current_line_width += acc_seg_width;
            } else {
                break;
            }
        }
        if (real_index == 0 && !rtl) || (rtl && real_index == positions.len() - 1) {
            real_index = naive_break_index;
        }

        drop(positions);
        // Split here, create a new span
        let mut new_metrics = self.clone();
        new_metrics.positions = {
            let mut p = self.positions.write().unwrap();
            Arc::new(RwLock::new(p.split_off(real_index)))
        };
        new_metrics
    }
}
