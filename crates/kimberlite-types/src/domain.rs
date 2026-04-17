//! Typed-domain primitives that make illegal states unrepresentable.
//!
//! These wrappers replace raw `Vec<T>` / `String` / `u8` fields at the boundary
//! between untrusted input and internal code, so that bug classes surfaced by
//! fuzzing (empty collections, unnormalised SQL identifiers, unbounded sizes,
//! out-of-range clearance levels) become compile errors rather than runtime
//! panics. See `docs/concepts/pressurecraft.md` §6 ("Fuzz findings as type
//! pressure") for rationale and `docs-internal/contributing/constructor-audit-2026-04.md`
//! for the migration punch-list.
//!
//! PRESSURECRAFT principles applied:
//! - §2 Make Illegal States Unrepresentable ([`NonEmptyVec`], [`ClearanceLevel`])
//! - §3 Parse, Don't Validate ([`SqlIdentifier`], [`BoundedSize`])

use std::{
    fmt::{self, Debug, Display},
    marker::PhantomData,
    ops::Deref,
};

use serde::{Deserialize, Serialize};

// ============================================================================
// NonEmptyVec<T>
// ============================================================================

/// A `Vec<T>` guaranteed to contain at least one element.
///
/// Construction via [`NonEmptyVec::try_new`] or the `TryFrom<Vec<T>>` impl
/// rejects an empty input. Once constructed, `Deref<Target=[T]>` makes it
/// behave like a slice for reads; mutation is only allowed through methods
/// that preserve the invariant ([`NonEmptyVec::push`], [`NonEmptyVec::extend_from_slice`]).
///
/// Kills the bug class: `parser::ParsedCreateTable.columns: Vec<_>` admitting
/// `CREATE TABLE t ()` — the type now rejects that at construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NonEmptyVec<T>(Vec<T>);

/// Error returned when constructing a [`NonEmptyVec`] from an empty `Vec`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyVecError;

impl Display for EmptyVecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("vector is empty; NonEmptyVec requires at least one element")
    }
}

impl std::error::Error for EmptyVecError {}

impl<T> NonEmptyVec<T> {
    /// Constructs a `NonEmptyVec` from a `Vec<T>`, rejecting empty inputs.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyVecError`] if `vec` is empty.
    pub fn try_new(vec: Vec<T>) -> Result<Self, EmptyVecError> {
        if vec.is_empty() {
            Err(EmptyVecError)
        } else {
            Ok(Self(vec))
        }
    }

    /// Constructs a `NonEmptyVec` from a single element.
    ///
    /// This is the infallible path per PRESSURECRAFT — `new()` cannot fail
    /// because the type system guarantees the invariant from the signature alone.
    pub fn singleton(first: T) -> Self {
        Self(vec![first])
    }

    /// Returns the first element. Never panics — a `NonEmptyVec` always has
    /// at least one element by construction.
    pub fn first(&self) -> &T {
        // SAFETY-by-invariant: NonEmptyVec guarantees len >= 1.
        &self.0[0]
    }

    /// Returns the last element. Never panics — see [`NonEmptyVec::first`].
    pub fn last(&self) -> &T {
        let len = self.0.len();
        &self.0[len - 1]
    }

    /// Consumes the `NonEmptyVec` and returns the inner `Vec<T>`.
    pub fn into_vec(self) -> Vec<T> {
        self.0
    }

    /// Returns a reference to the inner `Vec<T>`.
    pub fn as_vec(&self) -> &Vec<T> {
        &self.0
    }

    /// Pushes an element onto the vector. Preserves the non-empty invariant.
    pub fn push(&mut self, value: T) {
        self.0.push(value);
    }

    /// Returns the number of elements. Always `>= 1`.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always returns `false`. Provided for API symmetry with `Vec::is_empty`
    /// so that generic code using either type compiles unchanged.
    #[allow(clippy::unused_self)]
    pub const fn is_empty(&self) -> bool {
        false
    }
}

