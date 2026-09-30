use std::error::Error;
use std::fmt;
use std::io::{self, BufRead, BufReader, Read};

use crate::commit::{Commit, FileStat, StatCommit};

/// The `git log --format=` string this crate expects as input.
///
/// `%x1e` (ASCII record separator) marks the start of each commit so records
/// can be split without depending on newlines inside commit subjects. `%x1f`
/// (ASCII unit separator) separates the fields within a record. Neither byte
/// can legally appear in the fields git fills in here, so no escaping is
/// needed. `%P` is the space-separated list of parent hashes; it's empty for
/// a root commit and has two or more entries for a merge.
pub const LOG_FORMAT: &str = "%x1e%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%s";

const RECORD_SEP: u8 = 0x1e;
const FIELD_SEP: u8 = 0x1f;
const FIELD_COUNT: usize = 6;

/// Errors that can occur while reading or parsing a commit stream.
#[derive(Debug)]
pub enum GitLogError {
    Io(io::Error),
    /// A record didn't split into the expected number of fields, or its
    /// bytes weren't valid UTF-8. Carries the raw record for debugging.
    Malformed(String),
}

impl fmt::Display for GitLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitLogError::Io(e) => write!(f, "io error reading commit stream: {e}"),
            GitLogError::Malformed(record) => {
                write!(f, "malformed commit record: {record:?}")
            }
        }
    }
}

impl Error for GitLogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            GitLogError::Io(e) => Some(e),
            GitLogError::Malformed(_) => None,
        }
    }
}

impl From<io::Error> for GitLogError {
    fn from(e: io::Error) -> Self {
        GitLogError::Io(e)
    }
}

/// Reads commits one at a time from a stream produced with [`LOG_FORMAT`].
///
/// Each call to `next()` reads exactly one record's worth of bytes from the
/// underlying reader and discards them once the `Commit` is built. Nothing
/// from earlier or later commits is retained, so memory use stays flat no
/// matter how long the history is.
pub struct CommitReader<R: Read> {
    records: RecordReader<R>,
}

impl<R: Read> CommitReader<R> {
    pub fn new(reader: R) -> Self {
        CommitReader {
            records: RecordReader::new(reader),
        }
    }
}

impl<R: Read> Iterator for CommitReader<R> {
    type Item = Result<Commit, GitLogError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.records.next_record()? {
            Ok(bytes) => Some(parse_record(bytes)),
            Err(e) => Some(Err(GitLogError::Io(e))),
        }
    }
}

/// Like [`CommitReader`], but for a stream produced with [`LOG_FORMAT`] and
/// `--numstat`. Each item is a [`StatCommit`] carrying the per-file line
/// counts git prints after the format line.
///
/// Merge commits get an empty `files` list because `git log --numstat` does
/// not diff merges unless asked to with `-m`, `-c` or `--cc`.
pub struct StatReader<R: Read> {
    records: RecordReader<R>,
}

impl<R: Read> StatReader<R> {
    pub fn new(reader: R) -> Self {
        StatReader {
            records: RecordReader::new(reader),
        }
    }
}

impl<R: Read> Iterator for StatReader<R> {
    type Item = Result<StatCommit, GitLogError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.records.next_record()? {
            Ok(bytes) => Some(parse_stat_record(bytes)),
            Err(e) => Some(Err(GitLogError::Io(e))),
        }
    }
}

/// Splits the stream into raw records on the record separator, reusing one
/// buffer so only a single record is ever held.
struct RecordReader<R: Read> {
    reader: BufReader<R>,
    buf: Vec<u8>,
}

impl<R: Read> RecordReader<R> {
    fn new(reader: R) -> Self {
        RecordReader {
            reader: BufReader::new(reader),
            buf: Vec::new(),
        }
    }

