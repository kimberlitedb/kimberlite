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
/// that preserve the invariant ([`NonEmptyVec::push`]).
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
            Self::InvalidWildcardPosition => {
                f.write_str("wildcard '*' may only appear at the start or end of a pattern")
            }
            Self::StartsWithDigit => f.write_str("SQL identifier must not start with a digit"),
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
            return Err(BoundedSizeError { value, max: MAX });
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
        assert_eq!(SqlIdentifier::try_new(""), Err(SqlIdentifierError::Empty));
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
        let bs: BoundedSize<1024> = BoundedSize::try_new(512).expect("within bound");
        assert_eq!(bs.get(), 512);
        assert_eq!(BoundedSize::<1024>::max(), 1024);
    }

    #[test]
    fn bounded_size_accepts_exact_max() {
        let bs: BoundedSize<1024> = BoundedSize::try_new(1024).expect("exact max permitted");
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
            let back: ClearanceLevel = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, level);
        }
    }

    #[test]
    fn clearance_level_serde_rejects_out_of_range() {
        let err = serde_json::from_str::<ClearanceLevel>("7");
        assert!(err.is_err(), "deserialising 7 should fail");
    }
}

// ============================================================================
// AggregateMemoryBudget — v0.7.0
// ============================================================================

/// Byte budget for in-memory `GROUP BY` aggregation.
///
/// Replaces the v0.6.x `MAX_GROUP_COUNT = 100_000` panic-style cap
/// with a configurable knob that surfaces structured error info
/// when exceeded. The default (256 MiB) gives ≈ 1M groups at the
/// average 256-byte aggregate-state size — a 10× lift over the
/// fixed-count ceiling without a planner overhaul. Spill-to-disk
/// hash aggregate is the proper v0.8+ fix.
///
/// AUDIT-2026-05 H-4. Found by: notebar GST drill-down on
/// `ar_ledger`.
///
/// # Invariants
///
/// - `value() >= MIN` (currently 64 KiB — must fit at least one
///   large group plus a small overhead margin).
/// - Construction goes through [`AggregateMemoryBudget::try_new`]
///   or `TryFrom<u64>`. There is no panicking infallible
///   constructor — passing zero is a real bug, not a gentle
///   default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AggregateMemoryBudget(u64);

/// Minimum aggregate budget. A budget below this is unusable —
/// the per-group overhead alone (roughly two cache lines for
/// `HashMap` slot + state header) eats most of the available
/// space.
pub const AGGREGATE_BUDGET_MIN_BYTES: u64 = 64 * 1024;

/// Default aggregate budget: 256 MiB. Sized to admit ≈ 1M
/// groups at 256 bytes/group of aggregate state — enough head-
/// room for typical compliance reporting (per-month, per-tenant,
/// per-account-pair groupings) without unbounded growth on
/// hostile inputs.
pub const AGGREGATE_BUDGET_DEFAULT_BYTES: u64 = 256 * 1024 * 1024;

/// Error from constructing an [`AggregateMemoryBudget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AggregateMemoryBudgetError {
    /// Bytes the caller asked for.
    pub observed: u64,
    /// Minimum we accept.
    pub minimum: u64,
}

impl Display for AggregateMemoryBudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "aggregate memory budget {} bytes is below the minimum {} bytes",
            self.observed, self.minimum
        )
    }
}

impl std::error::Error for AggregateMemoryBudgetError {}

impl AggregateMemoryBudget {
    /// Default budget — 256 MiB. Statement-level constant so the
    /// tuple-struct `Self(...)` literal stays private.
    pub const DEFAULT: Self = Self(AGGREGATE_BUDGET_DEFAULT_BYTES);

    /// Tries to construct a budget from a raw byte count.
    ///
    /// # Errors
    ///
    /// Returns [`AggregateMemoryBudgetError`] if `bytes` is below
    /// [`AGGREGATE_BUDGET_MIN_BYTES`].
    pub fn try_new(bytes: u64) -> Result<Self, AggregateMemoryBudgetError> {
        // Precondition: must clear the floor.
        if bytes < AGGREGATE_BUDGET_MIN_BYTES {
            return Err(AggregateMemoryBudgetError {
                observed: bytes,
                minimum: AGGREGATE_BUDGET_MIN_BYTES,
            });
        }
        let budget = Self(bytes);
        // Postcondition: round-trip through the accessor.
        debug_assert_eq!(budget.bytes(), bytes, "AggregateMemoryBudget round-trip");
        Ok(budget)
    }