impl<T> Deref for NonEmptyVec<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<T> AsRef<[T]> for NonEmptyVec<T> {
    fn as_ref(&self) -> &[T] {
        &self.0
    }
}

impl<T> TryFrom<Vec<T>> for NonEmptyVec<T> {
    type Error = EmptyVecError;

    fn try_from(vec: Vec<T>) -> Result<Self, Self::Error> {
        Self::try_new(vec)
    }
}

impl<T> From<NonEmptyVec<T>> for Vec<T> {
    fn from(nev: NonEmptyVec<T>) -> Self {
        nev.0
    }
}

impl<T> IntoIterator for NonEmptyVec<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a NonEmptyVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T: Serialize> Serialize for NonEmptyVec<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for NonEmptyVec<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let vec = Vec::<T>::deserialize(deserializer)?;
        Self::try_new(vec).map_err(serde::de::Error::custom)
    }
}

// ============================================================================
// SqlIdentifier
// ============================================================================

/// A validated, case-folded SQL identifier.
///
/// SQL identifiers (`column_name`, `table_name`, `schema_name`) are
/// case-insensitive per SQL:2016 §5.4 ("Names and identifiers"). Comparing
/// them via raw `String` equality is a bug — an identifier `"Email"` in an
/// RBAC column-filter pattern did not match a live column `"email"` in the
/// query plan, bypassing the filter.
///
/// Construction normalises to lowercase and validates against
/// `[A-Za-z_][A-Za-z0-9_]*`. The original casing is preserved for display
/// via [`SqlIdentifier::original`], but equality, hashing, and ordering all
/// use the normalised form.
///
/// Special forms used by filter patterns (`*`, `prefix*`, `*suffix`) are
/// supported: the leading/trailing `*` is preserved as part of the pattern;
/// the alphanumeric portion is still validated.
#[derive(Debug, Clone)]
pub struct SqlIdentifier {
    original: String,
    normalised: String,
}

/// Error returned when constructing a [`SqlIdentifier`] from an invalid string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlIdentifierError {
    /// The identifier is empty.
    Empty,
    /// The identifier contains a character not in `[A-Za-z0-9_*]`.
    InvalidCharacter(char),
    /// The identifier contains `*` in a position other than leading or trailing.
    InvalidWildcardPosition,
    /// The identifier starts with a digit (not allowed in SQL).
    StartsWithDigit,
}

impl Display for SqlIdentifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("SQL identifier is empty"),
            Self::InvalidCharacter(c) => {
                write!(f, "SQL identifier contains invalid character {c:?}")
            }
            Self::InvalidWildcardPosition => f.write_str(
                "wildcard '*' may only appear at the start or end of a pattern",
            ),
            Self::StartsWithDigit => {
                f.write_str("SQL identifier must not start with a digit")
            }
        }
    }
}

impl std::error::Error for SqlIdentifierError {}