    fn next_record(&mut self) -> Option<io::Result<&[u8]>> {
        loop {
            self.buf.clear();
            match self.reader.read_until(RECORD_SEP, &mut self.buf) {
                Ok(0) => return None,
                Ok(_) => {
                    if self.buf.last() == Some(&RECORD_SEP) {
                        self.buf.pop();
                    }
                    // The very first read consumes only the leading %x1e
                    // that opens the first record, leaving an empty buffer.
                    // Trailing newlines between records collapse the same
                    // way if git ever emits a blank one.
                    while self.buf.last() == Some(&b'\n') {
                        self.buf.pop();
                    }
                    if self.buf.is_empty() {
                        continue;
                    }
                    return Some(Ok(&self.buf));
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}

fn decode(bytes: &[u8]) -> Result<&str, GitLogError> {
    std::str::from_utf8(bytes)
        .map_err(|_| GitLogError::Malformed(String::from_utf8_lossy(bytes).into_owned()))
}

fn parse_record(bytes: &[u8]) -> Result<Commit, GitLogError> {
    parse_fields(decode(bytes)?)
}

fn parse_stat_record(bytes: &[u8]) -> Result<StatCommit, GitLogError> {
    let text = decode(bytes)?;
    // %s never contains a newline, so the first one ends the format line and
    // everything after it is numstat output.
    let (head, stats) = match text.split_once('\n') {
        Some((head, stats)) => (head, stats),
        None => (text, ""),
    };
    let commit = parse_fields(head)?;

    let mut files = Vec::new();
    for line in stats.lines().filter(|l| !l.is_empty()) {
        let mut parts = line.splitn(3, '\t');
        let (added, deleted, path) = match (parts.next(), parts.next(), parts.next()) {
            (Some(a), Some(d), Some(p)) => (a, d, p),
            _ => return Err(GitLogError::Malformed(text.to_string())),
        };
        files.push(FileStat {
            added: parse_count(added, text)?,
            deleted: parse_count(deleted, text)?,
            path: path.to_string(),
        });
    }
    Ok(StatCommit { commit, files })
}

/// Binary files are reported as `-`, which maps to `None`.
fn parse_count(s: &str, record: &str) -> Result<Option<u64>, GitLogError> {
    if s == "-" {
        return Ok(None);
    }
    s.parse::<u64>()
        .map(Some)
        .map_err(|_| GitLogError::Malformed(record.to_string()))
}

fn parse_fields(text: &str) -> Result<Commit, GitLogError> {
    let mut fields = text.splitn(FIELD_COUNT, FIELD_SEP as char);
    let hash = fields.next();
    let parents = fields.next();
    let author_name = fields.next();
    let author_email = fields.next();
    let author_time = fields.next();
    let subject = fields.next();

    match (hash, parents, author_name, author_email, author_time, subject) {
        (
            Some(hash),
            Some(parents),
            Some(author_name),
            Some(author_email),
            Some(author_time),
            Some(subject),
        ) => {
            let author_time = author_time
                .parse::<i64>()
                .map_err(|_| GitLogError::Malformed(text.to_string()))?;
            let parents = parents
                .split_whitespace()
                .map(str::to_string)
                .collect();
            Ok(Commit {
                hash: hash.to_string(),
                parents,
                author_name: author_name.to_string(),
                author_email: author_email.to_string(),
                author_time,
                subject: subject.to_string(),
            })
        }
        _ => Err(GitLogError::Malformed(text.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        hash: &str,
        parents: &str,
        name: &str,
        email: &str,
        time: &str,
        subject: &str,
    ) -> Vec<u8> {
        let mut out = vec![RECORD_SEP];
        out.extend(hash.as_bytes());
        out.push(FIELD_SEP);
        out.extend(parents.as_bytes());
        out.push(FIELD_SEP);
        out.extend(name.as_bytes());
        out.push(FIELD_SEP);
        out.extend(email.as_bytes());
        out.push(FIELD_SEP);
        out.extend(time.as_bytes());
        out.push(FIELD_SEP);
        out.extend(subject.as_bytes());
        out.push(b'\n');
        out
    }

    #[test]
    fn parses_a_single_commit() {
        let input = record(
            "abc123",
            "parent1",
            "Jane Doe",
            "jane@example.com",
            "1700000000",
            "fix bug",
        );
        let mut reader = CommitReader::new(input.as_slice());
        let commit = reader.next().unwrap().unwrap();
        assert_eq!(commit.hash, "abc123");
        assert_eq!(commit.parents, vec!["parent1"]);
        assert_eq!(commit.author_name, "Jane Doe");
        assert_eq!(commit.author_email, "jane@example.com");
        assert_eq!(commit.author_time, 1700000000);
        assert_eq!(commit.subject, "fix bug");
        assert!(reader.next().is_none());
    }

    #[test]
    fn parses_multiple_commits_in_sequence() {
        let mut input = record("aaa", "", "A", "a@example.com", "1", "first");
        input.extend(record("bbb", "aaa", "B", "b@example.com", "2", "second"));
        let reader = CommitReader::new(input.as_slice());
        let commits: Vec<Commit> = reader.map(|r| r.unwrap()).collect();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "aaa");
        assert!(commits[0].parents.is_empty());
        assert_eq!(commits[1].hash, "bbb");
        assert_eq!(commits[1].parents, vec!["aaa"]);
    }

    #[test]
    fn merge_commit_has_multiple_parents() {
        let input = record("mmm", "aaa bbb", "M", "m@example.com", "3", "merge");
        let mut reader = CommitReader::new(input.as_slice());
        let commit = reader.next().unwrap().unwrap();
        assert_eq!(commit.parents, vec!["aaa", "bbb"]);
    }

    #[test]
    fn subject_may_contain_extra_unit_separators() {
        // splitn(FIELD_COUNT, ..) means only the separators up to the last
        // field split; anything after that stays part of the subject verbatim.
        let input = record("aaa", "", "A", "a@example.com", "1", "weird\x1fsubject");
        let mut reader = CommitReader::new(input.as_slice());
        let commit = reader.next().unwrap().unwrap();
        assert_eq!(commit.subject, "weird\x1fsubject");
    }

    #[test]
    fn empty_input_yields_no_commits() {
        let reader = CommitReader::new(&b""[..]);
        assert_eq!(reader.count(), 0);
    }

    fn stat_record(header: Vec<u8>, stats: &str) -> Vec<u8> {
        // header ends in "\n" already; git puts a blank line before numstat.
        let mut out = header;
        out.push(b'\n');
        out.extend(stats.as_bytes());
        out
    }

    #[test]
    fn stat_reader_parses_file_counts() {
        let mut input = stat_record(
            record("aaa", "", "A", "a@example.com", "1", "first"),
            "3\t1\tsrc/lib.rs\n-\t-\tlogo.png\n0\t5\tdir/old name.txt\n",
        );
        input.extend(record("bbb", "aaa", "B", "b@example.com", "2", "empty"));
        let commits: Vec<StatCommit> = StatReader::new(input.as_slice())
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].commit.subject, "first");
        assert_eq!(
            commits[0].files,
            vec![
                FileStat { added: Some(3), deleted: Some(1), path: "src/lib.rs".into() },
                FileStat { added: None, deleted: None, path: "logo.png".into() },
                FileStat { added: Some(0), deleted: Some(5), path: "dir/old name.txt".into() },
            ]
        );
        assert_eq!(commits[1].commit.hash, "bbb");
        assert!(commits[1].files.is_empty());
    }

    #[test]
    fn stat_reader_rejects_a_bad_count() {
        let input = stat_record(
            record("aaa", "", "A", "a@example.com", "1", "first"),
            "x\t1\tfile\n",
        );
        let mut reader = StatReader::new(input.as_slice());
        assert!(matches!(reader.next(), Some(Err(GitLogError::Malformed(_)))));
    }

    #[test]
    fn stat_reader_rejects_a_line_without_a_path() {
        let input = stat_record(
            record("aaa", "", "A", "a@example.com", "1", "first"),
            "1\t2\n",
        );
        let mut reader = StatReader::new(input.as_slice());
        assert!(matches!(reader.next(), Some(Err(GitLogError::Malformed(_)))));
    }

    #[test]
    fn rejects_a_record_missing_fields() {
        let mut bad = vec![RECORD_SEP];
        bad.extend(b"onlyhash\n");
        let mut reader = CommitReader::new(bad.as_slice());
        assert!(matches!(reader.next(), Some(Err(GitLogError::Malformed(_)))));
    }
}
