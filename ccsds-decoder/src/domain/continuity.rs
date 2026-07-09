// ── In-Memory Sequence Continuity Tracker ────────────────────────────────────
//
// Tracks the expected next sequence count per APID.
// CCSDS sequence counts are 14-bit, wrapping at 16383 (0x3FFF).
//
// Why in-memory only?
//   The CCSDS Decoder Service is deliberately stateless at the persistence
//   layer. If the service restarts, continuity tracking resets. This is
//   acceptable for telemetry — a gap is logged, not a system failure.

use crate::domain::ccsds_hdr::SequenceAnalysis;
use std::collections::HashMap;

const SEQ_MAX: u16 = 0x3FFF; // 14-bit wrap boundary = 16383

pub struct ContinuityEngine {
    /// Maps APID → the next expected sequence count.
    state: HashMap<u16, u16>,
}

impl ContinuityEngine {
    pub fn new() -> Self {
        Self {
            state: HashMap::new(),
        }
    }

    /// Inspect a received sequence count for a given APID.
    ///
    /// First-ever packet for an APID: recorded, no gap flagged.
    /// Subsequent packets: compared against the expected count.
    ///
    /// Returns a `SequenceAnalysis` describing whether the packet is a
    /// duplicate, gap, or clean continuation.
    pub fn check(&mut self, apid: u16, received: u16) -> SequenceAnalysis {
        // Check if this is the first packet for this APID *before* taking a
        // mutable borrow via entry(). This keeps the borrow checker happy.
        let is_first = !self.state.contains_key(&apid);

        if is_first {
            // Bootstrap: record the first sequence count as the base.
            self.state.insert(apid, (received + 1) & SEQ_MAX);
            return SequenceAnalysis {
                expected: received,
                is_duplicate: false,
                is_gap: false,
            };
        }

        let expected = *self.state.get(&apid).unwrap();

        let is_duplicate = received == expected.wrapping_sub(1) & SEQ_MAX
            || (expected == 0 && received == SEQ_MAX);

        let is_gap = received != expected;

        // Advance the tracker only if the packet is not a duplicate.
        if !is_duplicate {
            self.state.insert(apid, (received + 1) & SEQ_MAX);
        }

        SequenceAnalysis {
            expected,
            is_duplicate,
            is_gap: is_gap && !is_duplicate,
        }
    }

    /// Reset the tracker for a specific APID (e.g. on session reconnect).
    pub fn reset_apid(&mut self, apid: u16) {
        self.state.remove(&apid);
    }

    /// Clear all tracked APIDs.
    pub fn reset_all(&mut self) {
        self.state.clear();
    }
}

impl Default for ContinuityEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_packet_is_clean() {
        let mut engine = ContinuityEngine::new();
        let result = engine.check(42, 0);
        assert!(!result.is_duplicate);
        assert!(!result.is_gap);
    }

    #[test]
    fn test_sequential_packets_no_gap() {
        let mut engine = ContinuityEngine::new();
        engine.check(42, 100);
        let result = engine.check(42, 101);
        assert!(!result.is_gap);
        assert!(!result.is_duplicate);
    }

    #[test]
    fn test_gap_detected() {
        let mut engine = ContinuityEngine::new();
        engine.check(42, 100); // establishes expected = 101
        let result = engine.check(42, 103); // gap: skipped 101 and 102
        assert!(result.is_gap);
        assert!(!result.is_duplicate);
    }

    #[test]
    fn test_duplicate_detected() {
        let mut engine = ContinuityEngine::new();
        engine.check(42, 100); // expected becomes 101
        let result = engine.check(42, 100); // same as previous — duplicate
        assert!(result.is_duplicate);
        assert!(!result.is_gap);
    }

    #[test]
    fn test_sequence_wrap_at_14bit_max() {
        let mut engine = ContinuityEngine::new();
        engine.check(42, SEQ_MAX); // expected becomes 0
        let result = engine.check(42, 0); // clean wrap
        assert!(!result.is_gap);
        assert!(!result.is_duplicate);
    }

    #[test]
    fn test_independent_apids_do_not_interfere() {
        let mut engine = ContinuityEngine::new();
        engine.check(10, 500);
        engine.check(20, 200);
        let r10 = engine.check(10, 501);
        let r20 = engine.check(20, 201);
        assert!(!r10.is_gap);
        assert!(!r20.is_gap);
    }

    #[test]
    fn test_reset_apid_clears_state() {
        let mut engine = ContinuityEngine::new();
        engine.check(42, 100);
        engine.reset_apid(42);
        // After reset, next packet is treated as first → no gap
        let result = engine.check(42, 500);
        assert!(!result.is_gap);
        assert!(!result.is_duplicate);
    }

    #[test]
    fn test_expected_value_returned_correctly() {
        let mut engine = ContinuityEngine::new();
        engine.check(42, 100); // expected becomes 101
        let result = engine.check(42, 105); // gap
        assert_eq!(result.expected, 101);
    }
}
