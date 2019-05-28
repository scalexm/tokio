//! Spans represent periods of time in the execution of a program.

use {field, Metadata};

/// Identifies a span within the context of a subscriber.
///
/// They are generated by [`Subscriber`]s for each span as it is created, by
/// the [`new_span`] trait method. See the documentation for that method for
/// more information on span ID generation.
///
/// [`Subscriber`]: ../subscriber/trait.Subscriber.html
/// [`new_span`]: ../subscriber/trait.Subscriber.html#method.new_span
#[derive(Debug, PartialEq, Eq, Hash)]
// TODO(eliza): when Tokio's minimum Rust version is >= 1.28, change the
// internal representation to a `NonZeroU64`.
pub struct Id(u64);

/// Attributes provided to a `Subscriber` describing a new span when it is
/// created.
#[derive(Debug)]
pub struct Attributes<'a> {
    metadata: &'a Metadata<'a>,
    values: &'a field::ValueSet<'a>,
    parent: Parent,
}

/// A set of fields recorded by a span.
#[derive(Debug)]
pub struct Record<'a> {
    values: &'a field::ValueSet<'a>,
}

#[derive(Debug)]
enum Parent {
    /// The new span will be a root span.
    Root,
    /// The new span will be rooted in the current span.
    Current,
    /// The new span has an explicitly-specified parent.
    Explicit(Id),
}

// ===== impl Span =====

impl Id {
    /// Constructs a new span ID from the given `u64`.
    ///
    /// **Note**: Span IDs must be greater than zero.
    ///
    /// # Panics
    /// - If the provided `u64` is 0
    pub fn from_u64(u: u64) -> Self {
        assert!(u > 0);
        Id(u)
    }

    /// Returns the span's ID as a  `u64`.
    pub fn into_u64(&self) -> u64 {
        self.0
    }
}

#[deprecated(
    since = "0.2.1",
    note = "cloning span IDs can cause incorrect reference counts, use \
            `Subscriber::clone_span` instead"
)]
#[doc(hidden)]
impl Clone for Id {
    fn clone(&self) -> Self {
        panic!(
            "cloning span IDs can cause incorrect reference counts, use \
             `Subscriber::clone_span` instead"
        )
    }
}

// ===== impl Attributes =====

impl<'a> Attributes<'a> {
    /// Returns `Attributes` describing a new child span of the current span,
    /// with the provided metadata and values.
    pub fn new(metadata: &'a Metadata<'a>, values: &'a field::ValueSet<'a>) -> Self {
        Attributes {
            metadata,
            values,
            parent: Parent::Current,
        }
    }

    /// Returns `Attributes` describing a new span at the root of its own trace
    /// tree, with the provided metadata and values.
    pub fn new_root(metadata: &'a Metadata<'a>, values: &'a field::ValueSet<'a>) -> Self {
        Attributes {
            metadata,
            values,
            parent: Parent::Root,
        }
    }

    /// Returns `Attributes` describing a new child span of the specified
    /// parent span, with the provided metadata and values.
    pub fn child_of(
        parent: Id,
        metadata: &'a Metadata<'a>,
        values: &'a field::ValueSet<'a>,
    ) -> Self {
        Attributes {
            metadata,
            values,
            parent: Parent::Explicit(parent),
        }
    }

    /// Returns a reference to the new span's metadata.
    pub fn metadata(&self) -> &Metadata<'a> {
        self.metadata
    }

    /// Returns a reference to a `ValueSet` containing any values the new span
    /// was created with.
    pub fn values(&self) -> &field::ValueSet<'a> {
        self.values
    }

    /// Returns true if the new span shoold be a root.
    pub fn is_root(&self) -> bool {
        match self.parent {
            Parent::Root => true,
            _ => false,
        }
    }

    /// Returns true if the new span's parent should be determined based on the
    /// current context.
    ///
    /// If this is true and the current thread is currently inside a span, then
    /// that span should be the new span's parent. Otherwise, if the current
    /// thread is _not_ inside a span, then the new span will be the root of its
    /// own trace tree.
    pub fn is_contextual(&self) -> bool {
        match self.parent {
            Parent::Current => true,
            _ => false,
        }
    }

    /// Returns a reference to the new span's explicitly-specified parent, if
    /// there is one.
    ///
    /// Otherwise (if the new span is a root or is a child of the current span),
    /// returns none.
    pub fn parent(&self) -> Option<&Id> {
        match self.parent {
            Parent::Explicit(ref p) => Some(p),
            _ => None,
        }
    }

    /// Returns the new span's explicitly-specified parent, if there is one.
    ///
    /// Otherwise (if the new span is a root or is a child of the current span),
    /// returns false.
    pub fn take_parent(self) -> Option<Id> {
        match self.parent {
            Parent::Explicit(p) => Some(p),
            _ => None,
        }
    }

    /// Records all the fields in this set of `Attributes` with the provided
    /// [Visitor].
    ///
    /// [visitor]: ../field/trait.Visit.html
    pub fn record(&self, visitor: &mut field::Visit) {
        self.values.record(visitor)
    }

    /// Returns `true` if this set of `Attributes` contains a value for the
    /// given `Field`.
    pub fn contains(&self, field: &field::Field) -> bool {
        self.values.contains(field)
    }

    /// Returns true if this set of `Attributes` contains _no_ values.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

// ===== impl Record =====

impl<'a> Record<'a> {
    /// Constructs a new `Record` from a `ValueSet`.
    pub fn new(values: &'a field::ValueSet<'a>) -> Self {
        Self { values }
    }

    /// Records all the fields in this `Record` with the provided [Visitor].
    ///
    /// [visitor]: ../field/trait.Visit.html
    pub fn record(&self, visitor: &mut field::Visit) {
        self.values.record(visitor)
    }

    /// Returns `true` if this `Record` contains a value for the given `Field`.
    pub fn contains(&self, field: &field::Field) -> bool {
        self.values.contains(field)
    }

    /// Returns true if this `Record` contains _no_ values.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}
