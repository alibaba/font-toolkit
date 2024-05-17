use std::collections::VecDeque;

use textwrap::Options;
use textwrap::WordSplitter::NoHyphenation;
use unicode_normalization::UnicodeNormalization;

use crate::{Error, FontKey, TextMetrics};

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

    fn is_rtl(&self) -> bool {
        self.spans.iter().all(|span| span.is_rtl())
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
        if self.metrics.positions.is_empty() {
            return 0.0;
        }
        let mut width = self.metrics.width(self.size, self.letter_spacing);
        if self.swallow_leading_space && self.metrics.positions[0].metrics.c == ' ' {
            let c = &self.metrics.positions[0];
            width -=
                c.metrics.advanced_x as f32 / c.metrics.units * self.size + self.letter_spacing;
        }
        width
    }

    fn height(&self) -> f32 {
        self.metrics.height(self.size, self.line_height)
    }

    fn is_rtl(&self) -> bool {
        self.metrics
            .positions
            .iter()
            .all(|p| p.level.map(|l| l.is_rtl()).unwrap_or_default())
    }
}

/// Metrics of an area of rich-content text
#[derive(Debug, Clone, Default)]
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
                let last_line = self.lines.last_mut().unwrap();
                let rtl = last_line.is_rtl() && line.is_rtl();
                let last_line = &mut last_line.spans;
                for mut span in line.spans {
                    span.swallow_leading_space = false;
                    if rtl {
                        if span.broke_from_prev {
                            if let Some(first_span) = last_line.first_mut() {
                                span.metrics.value.push_str(&first_span.metrics.value);
                                span.metrics
                                    .positions
                                    .append(&mut first_span.metrics.positions);
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
    }

    pub fn wrap_text(&mut self, width: f32) -> Result<(), Error> {
        let rtl = self.lines.iter().all(|line| {
            line.spans.iter().all(|span| {
                span.metrics
                    .positions
                    .iter()
                    .all(|p| p.level.map(|l| l.is_rtl()).unwrap_or_default())
            })
        });
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
                    .map(|span| span.metrics.value.clone())
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
            if width - (line_width + current_line_width) >= -0.01 {
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
                        return Ok(());
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
                let total_count = span.metrics.positions.len();
                if rtl {
                    span.metrics.positions.reverse();
                }
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
                if rtl {
                    naive_break_index = total_count - naive_break_index;
                    span.metrics.positions.reverse();
                }
                // NOTE: str.nfc() & textwrap all handles RTL text well, so we do
                // not take extra effort here
                let display_str = fixed_value
                    .nfc()
                    .take(naive_break_index)
                    .collect::<String>();
                let display_width = textwrap::core::display_width(&display_str);
                let options = Options::new(display_width)
                    .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit)
                    .word_splitter(NoHyphenation);
                let wrapped = textwrap::wrap(&*fixed_value, options);
                log::trace!("{:?}", wrapped);
                let mut real_index = 0;
                log::debug!(
                    "wrapped nfc count {}, metrics {}",
                    wrapped.iter().map(|span| span.nfc().count()).sum::<usize>(),
                    span.metrics.positions.len()
                );
                if rtl {
                    real_index = total_count - 1;
                }
                for seg in wrapped {
                    let count = seg.nfc().count();
                    if count == 0 {
                        continue;
                    }
                    let span_values = seg.nfc().collect::<Vec<_>>();
                    let mut current_real_index = real_index;
                    while span.metrics.positions()[current_real_index].metrics.c == ' '
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
                    let factor = span.size / span.metrics.units() as f32;
                    let range = if rtl {
                        (current_real_index + 1 - count)..current_real_index
                    } else {
                        current_real_index..(current_real_index + count)
                    };
                    let acc_seg_width = range
                        .map(|index| span.metrics.positions.get(index).unwrap())
                        .fold(0.0, |current, p| {
                            current
                                + p.kerning as f32 * factor
                                + p.metrics.advanced_x as f32 * factor
                                + span.letter_spacing
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
                            .map(|index| span.metrics.positions.get(index).unwrap())
                            .fold(0.0, |current, p| {
                                current
                                    + p.kerning as f32 * factor
                                    + p.metrics.advanced_x as f32 * factor
                                    + span.letter_spacing
                            })
                    };
                    if current_line_width + acc_seg_width_with_space <= width {
                        if rtl {
                            real_index = current_real_index - count;
                        } else {
                            real_index = current_real_index + count;
                        }
                        current_line_width += acc_seg_width;
                    } else {
                        break;
                    }
                }
                if (real_index == 0 && !rtl)
                    || (rtl && real_index == span.metrics.positions.len() - 1)
                {
                    real_index = naive_break_index;
                }

                // Split here, create a new span
                let mut new_span = span.clone();
                new_span.broke_from_prev = true;
                new_span.metrics.positions = span.metrics.positions.split_off(real_index);
                let mut chars = fixed_value.nfc().collect::<Vec<_>>();
                let new_chars = chars.split_off(real_index);
                log::trace!(
                    "real_index {} index {}, {:?}, {:?}",
                    real_index,
                    index,
                    chars,
                    new_chars
                );
                span.metrics.value = chars.into_iter().collect::<String>();
                new_span.metrics.value = new_chars.into_iter().collect::<String>();
                if rtl {
                    std::mem::swap(span, &mut new_span);
                }
                if !span.metrics.value.is_empty() {
                    current_line.spans.push(span.clone());
                }
                if span.metrics.value.nfc().count() != span.metrics.positions.len() {
                    return Err(Error::MetricsMismatch {
                        value: span.metrics.value.nfc().collect(),
                        metrics: span.metrics.positions.clone(),
                    });
                }
                if new_span.metrics.value.nfc().count() != new_span.metrics.positions.len() {
                    return Err(Error::MetricsMismatch {
                        value: new_span.metrics.value.nfc().collect(),
                        metrics: new_span.metrics.positions.clone(),
                    });
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
                if real_index != 0 {
                    failed_with_no_acception = false;
                }
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
        !self.lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.metrics.positions.is_empty() && !span.metrics.value.is_empty())
        })
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
            let mut removed = false;
            while line.width() + postfix.width(ellipsis_span.size, ellipsis_span.letter_spacing)
                - width
                >= 0.01
                && line.width() > 0.0
            {
                removed = true;
                let span = line.spans.last_mut().unwrap();
                span.metrics.pop();
                if span.metrics.positions.is_empty() {
                    line.spans.pop();
                }
            }
            if removed {
                ellipsis_span.metrics = postfix;
                line.spans.push(ellipsis_span);
            }
        }
    }
}
