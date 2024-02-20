/// USB Error type
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ErrorKind {
    Timeout,
}

impl core::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&self, f)
    }
}

#[cfg(feature = "nightly")]
impl core::error::Error for ErrorKind {
    #[allow(deprecated)]
    fn description(&self) -> &str {
        use ErrorKind::*;
        match self {
            Timeout => "Blocking operation timed-out",
        }
    }
}
