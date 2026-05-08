//! Validated Firecracker microVM identifier.
//!
//! Firecracker's `--id` flag panics on any character outside `is_alphanumeric`
//! or `-`, and rejects anything outside the byte-length range 1..=64. The rule
//! is enforced by both the `firecracker` and `jailer` binaries via
//! `utils::validators::validate_instance_id`. Keeping validation in lock-step
//! with that rule here means callers can build a [`VmId`] once and never worry
//! about the child process aborting on startup.
//!
//! Two constructors are provided:
//!
//! - [`VmId::new`] — strict, returns [`VmIdError`] for anything the Firecracker
//!   validator would reject. Use this for identifiers that are already expected
//!   to be valid (CLI input, config files, other callers).
//! - [`VmId::from_sanitized`] — infallible projection. Non-alphanumeric,
//!   non-hyphen characters become `-`; the result is truncated to 64 bytes on
//!   a char boundary; an empty result falls back to `"vm"`. Deterministic, so
//!   the same input always produces the same identifier within one caller's
//!   namespace.
//!
//! ```
//! use fc_sdk::VmId;
//!
//! let strict = VmId::new("my-vm-01").unwrap();
//! assert_eq!(strict.as_ref(), "my-vm-01");
//!
//! let projected = VmId::from_sanitized("inst_019dbe46-6f74");
//! assert_eq!(projected.as_ref(), "inst-019dbe46-6f74");
//! ```
use std::fmt;

use thiserror::Error;

const MIN_LEN: usize = 1;
const MAX_LEN: usize = 64;
const SANITIZE_FALLBACK: &str = "vm";

/// A validated Firecracker microVM identifier.
///
/// Accepted strings match the same rule as Firecracker's
/// `utils::validators::validate_instance_id`: byte length in `1..=64` and every
/// `char` matches `c == '-' || c.is_alphanumeric()` (Unicode-aware).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VmId(String);

/// Reasons [`VmId::new`] rejects an input.
///
/// Error wording mirrors Firecracker's own `ValidatorError` so diagnostic
/// messages line up across the two layers.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum VmIdError {
    /// The identifier contained a character Firecracker does not accept.
    #[error("Invalid char ({c}) at position {position}")]
    InvalidChar {
        /// The offending character.
        c: char,
        /// Char index of the offending char in the original input. Matches the
        /// `position` reported by upstream's `validate_instance_id`, which uses
        /// `chars().enumerate()` rather than byte offsets.
        position: usize,
    },
    /// The identifier's byte length was outside the accepted range.
    #[error("Invalid len ({length}); the length must be between {min} and {max}")]
    InvalidLen {
        /// Observed byte length.
        length: usize,
        /// Minimum accepted byte length.
        min: usize,
        /// Maximum accepted byte length.
        max: usize,
    },
}

impl VmId {
    /// Build a [`VmId`] from an already-valid identifier.
    ///
    /// Returns [`VmIdError`] if the input would be rejected by Firecracker.
    pub fn new(input: impl AsRef<str>) -> Result<Self, VmIdError> {
        let input = input.as_ref();
        let length = input.len();
        if !(MIN_LEN..=MAX_LEN).contains(&length) {
            return Err(VmIdError::InvalidLen {
                length,
                min: MIN_LEN,
                max: MAX_LEN,
            });
        }
        for (position, c) in input.chars().enumerate() {
            if !is_valid_char(c) {
                return Err(VmIdError::InvalidChar { c, position });
            }
        }
        Ok(Self(input.to_owned()))
    }

    /// Project an arbitrary string into a valid [`VmId`] deterministically.
    ///
    /// Every non-(alphanumeric or `-`) character is replaced with `-`, the
    /// result is truncated to 64 bytes at the nearest char boundary, and an
    /// empty result is replaced with `"vm"`. The output is always accepted
    /// by [`VmId::new`].
    pub fn from_sanitized(input: impl AsRef<str>) -> Self {
        let sanitized: String = input
            .as_ref()
            .chars()
            .map(|c| if is_valid_char(c) { c } else { '-' })
            .collect();

        let truncated = truncate_to_bytes(&sanitized, MAX_LEN);
        let final_id = if truncated.is_empty() {
            SANITIZE_FALLBACK.to_owned()
        } else {
            truncated.to_owned()
        };

        debug_assert!(
            Self::new(&final_id).is_ok(),
            "from_sanitized produced invalid id: {final_id:?}"
        );
        Self(final_id)
    }