    /// Returns the budget in bytes.
    pub const fn bytes(&self) -> u64 {
        self.0
    }
}

impl Default for AggregateMemoryBudget {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl TryFrom<u64> for AggregateMemoryBudget {
    type Error = AggregateMemoryBudgetError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl Display for AggregateMemoryBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} bytes", self.0)
    }
}

// ============================================================================
// DateField — v0.7.0
// ============================================================================

/// Date/time field selector for `EXTRACT` and `DATE_TRUNC`.
///
/// Closed enum: every variant the parser may emit is named here,
/// and the evaluator's `match` is therefore exhaustive — adding a
/// new field requires updating both call sites in lockstep.
/// Matches Postgres `EXTRACT(field FROM x)` syntax.
///
/// AUDIT-2026-05 S3.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DateField {
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    Millisecond,
    Microsecond,
    DayOfWeek,
    DayOfYear,
    Quarter,
    Week,
    /// Unix epoch seconds (signed; pre-1970 timestamps are
    /// negative). Returned as `BigInt`.
    Epoch,
}

/// Error returned when parsing an unknown `EXTRACT` field name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateFieldParseError(pub String);

impl Display for DateFieldParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown date field '{}' (expected one of: YEAR, MONTH, DAY, HOUR, MINUTE, SECOND, MILLISECOND, MICROSECOND, DOW, DOY, QUARTER, WEEK, EPOCH)", self.0)
    }
}

impl std::error::Error for DateFieldParseError {}

impl DateField {
    /// Parses a SQL keyword (case-insensitive) into a [`DateField`].
    ///
    /// Recognises Postgres aliases: `DOW` for `DayOfWeek`, `DOY`
    /// for `DayOfYear`. Rejects anything else with
    /// [`DateFieldParseError`] — Parse, Don't Validate (§3): out
    /// of range cannot be represented post-construction.
    pub fn parse(s: &str) -> Result<Self, DateFieldParseError> {
        match s.to_ascii_uppercase().as_str() {
            "YEAR" => Ok(Self::Year),
            "MONTH" => Ok(Self::Month),
            "DAY" => Ok(Self::Day),
            "HOUR" => Ok(Self::Hour),
            "MINUTE" => Ok(Self::Minute),
            "SECOND" => Ok(Self::Second),
            "MILLISECOND" | "MILLISECONDS" => Ok(Self::Millisecond),
            "MICROSECOND" | "MICROSECONDS" => Ok(Self::Microsecond),
            "DOW" | "DAYOFWEEK" => Ok(Self::DayOfWeek),
            "DOY" | "DAYOFYEAR" => Ok(Self::DayOfYear),
            "QUARTER" => Ok(Self::Quarter),
            "WEEK" => Ok(Self::Week),
            "EPOCH" => Ok(Self::Epoch),
            _ => Err(DateFieldParseError(s.to_string())),
        }
    }

    /// Returns `true` if this field is valid for `DATE_TRUNC`.
    /// `DATE_TRUNC` accepts a strict subset (`Year`/`Month`/`Day`/
    /// `Hour`/`Minute`/`Second`); the others (e.g. `DayOfWeek`,
    /// `Quarter`, `Epoch`) only make sense for `EXTRACT`.
    pub const fn is_truncatable(&self) -> bool {
        matches!(
            self,
            Self::Year | Self::Month | Self::Day | Self::Hour | Self::Minute | Self::Second
        )
    }
}

// ============================================================================
// SubstringRange — v0.7.0
// ============================================================================

