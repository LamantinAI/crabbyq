/// Common boxed error type used across the framework.
pub type CrabbyError = Box<dyn std::error::Error + Send + Sync>;

/// Convenient result alias for handlers and applications that use the default
/// framework error type.
pub type CrabbyResult<T> = Result<T, CrabbyError>;
