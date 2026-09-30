//! Streaming parser for `git log` output.
//!
//! `git log` on a large repository (Linux, Chromium, anything with a few hundred
//! thousand commits) can produce hundreds of megabytes of text. Most tools that
//! want to walk that history read it into a `String` or a `Vec` first, which
//! means memory use grows with repo size instead of staying flat.
//!
//! This crate parses that output one commit at a time from anything that
//! implements `std::io::Read` (a pipe, a file, a `Vec<u8>` in tests) so peak
//! memory use is bounded by the size of the largest single commit record, not
//! by the size of the history.
//!
//! It does not run `git` itself. Feed it a reader; how that reader is produced
//! (spawning `git log`, reading a file someone else generated) is up to the
//! caller. See the README for the exact `--format` string to pass to `git log`.

mod commit;
mod parser;

pub use commit::{Commit, FileStat, StatCommit};
pub use parser::{CommitReader, GitLogError, StatReader, LOG_FORMAT};