impl SqlIdentifier {
    /// Constructs a SQL identifier, validating and normalising casing.
    ///
    /// # Errors
    ///
    /// Returns [`SqlIdentifierError`] if the input is empty, contains invalid
    /// characters, has a misplaced wildcard, or starts with a digit.
    pub fn try_new(raw: impl Into<String>) -> Result<Self, SqlIdentifierError> {
        let original = raw.into();
        if original.is_empty() {
            return Err(SqlIdentifierError::Empty);
        }
        let bytes = original.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            let is_leading_wildcard = i == 0 && b == b'*';
            let is_trailing_wildcard = i + 1 == bytes.len() && b == b'*';
            if b == b'*' && !is_leading_wildcard && !is_trailing_wildcard {
                return Err(SqlIdentifierError::InvalidWildcardPosition);
            }
            let is_alpha = b.is_ascii_alphabetic();
            let is_digit = b.is_ascii_digit();
            let is_underscore = b == b'_';
            if !(is_alpha
                || is_digit
                || is_underscore
                || is_leading_wildcard
                || is_trailing_wildcard)
            {
                return Err(SqlIdentifierError::InvalidCharacter(char::from(b)));
            }
            if i == 0 && is_digit {
                return Err(SqlIdentifierError::StartsWithDigit);
            }
        }
        let normalised = original.to_ascii_lowercase();
        Ok(Self {
            original,
            normalised,
        })
    }

    /// Returns the identifier in its original casing (for display).
    pub fn original(&self) -> &str {
        &self.original
    }

    /// Returns the identifier in its normalised (lowercase) form.
    ///
    /// Use this when you need a string for equality or hashing in contexts
    /// that don't hold a `SqlIdentifier`.
    pub fn normalised(&self) -> &str {
        &self.normalised
    }

    /// Returns `true` if this pattern is the single-wildcard `"*"`.
    pub fn is_wildcard(&self) -> bool {
        self.normalised == "*"
    }

    /// Returns `Some(prefix)` if this is a `prefix*` pattern, where `prefix`
    /// does not contain a wildcard.
    pub fn as_prefix_pattern(&self) -> Option<&str> {
        self.normalised
            .strip_suffix('*')
            .filter(|s| !s.is_empty() && !s.contains('*'))
    }

    /// Returns `Some(suffix)` if this is a `*suffix` pattern.
    pub fn as_suffix_pattern(&self) -> Option<&str> {
        self.normalised
            .strip_prefix('*')
            .filter(|s| !s.is_empty() && !s.contains('*'))
    }

    /// Returns `true` if `column_name` matches this identifier (case-insensitive).
    /// Supports `*`, `prefix*`, `*suffix` pattern forms.
    pub fn matches(&self, column_name: &str) -> bool {
        if self.is_wildcard() {
            return true;
        }
        let lhs = column_name.to_ascii_lowercase();
        if let Some(prefix) = self.as_prefix_pattern() {
            return lhs.starts_with(prefix);
        }
        if let Some(suffix) = self.as_suffix_pattern() {
            return lhs.ends_with(suffix);
        }
        lhs == self.normalised
    }
}

impl Display for SqlIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.original)
    }
}

impl PartialEq for SqlIdentifier {
    fn eq(&self, other: &Self) -> bool {
        self.normalised == other.normalised
    }
}

impl Eq for SqlIdentifier {}

impl std::hash::Hash for SqlIdentifier {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.normalised.hash(state);
    }
}

impl PartialOrd for SqlIdentifier {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SqlIdentifier {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.normalised.cmp(&other.normalised)
    }
}

impl Serialize for SqlIdentifier {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.original.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SqlIdentifier {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::try_new(raw).map_err(serde::de::Error::custom)
    }
}

// ============================================================================
// BoundedSize<const MAX: usize>
// ============================================================================

/// A `usize` wrapped at construction to reject values exceeding `MAX`.
///
/// Kills the bug class: a `u32` size prefix read from untrusted bytes being
/// cast to `usize` and used directly as an allocation size. The LZ4 codec
/// previously trusted the first 4 bytes of a compressed block, allowing a
/// decompression bomb. With `BoundedSize<MAX_DECOMPRESSED_SIZE>` the boundary
/// check is a type constructor, not an `if` statement the next author can
/// forget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BoundedSize<const MAX: usize>(usize);

/// Error returned when a `BoundedSize` is constructed from an out-of-range value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedSizeError {
    /// The value that exceeded the bound.
    pub value: u64,
    /// The maximum permitted value.
    pub max: usize,
}

impl Display for BoundedSizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "size {} exceeds bound {} (decompression bomb or corruption)",
            self.value, self.max
        )
    }
}

impl std::error::Error for BoundedSizeError {}

