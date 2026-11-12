//! Detection, localisation and false positives.

use serde::Serialize;

use crate::corpus::{Answer, Defect, Expected, ExpectedDefect, ExpectedKind};

#[derive(Debug, Clone, Serialize)]
pub struct Score {
    pub detected: bool,
    /// The mean IoU of the expected defects that matched.
    pub iou: Option<f64>,
    /// Reported defects that matched nothing expected.
    pub false_positives: usize,
    pub matched: usize,
}

/// The model's `[x0, y0, x1, y1]` in 0–1000 as full-image `[x, y, w, h]`.
pub fn to_pixels(bounds: [f64; 4], size: (u32, u32)) -> [f64; 4] {
    let width = f64::from(size.0);
    let height = f64::from(size.1);
    let [x0, y0, x1, y1] = bounds;
    [
        x0 / 1000.0 * width,
        y0 / 1000.0 * height,
        (x1 - x0) / 1000.0 * width,
        (y1 - y0) / 1000.0 * height,
    ]
}

fn area(bounds: [f64; 4]) -> f64 {
    bounds[2].max(0.0) * bounds[3].max(0.0)
}

fn intersection(a: [f64; 4], b: [f64; 4]) -> f64 {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = (a[0] + a[2]).min(b[0] + b[2]);
    let y1 = (a[1] + a[3]).min(b[1] + b[3]);
    (x1 - x0).max(0.0) * (y1 - y0).max(0.0)
}

pub fn iou(a: [f64; 4], b: [f64; 4]) -> f64 {
    let inter = intersection(a, b);
    let union = area(a) + area(b) - inter;
    if union > 0.0 {
        inter / union
    } else {
        0.0
    }
}

fn matches(got: &Defect, got_bounds: Option<[f64; 4]>, want: &ExpectedDefect) -> bool {
    let Some(got_bounds) = got_bounds else {
        return false;
    };
    let kind_matches =
        got.kind.eq_ignore_ascii_case(&want.kind) || got.kind.eq_ignore_ascii_case("other");
    kind_matches && intersection(got_bounds, want.bounds) > 0.0
}

pub fn score(expected: &Expected, answer: Option<&Answer>, size: (u32, u32)) -> Score {
    let defects = answer
        .map(|answer| answer.defects.as_slice())
        .unwrap_or(&[]);
    if expected.expected == ExpectedKind::Clean {
        return Score {
            detected: true,
            iou: None,
            false_positives: defects.len(),
            matched: 0,
        };
    }
    let bounds: Vec<Option<[f64; 4]>> = defects
        .iter()
        .map(|defect| defect.bounds.map(|bounds| to_pixels(bounds, size)))
        .collect();
    let mut used = vec![false; defects.len()];
    let mut hits: Vec<Option<f64>> = Vec::new();
    for want in &expected.defect {
        let mut best: Option<(f64, usize)> = None;
        for (index, got) in defects.iter().enumerate() {
            if used[index] || !matches(got, bounds[index], want) {
                continue;
            }
            let value = bounds[index]
                .map(|got| iou(got, want.bounds))
                .unwrap_or(0.0);
            if best.is_none_or(|(best, _)| value > best) {
                best = Some((value, index));
            }
        }
        if let Some((value, index)) = best {
            used[index] = true;
            hits.push(Some(value));
        } else {
            hits.push(None);
        }
    }
    let matched = hits.iter().filter(|hit| hit.is_some()).count();
    let iou =
        (matched > 0).then(|| hits.iter().filter_map(|hit| *hit).sum::<f64>() / matched as f64);
    Score {
        detected: !hits.is_empty() && hits.iter().all(Option::is_some),
        iou,
        false_positives: defects.len() - used.iter().filter(|used| **used).count(),
        matched,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{Answer, Defect, Expected, ExpectedDefect, ExpectedKind, Severity};

    fn expected_defect(bounds: [f64; 4]) -> Expected {
        Expected {
            expected: ExpectedKind::Defect,
            defect: vec![ExpectedDefect {
                kind: "clipped".to_owned(),
                bounds,
                severity: Severity::Must,
            }],
        }
    }

    fn answer(kind: &str, bounds: Option<[f64; 4]>) -> Answer {
        Answer {
            defects: vec![Defect {
                kind: kind.to_owned(),
                bounds,
                confidence: 0.9,
                note: String::new(),
            }],
        }
    }

    #[test]
    fn the_whole_image_maps_to_its_pixels() {
        assert_eq!(
            to_pixels([0.0, 0.0, 1000.0, 1000.0], (452, 332)),
            [0.0, 0.0, 452.0, 332.0]
        );
    }

    #[test]
    fn identical_boxes_have_iou_one() {
        assert_eq!(iou([0.0, 0.0, 10.0, 10.0], [0.0, 0.0, 10.0, 10.0]), 1.0);
    }

    #[test]
    fn a_normalized_box_matches_the_expected_pixels() {
        // The expected box is 100,100..200,200 in a 1000x1000 image, so the
        // model's box is 100..200 in 0-1000.
        let expected = expected_defect([100.0, 100.0, 100.0, 100.0]);
        let got = answer("clipped", Some([100.0, 100.0, 200.0, 200.0]));
        let score = score(&expected, Some(&got), (1000, 1000));
        assert!(score.detected);
        assert_eq!(score.iou, Some(1.0));
        assert_eq!(score.false_positives, 0);
    }

    #[test]
    fn a_report_on_a_clean_case_is_a_false_positive() {
        let expected = Expected {
            expected: ExpectedKind::Clean,
            defect: Vec::new(),
        };
        let got = answer("clipped", Some([0.0, 0.0, 100.0, 100.0]));
        let score = score(&expected, Some(&got), (390, 844));
        assert_eq!(score.false_positives, 1);
    }

    #[test]
    fn a_report_without_a_box_is_not_a_match() {
        let expected = expected_defect([100.0, 100.0, 100.0, 100.0]);
        let got = answer("clipped", None);
        let score = score(&expected, Some(&got), (1000, 1000));
        assert!(!score.detected);
        assert_eq!(score.false_positives, 1);
    }

    #[test]
    fn the_wrong_kind_is_not_a_match() {
        let expected = expected_defect([100.0, 100.0, 100.0, 100.0]);
        let got = answer("spacing", Some([100.0, 100.0, 200.0, 200.0]));
        let score = score(&expected, Some(&got), (1000, 1000));
        assert!(!score.detected);
        assert_eq!(score.false_positives, 1);
    }
}