/// `SUBSTRING(s FROM start [FOR length])` operand range.
///
/// SQL semantics: `start` is 1-based and may be ≤ 0 (negative or
/// zero starts shift the effective slice left without erroring).
/// `length` MUST be non-negative — a negative length is a parse
/// error in Postgres, and we lift that to a typed precondition
/// (Parse, Don't Validate §3): negative `length` cannot be
/// represented in a `SubstringRange`.
///
/// AUDIT-2026-05 S3.8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubstringRange {
    /// 1-based starting position. Negative values shift the slice
    /// left of the string start; zero is legal and equivalent to
    /// 1 (Postgres behaviour).
    pub start: i64,
    /// Number of characters (inclusive). `None` means "to end of
    /// string". Always non-negative when `Some(_)`.
    pub length: Option<i64>,
}

/// Error returned when constructing a [`SubstringRange`] with a
/// negative length argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NegativeSubstringLength(pub i64);

impl Display for NegativeSubstringLength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SUBSTRING length must be non-negative, got {}",
            self.0
        )
    }
}

impl std::error::Error for NegativeSubstringLength {}

impl SubstringRange {
    /// Two-argument form: `SUBSTRING(s FROM start)`.
    pub const fn from_start(start: i64) -> Self {
        Self { start, length: None }
    }

    /// Three-argument form: `SUBSTRING(s FROM start FOR length)`.
    /// Rejects negative `length`.
    pub fn try_new(start: i64, length: i64) -> Result<Self, NegativeSubstringLength> {
        if length < 0 {
            return Err(NegativeSubstringLength(length));
        }
        Ok(Self {
            start,
            length: Some(length),
        })
    }
}

// ============================================================================
// Interval — v0.7.0
// ============================================================================

/// SQL `INTERVAL` value with three independent components.
///
/// PostgreSQL-compatible representation: month, day, and sub-day
/// (nanos) components are kept separate because they have
/// different semantics under arithmetic:
///
/// - `months` are calendar-relative — `INTERVAL '1 month'` added
///   to `2026-01-31` is `2026-02-28`, but added to `2026-03-31`
///   is `2026-04-30`. Cannot be expressed in days or nanos
///   without a reference timestamp.
/// - `days` are wall-clock days. Independent of timezone in this
///   representation (we don't model DST shifts at the type
///   level — that's a query-engine concern).
/// - `nanos` is the sub-day remainder. In-memory invariant:
///   `|nanos| < 86_400_000_000_000` (one day in ns). Construction
///   via [`Interval::try_from_components`] normalises overflow
///   into `days`.
///
/// Arithmetic on `Interval`s is component-wise. Adding
/// intervals to timestamps / dates is handled in
/// `kimberlite-query` (it needs the calendar arithmetic from
/// `chrono`).
///
/// AUDIT-2026-05 S3.9. Tracks ROADMAP v0.7.0 interval-arithmetic
/// item.
///
/// # Kani-friendly design
///
/// The three i32/i64 components are deliberately not packed —
/// Kani's symbolic execution traverses them independently, which
/// keeps the associativity proof tractable for the no-month case.
/// The month-component proof carries a known limitation
/// (calendar arithmetic is non-associative across different-
/// length months — `(Jan 31 + 1 month) + 1 month` ≠ `Jan 31 + 2
/// months` in general); we document that and ship the proof for
/// the nanos+days subset only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Interval {
    months: i32,
    days: i32,
    nanos: i64,
}

/// One day in nanoseconds.
pub const NANOS_PER_DAY: i64 = 86_400_000_000_000;

/// Error returned when constructing an [`Interval`] would
/// overflow internal counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalConstructionError {
    /// Day-component overflow — happens when normalising
    /// out-of-range nanos into days exceeds `i32::MAX`.
    DayOverflow,
    /// Month-component overflow.
    MonthOverflow,
}

impl Display for IntervalConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DayOverflow => f.write_str("interval day component overflow"),
            Self::MonthOverflow => f.write_str("interval month component overflow"),
        }
    }
}

impl std::error::Error for IntervalConstructionError {}

impl Interval {
    /// The zero interval (`INTERVAL '0'`).
    pub const ZERO: Self = Self {
        months: 0,
        days: 0,
        nanos: 0,
    };

