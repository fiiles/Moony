//! Device sync engine (phase 1). Task 1 only holds the SQLite probes and shared test helpers.

pub mod hlc;
pub mod registry;

#[cfg(test)]
mod probe;
#[cfg(test)]
pub(crate) mod test_support;