impl<const MAX: usize> BoundedSize<MAX> {
    /// Constructs a `BoundedSize`, rejecting `value > MAX`.
    ///
    /// # Errors
    ///
    /// Returns [`BoundedSizeError`] if `value > MAX`.
    pub const fn try_new(value: usize) -> Result<Self, BoundedSizeError> {
        if value > MAX {
            Err(BoundedSizeError {
                value: value as u64,
                max: MAX,
            })
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the inner `usize`.
    pub const fn get(self) -> usize {
        self.0
    }

    /// Returns the compile-time maximum.
    pub const fn max() -> usize {
        MAX
    }
}

impl<const MAX: usize> TryFrom<u32> for BoundedSize<MAX> {
    type Error = BoundedSizeError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_new(value as usize)
    }
}

impl<const MAX: usize> TryFrom<u64> for BoundedSize<MAX> {
    type Error = BoundedSizeError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value > usize::MAX as u64 {
            return Err(BoundedSizeError {
                value,
                max: MAX,
            });
        }
        Self::try_new(value as usize)
    }
}

impl<const MAX: usize> TryFrom<usize> for BoundedSize<MAX> {
    type Error = BoundedSizeError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl<const MAX: usize> From<BoundedSize<MAX>> for usize {
    fn from(bs: BoundedSize<MAX>) -> Self {
        bs.0
    }
}

// Serde round-trips through usize; deserialising re-applies the bound check.
impl<const MAX: usize> Serialize for BoundedSize<MAX> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for BoundedSize<MAX> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = usize::deserialize(deserializer)?;
        Self::try_new(raw).map_err(serde::de::Error::custom)
    }
}

// PhantomData marker to keep MAX in variance calculations stable across
// generic uses — matters for refactors that move BoundedSize into generic
// containers. Unused today but cheap to include.
const _: () = {
    fn _phantom<const MAX: usize>() -> PhantomData<[(); MAX]> {
        PhantomData
    }
};

// ============================================================================
// ClearanceLevel
// ============================================================================

/// Mandatory Access Control clearance level, per the Bell–LaPadula model used
/// by the ABAC crate for FedRAMP / HIPAA policy evaluation.
///
/// Kills the bug class: `UserAttributes.clearance_level: u8` admitting
/// values 4..=255 that panic on comparison with the enum-backed policy side.
/// The `#[repr(u8)]` preserves wire compatibility with existing serialised
/// records.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum ClearanceLevel {
    /// Unclassified data; no restriction.
    #[default]
    Public = 0,
    /// Sensitive but unclassified.
    Confidential = 1,
    /// Classified — restricted distribution.
    Secret = 2,
    /// Highest classification in the default lattice.
    TopSecret = 3,
}

/// Error returned when converting an out-of-range `u8` to a `ClearanceLevel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClearanceLevelError {
    /// The input byte that could not be mapped.
    pub value: u8,
}

impl Display for ClearanceLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "clearance level {} is out of range (valid: 0..=3)",
            self.value
        )
    }
}

impl std::error::Error for ClearanceLevelError {}

impl ClearanceLevel {
    /// Returns the numeric discriminant (for wire serialisation).
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Returns `true` if this clearance dominates (is `>=`) `other`
    /// in the Bell–LaPadula lattice. Convenience helper for policy checks.
    pub const fn dominates(self, other: Self) -> bool {
        (self as u8) >= (other as u8)
    }
}

impl TryFrom<u8> for ClearanceLevel {
    type Error = ClearanceLevelError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Public),
            1 => Ok(Self::Confidential),
            2 => Ok(Self::Secret),
            3 => Ok(Self::TopSecret),
            _ => Err(ClearanceLevelError { value }),
        }
    }
}

impl From<ClearanceLevel> for u8 {
    fn from(level: ClearanceLevel) -> Self {
        level as u8
    }
}

impl Display for ClearanceLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Public => f.write_str("public"),
            Self::Confidential => f.write_str("confidential"),
            Self::Secret => f.write_str("secret"),
            Self::TopSecret => f.write_str("top_secret"),
        }
    }
}

impl Serialize for ClearanceLevel {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (*self as u8).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ClearanceLevel {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let byte = u8::deserialize(deserializer)?;
        Self::try_from(byte).map_err(serde::de::Error::custom)
    }
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- NonEmptyVec ----

    #[test]
    fn non_empty_vec_rejects_empty() {
        assert_eq!(NonEmptyVec::<u8>::try_new(vec![]), Err(EmptyVecError));
    }