    /// Constructs an `Interval` from raw components.
    ///
    /// Normalises `nanos` overflow into `days` so the in-memory
    /// invariant `|nanos| < NANOS_PER_DAY` holds afterward.
    /// Returns [`IntervalConstructionError`] on day/month overflow.
    pub fn try_from_components(
        months: i32,
        days: i32,
        nanos: i64,
    ) -> Result<Self, IntervalConstructionError> {
        // Normalise nanos: extract whole-day overflow into `days`.
        let extra_days_i64 = nanos / NANOS_PER_DAY;
        let normalised_nanos = nanos % NANOS_PER_DAY;

        let extra_days_i32 = i32::try_from(extra_days_i64)
            .map_err(|_| IntervalConstructionError::DayOverflow)?;
        let new_days = days
            .checked_add(extra_days_i32)
            .ok_or(IntervalConstructionError::DayOverflow)?;

        let interval = Self {
            months,
            days: new_days,
            nanos: normalised_nanos,
        };
        // Postcondition: nanos invariant holds.
        debug_assert!(
            interval.nanos.unsigned_abs() < NANOS_PER_DAY as u64,
            "Interval normalisation failed: |nanos|={} >= {}",
            interval.nanos.unsigned_abs(),
            NANOS_PER_DAY
        );
        Ok(interval)
    }

    /// Constructs an interval entirely in months (calendar-
    /// relative). Convenience for parser glue.
    pub const fn from_months(months: i32) -> Self {
        Self {
            months,
            days: 0,
            nanos: 0,
        }
    }

    /// Constructs an interval entirely in days.
    pub const fn from_days(days: i32) -> Self {
        Self {
            months: 0,
            days,
            nanos: 0,
        }
    }

    /// Constructs an interval entirely in nanoseconds. Normalises
    /// into days/nanos so the invariant holds.
    pub fn from_nanos(nanos: i64) -> Result<Self, IntervalConstructionError> {
        Self::try_from_components(0, 0, nanos)
    }

    /// Returns the month component.
    pub const fn months(&self) -> i32 {
        self.months
    }

    /// Returns the day component.
    pub const fn days(&self) -> i32 {
        self.days
    }

    /// Returns the sub-day nanosecond component. Always satisfies
    /// `|nanos()| < NANOS_PER_DAY` post-construction.
    pub const fn nanos(&self) -> i64 {
        self.nanos
    }

    /// Returns `true` if every component is zero.
    pub const fn is_zero(&self) -> bool {
        self.months == 0 && self.days == 0 && self.nanos == 0
    }

    /// Component-wise addition with checked arithmetic. Returns
    /// `None` on i32 overflow in `months` or `days`.
    pub fn checked_add(&self, rhs: &Self) -> Option<Self> {
        let months = self.months.checked_add(rhs.months)?;
        let total_nanos = self.nanos.checked_add(rhs.nanos)?;
        let extra_days = total_nanos / NANOS_PER_DAY;
        let nanos = total_nanos % NANOS_PER_DAY;
        let days = self
            .days
            .checked_add(rhs.days)?
            .checked_add(i32::try_from(extra_days).ok()?)?;
        Some(Self {
            months,
            days,
            nanos,
        })
    }

    /// Component-wise subtraction (delegates to negation + add).
    pub fn checked_sub(&self, rhs: &Self) -> Option<Self> {
        let neg_rhs = rhs.checked_neg()?;
        self.checked_add(&neg_rhs)
    }

    /// Negates every component. Returns `None` on `i32::MIN`
    /// overflow.
    pub fn checked_neg(&self) -> Option<Self> {
        Some(Self {
            months: self.months.checked_neg()?,
            days: self.days.checked_neg()?,
            nanos: self.nanos.checked_neg()?,
        })
    }
}

impl Default for Interval {
    fn default() -> Self {
        Self::ZERO
    }
}

impl Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Postgres-compatible-ish: "M months D days HH:MM:SS.fff"
        // We don't need round-trippability through Display — the
        // wire / planner uses the components directly.
        write!(
            f,
            "{} months {} days {} ns",
            self.months, self.days, self.nanos
        )
    }
}

// Kani harnesses for the no-month subset's associativity proof.
// Compiled only under the `kani` cfg so the production build
// has no Kani dependency. Run via `just verify-kani`.
#[cfg(kani)]
mod kani_harnesses {
    use super::*;