    /// Borrow the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for VmId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<VmId> for String {
    fn from(id: VmId) -> Self {
        id.0
    }
}

fn is_valid_char(c: char) -> bool {
    c == '-' || c.is_alphanumeric()
}

fn truncate_to_bytes(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        return input;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !input.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &input[..boundary]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_canonical_example_from_upstream_tests() {
        // Mirrors firecracker's own validate_instance_id happy-path assertion.
        let id = VmId::new("12-3aa").unwrap();
        assert_eq!(id.as_ref(), "12-3aa");
    }

    #[test]
    fn new_rejects_empty_input_as_invalid_len() {
        assert_eq!(
            VmId::new("").unwrap_err(),
            VmIdError::InvalidLen {
                length: 0,
                min: 1,
                max: 64,
            }
        );
    }

    #[test]
    fn new_accepts_exactly_sixty_four_ascii_chars() {
        let id = "a".repeat(64);
        VmId::new(&id).unwrap();
    }

    #[test]
    fn new_rejects_sixty_five_chars_as_invalid_len() {
        let id = "a".repeat(65);
        assert_eq!(
            VmId::new(&id).unwrap_err(),
            VmIdError::InvalidLen {
                length: 65,
                min: 1,
                max: 64,
            }
        );
    }

    #[test]
    fn new_rejects_underscore_with_position_matching_upstream() {
        assert_eq!(
            VmId::new("12_3aa").unwrap_err(),
            VmIdError::InvalidChar {
                c: '_',
                position: 2
            }
        );
    }

    #[test]
    fn new_rejects_colon_with_position_matching_upstream() {
        assert_eq!(
            VmId::new("12:3aa").unwrap_err(),
            VmIdError::InvalidChar {
                c: ':',
                position: 2
            }
        );
    }

    #[test]
    fn new_accepts_unicode_alphanumerics_like_firecracker_does() {
        // `is_alphanumeric` is Unicode-aware; firecracker's own validator
        // accepts these, so the SDK must too.
        VmId::new("漢字-1").unwrap();
    }

    #[test]
    fn new_reports_position_as_char_index_to_match_upstream() {
        // Upstream uses `chars().enumerate()`, so an underscore after two
        // multi-byte chars is reported at char index 2, not byte index 6.
        assert_eq!(
            VmId::new("漢字_1").unwrap_err(),
            VmIdError::InvalidChar {
                c: '_',
                position: 2
            }
        );
    }

    #[test]
    fn from_sanitized_replaces_every_rejected_character_with_hyphen() {
        assert_eq!(
            VmId::from_sanitized("inst_019dbe46-6f74-7130").as_ref(),
            "inst-019dbe46-6f74-7130"
        );
        assert_eq!(VmId::from_sanitized("a.b+c d").as_ref(), "a-b-c-d");
    }

    #[test]
    fn from_sanitized_falls_back_to_vm_when_input_is_empty() {
        assert_eq!(VmId::from_sanitized("").as_ref(), "vm");
    }

    #[test]
    fn from_sanitized_truncates_long_inputs_on_char_boundaries() {
        let multi_byte = "漢".repeat(30); // 90 bytes: 30 chars × 3 bytes.
        let id = VmId::from_sanitized(&multi_byte);
        assert!(id.as_ref().len() <= MAX_LEN);
        assert!(id.as_ref().is_char_boundary(id.as_ref().len()));
        VmId::new(id.as_ref()).expect("sanitized output must round-trip");
    }

    #[test]
    fn from_sanitized_output_always_passes_strict_new() {
        for input in [
            "",
            "_",
            "...",
            "inst_019dbe46-6f74-7130-8f72-dd8b45ebad7b",
            "has spaces and / slashes",
        ] {
            VmId::new(VmId::from_sanitized(input).as_ref())
                .unwrap_or_else(|e| panic!("sanitized {input:?} -> invalid: {e}"));
        }
    }
}
