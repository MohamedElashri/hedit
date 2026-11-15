//! Bounded, RAM-only samples. Weak identity never retains remote document text.
use super::*;
use std::sync::Weak;
const MAX_SAMPLES: usize = 4096;
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Sample {
    pub leading: u8,
    pub width: u8,
}
#[derive(Default)]
pub(super) struct Cache {
    projection: Weak<hedit::advanced::Projection>,
    samples: Arc<Vec<Sample>>,
}
impl Cache {
    pub fn get(
        &mut self,
        projection: &Arc<hedit::advanced::Projection>,
        height: f32,
    ) -> Arc<Vec<Sample>> {
        let rows = projection
            .source_lines
            .len()
            .saturating_sub(usize::from(projection.text.ends_with('\n')))
            .max(1);
        let count = (height as usize).clamp(1, MAX_SAMPLES).min(rows);
        if self.samples.len() != count
            || self
                .projection
                .upgrade()
                .is_none_or(|old| !Arc::ptr_eq(&old, projection))
        {
            let mut samples = Vec::with_capacity(count);
            for (n, line) in projection.text.lines().enumerate() {
                if n == samples.len() * rows / count {
                    samples.push(Sample {
                        leading: line
                            .chars()
                            .take(100)
                            .take_while(|c| c.is_whitespace())
                            .count() as u8,
                        width: line.chars().take(120).count() as u8,
                    });
                    if samples.len() == count {
                        break;
                    }
                }
            }
            if samples.is_empty() {
                samples.push(Sample {
                    leading: 0,
                    width: 0,
                });
            }
            self.samples = Arc::new(samples);
            self.projection = Arc::downgrade(projection);
        }
        Arc::clone(&self.samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn samples_match_reference_for_unicode_crlf_tabs_folds_and_heights() {
        for text in [
            "",
            "\n",
            "α🦀\r\n  שלום\n\nlast",
            "a\n",
            &" \t界".repeat(300),
            &"  x\r\n".repeat(10_000),
        ] {
            for folds in [
                vec![],
                vec![hedit::advanced::Fold {
                    start: 2,
                    end: text.chars().count().saturating_sub(2),
                }],
            ] {
                let projection = Arc::new(hedit::advanced::Projection::new(text, 4, &folds));
                let mut cache = Cache::default();
                for height in [0.0, 1.0, 7.5, 800.0, 100_000.0, f32::INFINITY, f32::NAN] {
                    let got = cache.get(&projection, height);
                    let lines: Vec<_> = projection.text.lines().collect();
                    let rows = lines.len().max(1);
                    let count = (height as usize).clamp(1, MAX_SAMPLES).min(rows);
                    let expected: Vec<_> = (0..count)
                        .map(|n| {
                            let line = lines.get(n * rows / count).copied().unwrap_or("");
                            Sample {
                                leading: line
                                    .chars()
                                    .take_while(|c| c.is_whitespace())
                                    .count()
                                    .min(100) as u8,
                                width: line.chars().count().min(120) as u8,
                            }
                        })
                        .collect();
                    assert_eq!(*got, expected);
                    assert!(Arc::ptr_eq(&got, &cache.get(&projection, height)));
                    assert!(got.len() <= MAX_SAMPLES);
                }
            }
        }
    }
    #[test]
    fn cache_invalidates_without_retaining_document_text() {
        let mut cache = Cache::default();
        let first = Arc::new(hedit::advanced::Projection::new("one\n", 4, &[]));
        let old = cache.get(&first, 100.0);
        let weak = Arc::downgrade(&first);
        drop(first);
        assert!(weak.upgrade().is_none());
        let next = Arc::new(hedit::advanced::Projection::new(" longer\n", 4, &[]));
        let new = cache.get(&next, 100.0);
        assert!(!Arc::ptr_eq(&old, &new));
        assert_eq!(
            new[0],
            Sample {
                leading: 1,
                width: 7
            }
        );
    }
}