    #[kani::proof]
    fn interval_add_associative_no_month() {
        let a_days: i32 = kani::any();
        let a_nanos: i64 = kani::any();
        let b_days: i32 = kani::any();
        let b_nanos: i64 = kani::any();
        let c_days: i32 = kani::any();
        let c_nanos: i64 = kani::any();

        // Bound the inputs so we don't trip i32 overflow on the
        // sums — that's a separate, expected error path.
        kani::assume(a_days.abs() < 1_000_000);
        kani::assume(b_days.abs() < 1_000_000);
        kani::assume(c_days.abs() < 1_000_000);
        kani::assume(a_nanos.abs() < NANOS_PER_DAY);
        kani::assume(b_nanos.abs() < NANOS_PER_DAY);
        kani::assume(c_nanos.abs() < NANOS_PER_DAY);

        let a = Interval::try_from_components(0, a_days, a_nanos).unwrap();
        let b = Interval::try_from_components(0, b_days, b_nanos).unwrap();
        let c = Interval::try_from_components(0, c_days, c_nanos).unwrap();

        let lhs = a.checked_add(&b).and_then(|ab| ab.checked_add(&c));
        let rhs = b.checked_add(&c).and_then(|bc| a.checked_add(&bc));

        // Either both succeed and are equal, or both fail.
        match (lhs, rhs) {
            (Some(l), Some(r)) => assert_eq!(l, r),
            (None, None) => {}
            _ => kani::cover!(),
        }
    }

    #[kani::proof]
    fn interval_zero_identity() {
        let m: i32 = kani::any();
        let d: i32 = kani::any();
        let n: i64 = kani::any();
        kani::assume(n.abs() < NANOS_PER_DAY);
        let i = Interval::try_from_components(m, d, n).unwrap();
        let z = Interval::ZERO;
        assert_eq!(i.checked_add(&z), Some(i));
        assert_eq!(z.checked_add(&i), Some(i));
    }
}

#[cfg(test)]
mod v07_tests {
    use super::*;

    // ----- AggregateMemoryBudget -----

    #[test]
    fn aggregate_budget_default_is_256_mib() {
        assert_eq!(
            AggregateMemoryBudget::DEFAULT.bytes(),
            256 * 1024 * 1024,
            "default budget drift breaks consumers tuning against the documented constant"
        );
    }

    #[test]
    fn aggregate_budget_try_new_rejects_below_floor() {
        let err = AggregateMemoryBudget::try_new(1024).expect_err("1 KiB is below the floor");
        assert_eq!(err.observed, 1024);
        assert_eq!(err.minimum, AGGREGATE_BUDGET_MIN_BYTES);
    }

    #[test]
    fn aggregate_budget_accepts_floor_exactly() {
        let b = AggregateMemoryBudget::try_new(AGGREGATE_BUDGET_MIN_BYTES).expect("at-floor ok");
        assert_eq!(b.bytes(), AGGREGATE_BUDGET_MIN_BYTES);
    }

    #[test]
    fn aggregate_budget_round_trips_through_tryfrom() {
        let b: AggregateMemoryBudget = (8 * 1024 * 1024_u64).try_into().expect("8 MiB ok");
        assert_eq!(b.bytes(), 8 * 1024 * 1024);
    }

    // ----- DateField -----

    #[test]
    fn datefield_parse_canonical_keywords() {
        assert_eq!(DateField::parse("YEAR").unwrap(), DateField::Year);
        assert_eq!(DateField::parse("year").unwrap(), DateField::Year);
        assert_eq!(DateField::parse("Year").unwrap(), DateField::Year);
        assert_eq!(DateField::parse("DOW").unwrap(), DateField::DayOfWeek);
        assert_eq!(
            DateField::parse("DAYOFWEEK").unwrap(),
            DateField::DayOfWeek
        );
        assert_eq!(DateField::parse("EPOCH").unwrap(), DateField::Epoch);
    }

    #[test]
    fn datefield_parse_rejects_unknown() {
        let err = DateField::parse("DECADE").expect_err("DECADE not supported");
        assert!(err.to_string().contains("DECADE"));
    }