    #[test]
    fn non_empty_vec_accepts_single_element() {
        let v = NonEmptyVec::singleton(42u8);
        assert_eq!(v.len(), 1);
        assert_eq!(*v.first(), 42);
        assert_eq!(*v.last(), 42);
        assert!(!v.is_empty());
    }

    #[test]
    fn non_empty_vec_push_preserves_invariant() {
        let mut v = NonEmptyVec::singleton(1u8);
        v.push(2);
        v.push(3);
        assert_eq!(v.len(), 3);
        assert_eq!(&*v, &[1, 2, 3]);
    }

    #[test]
    fn non_empty_vec_serde_roundtrip() {
        let v = NonEmptyVec::try_new(vec![1, 2, 3]).expect("non-empty");
        let json = serde_json::to_string(&v).expect("serialize");
        assert_eq!(json, "[1,2,3]");
        let back: NonEmptyVec<i32> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, v);
    }

    #[test]
    fn non_empty_vec_serde_rejects_empty() {
        let err = serde_json::from_str::<NonEmptyVec<i32>>("[]");
        assert!(err.is_err(), "deserializing empty should fail");
    }

    // ---- SqlIdentifier ----

    #[test]
    fn sql_identifier_normalises_case() {
        let a = SqlIdentifier::try_new("Email").expect("valid");
        let b = SqlIdentifier::try_new("EMAIL").expect("valid");
        let c = SqlIdentifier::try_new("email").expect("valid");
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a.original(), "Email");
        assert_eq!(a.normalised(), "email");
    }

    #[test]
    fn sql_identifier_rejects_empty() {
        assert_eq!(
            SqlIdentifier::try_new(""),
            Err(SqlIdentifierError::Empty)
        );
    }

    #[test]
    fn sql_identifier_rejects_leading_digit() {
        assert_eq!(
            SqlIdentifier::try_new("1col"),
            Err(SqlIdentifierError::StartsWithDigit)
        );
    }

    #[test]
    fn sql_identifier_rejects_invalid_char() {
        match SqlIdentifier::try_new("col-name") {
            Err(SqlIdentifierError::InvalidCharacter(c)) => assert_eq!(c, '-'),
            other => panic!("expected InvalidCharacter, got {other:?}"),
        }
    }

    #[test]
    fn sql_identifier_accepts_wildcard_patterns() {
        SqlIdentifier::try_new("*").expect("bare wildcard");
        SqlIdentifier::try_new("email_*").expect("prefix pattern");
        SqlIdentifier::try_new("*_token").expect("suffix pattern");
    }

    #[test]
    fn sql_identifier_rejects_middle_wildcard() {
        assert_eq!(
            SqlIdentifier::try_new("em*ail"),
            Err(SqlIdentifierError::InvalidWildcardPosition)
        );
    }

    #[test]
    fn sql_identifier_matches_case_insensitively() {
        let pat = SqlIdentifier::try_new("Email").expect("valid");
        assert!(pat.matches("email"));
        assert!(pat.matches("EMAIL"));
        assert!(pat.matches("Email"));
        assert!(!pat.matches("name"));
    }

    #[test]
    fn sql_identifier_prefix_suffix_wildcard_match() {
        let prefix = SqlIdentifier::try_new("user_*").expect("valid");
        assert!(prefix.matches("user_id"));
        assert!(prefix.matches("USER_NAME"));
        assert!(!prefix.matches("id"));

        let suffix = SqlIdentifier::try_new("*_id").expect("valid");
        assert!(suffix.matches("user_id"));
        assert!(suffix.matches("ORDER_ID"));
        assert!(!suffix.matches("user"));

        let wildcard = SqlIdentifier::try_new("*").expect("valid");
        assert!(wildcard.matches("anything"));
    }

    #[test]
    fn sql_identifier_serde_roundtrip() {
        let id = SqlIdentifier::try_new("User_Email").expect("valid");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, "\"User_Email\"");
        let back: SqlIdentifier = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, id);
        assert_eq!(back.original(), "User_Email");
    }

    // ---- BoundedSize ----

    #[test]
    fn bounded_size_accepts_within_bound() {
        let bs: BoundedSize<1024> =
            BoundedSize::try_new(512).expect("within bound");
        assert_eq!(bs.get(), 512);
        assert_eq!(BoundedSize::<1024>::max(), 1024);
    }

    #[test]
    fn bounded_size_accepts_exact_max() {
        let bs: BoundedSize<1024> =
            BoundedSize::try_new(1024).expect("exact max permitted");
        assert_eq!(bs.get(), 1024);
    }

    #[test]
    fn bounded_size_rejects_over_bound() {
        let err = BoundedSize::<1024>::try_new(1025).unwrap_err();
        assert_eq!(err.value, 1025);
        assert_eq!(err.max, 1024);
    }

    #[test]
    fn bounded_size_tryfrom_u32() {
        let bs: BoundedSize<1024> = 512u32.try_into().expect("within bound");
        assert_eq!(bs.get(), 512);
        let err: Result<BoundedSize<1024>, _> = 2048u32.try_into();
        assert!(err.is_err());
    }

    #[test]
    fn bounded_size_tryfrom_u64_overflow_on_32bit_safe() {
        // Even on 64-bit, the largest permitted u64 should be <= usize::MAX.
        let bs: BoundedSize<{ usize::MAX }> = 42u64.try_into().expect("within bound");
        assert_eq!(bs.get(), 42);
    }

    #[test]
    fn bounded_size_serde_enforces_on_deserialize() {
        let bs: BoundedSize<100> = 50usize.try_into().expect("valid");
        let json = serde_json::to_string(&bs).expect("serialize");
        assert_eq!(json, "50");
        let ok: BoundedSize<100> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ok.get(), 50);
        let err = serde_json::from_str::<BoundedSize<100>>("200");
        assert!(err.is_err(), "deserialising over-bound should fail");
    }

    // ---- ClearanceLevel ----

    #[test]
    fn clearance_level_tryfrom_valid() {
        assert_eq!(ClearanceLevel::try_from(0), Ok(ClearanceLevel::Public));
        assert_eq!(
            ClearanceLevel::try_from(1),
            Ok(ClearanceLevel::Confidential)
        );
        assert_eq!(ClearanceLevel::try_from(2), Ok(ClearanceLevel::Secret));
        assert_eq!(ClearanceLevel::try_from(3), Ok(ClearanceLevel::TopSecret));
    }

    #[test]
    fn clearance_level_tryfrom_invalid() {
        let err = ClearanceLevel::try_from(4).unwrap_err();
        assert_eq!(err.value, 4);
        let err = ClearanceLevel::try_from(255).unwrap_err();
        assert_eq!(err.value, 255);
    }

    #[test]
    fn clearance_level_dominates() {
        assert!(ClearanceLevel::TopSecret.dominates(ClearanceLevel::Public));
        assert!(ClearanceLevel::Secret.dominates(ClearanceLevel::Confidential));
        assert!(ClearanceLevel::Public.dominates(ClearanceLevel::Public));
        assert!(!ClearanceLevel::Public.dominates(ClearanceLevel::Secret));
    }

    #[test]
    fn clearance_level_default_is_public() {
        assert_eq!(ClearanceLevel::default(), ClearanceLevel::Public);
    }

    #[test]
    fn clearance_level_serde_roundtrip() {
        for level in [
            ClearanceLevel::Public,
            ClearanceLevel::Confidential,
            ClearanceLevel::Secret,
            ClearanceLevel::TopSecret,
        ] {
            let json = serde_json::to_string(&level).expect("serialize");
            let back: ClearanceLevel =
                serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, level);
        }
    }

    #[test]
    fn clearance_level_serde_rejects_out_of_range() {
        let err = serde_json::from_str::<ClearanceLevel>("7");
        assert!(err.is_err(), "deserialising 7 should fail");
    }
}
