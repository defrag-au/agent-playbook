//! Labels a run carries beyond its measurements: why it is excluded, or what it inspected.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunTag {
    /// A tool output carried the expected answer; the run is excluded from the comparison.
    Contaminated,
    /// A tool named the sealed corpus; the read was refused, so the run is kept.
    InspectedCorpus,
    /// The launcher was killed at the timeout.
    TimedOut,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_serializes_as_a_kebab_case_name() {
        assert_eq!(
            serde_json::to_string(&RunTag::InspectedCorpus).expect("a tag"),
            "\"inspected-corpus\""
        );
    }
}