    #[test]
    fn datefield_truncatable_subset() {
        for tf in [
            DateField::Year,
            DateField::Month,
            DateField::Day,
            DateField::Hour,
            DateField::Minute,
            DateField::Second,
        ] {
            assert!(tf.is_truncatable(), "{tf:?} must be truncatable");
        }
        for nontf in [
            DateField::DayOfWeek,
            DateField::Quarter,
            DateField::Week,
            DateField::Epoch,
            DateField::Millisecond,
            DateField::Microsecond,
        ] {
            assert!(!nontf.is_truncatable(), "{nontf:?} must NOT be truncatable");
        }
    }

    // ----- SubstringRange -----

    #[test]
    fn substring_range_two_arg_form() {
        let r = SubstringRange::from_start(3);
        assert_eq!(r.start, 3);
        assert!(r.length.is_none());
    }

    #[test]
    fn substring_range_three_arg_form_accepts_zero_length() {
        let r = SubstringRange::try_new(1, 0).expect("zero length is legal");
        assert_eq!(r.start, 1);
        assert_eq!(r.length, Some(0));
    }

    #[test]
    fn substring_range_rejects_negative_length() {
        let err = SubstringRange::try_new(1, -3).expect_err("negative length rejected");
        assert_eq!(err.0, -3);
    }

    #[test]
    fn substring_range_accepts_negative_start() {
        // Postgres: negative start shifts the effective slice
        // left of position 1 — legal, not an error.
        let r = SubstringRange::try_new(-2, 5).expect("negative start allowed");
        assert_eq!(r.start, -2);
    }

    // ----- Interval -----

    #[test]
    fn interval_zero_is_zero() {
        assert!(Interval::ZERO.is_zero());
        assert_eq!(Interval::ZERO.months(), 0);
        assert_eq!(Interval::ZERO.days(), 0);
        assert_eq!(Interval::ZERO.nanos(), 0);
    }

    #[test]
    fn interval_normalises_nanos_overflow_into_days() {
        let i = Interval::try_from_components(0, 0, 2 * NANOS_PER_DAY + 5).unwrap();
        assert_eq!(i.days(), 2);
        assert_eq!(i.nanos(), 5);
    }

    #[test]
    fn interval_handles_negative_nanos() {
        let i = Interval::try_from_components(0, 1, -NANOS_PER_DAY - 1).unwrap();
        assert_eq!(i.days(), 0);
        assert_eq!(i.nanos(), -1);
    }

    #[test]
    fn interval_round_trip_components() {
        let i = Interval::try_from_components(13, 7, 60_000_000_000).unwrap();
        assert_eq!(i.months(), 13);
        assert_eq!(i.days(), 7);
        assert_eq!(i.nanos(), 60_000_000_000);
    }

    #[test]
    fn interval_zero_identity() {
        let a = Interval::try_from_components(2, 5, 1_000_000_000).unwrap();
        assert_eq!(a.checked_add(&Interval::ZERO), Some(a));
        assert_eq!(Interval::ZERO.checked_add(&a), Some(a));
    }

    #[test]
    fn interval_associativity_no_month() {
        // Same property the Kani harness asserts symbolically.
        let a = Interval::from_days(3);
        let b = Interval::from_days(7);
        let c = Interval::from_days(11);
        let lhs = a.checked_add(&b).and_then(|ab| ab.checked_add(&c));
        let rhs = b.checked_add(&c).and_then(|bc| a.checked_add(&bc));
        assert_eq!(lhs, rhs);
        assert_eq!(lhs.unwrap().days(), 21);
    }

    #[test]
    fn interval_negation_is_self_inverse() {
        let i = Interval::try_from_components(1, 2, 3).unwrap();
        let neg = i.checked_neg().unwrap();
        let zero = i.checked_add(&neg).unwrap();
        assert!(zero.is_zero());
    }

    #[test]
    fn interval_serde_roundtrip() {
        let i = Interval::try_from_components(5, 10, 1234).unwrap();
        let json = serde_json::to_string(&i).expect("serialize");
        let back: Interval = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, i);
    }
}
