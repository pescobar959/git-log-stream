# git-log-stream

A Rust library that parses `git log` output into `Commit` records one at a
time, from any `std::io::Read`, without ever holding the whole history in
memory.

## The problem

`git log` on a repository with a real amount of history produces a lot of
text. Read it into a `String`, split it into lines, collect it into a
`Vec<Commit>` — any of the obvious approaches means peak memory use scales
with the size of the repo. That's fine for a hobby project and a problem for
anything with a few hundred thousand commits, or for a long-running service
that processes many repos.

This crate reads one commit record at a time and hands it to the caller
before reading the next one. Peak memory use is bounded by the size of a
single commit record (basically: how long the subject line is), not by how
much history exists.

It does not shell out to `git` for you. You decide how the byte stream is
produced — a spawned `git log` process, a file someone piped it to, a network
socket, whatever implements `Read`.

## Usage

`git log` needs to be told to emit records in a format this crate can split
unambiguously. The `LOG_FORMAT` constant is exactly that format string —
pass it to `git log --format=`.

```rust
use std::process::{Command, Stdio};
use git_log_stream::{CommitReader, LOG_FORMAT};

fn main() -> std::io::Result<()> {
    let mut child = Command::new("git")
        .arg("log")
        .arg(format!("--format={LOG_FORMAT}"))
        .stdout(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().expect("child stdout was piped");
    let commits = CommitReader::new(stdout);

    let mut by_author: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for commit in commits {
        let commit = commit.expect("valid commit record");
        *by_author.entry(commit.author_name).or_insert(0) += 1;
    }

    child.wait()?;

    let mut counts: Vec<_> = by_author.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    for (author, count) in counts.iter().take(10) {
        println!("{count:>6}  {author}");
    }

    Ok(())
}
```

At no point does that loop hold more than one commit in memory. It never
builds a `Vec<Commit>` of the whole history — `by_author` grows with the
number of distinct authors, not the number of commits.

`CommitReader` also works over anything else that implements `Read`, which
makes it easy to test against a fixed byte string instead of spawning `git`:

```rust
use git_log_stream::CommitReader;

let input = b"\x1eabc123\x1fJane Doe\x1fjane@example.com\x1f1700000000\x1ffix the thing\n";
let mut commits = CommitReader::new(&input[..]);
let commit = commits.next().unwrap().unwrap();
assert_eq!(commit.hash, "abc123");
```

## Why `%x1e` / `%x1f`

Commit subjects can contain almost anything, including newlines (for the
first line of a multi-line format) and arbitrary punctuation, so splitting
records on `\n` or fields on a printable character like `|` is one bad commit
message away from breaking. `%x1e` (record separator) and `%x1f` (unit
separator) are ASCII control characters set aside for exactly this — field
and record separation in text — and they never legitimately appear in commit
metadata, so splitting on them is safe without any escaping.

## Status

Early. The commit format currently covers hash, author name, author email,
author timestamp, and subject. See the roadmap in the repo for what's next —
diffstats, parent hashes, and a way to bound how much of the stream is read
are the main gaps.

## License

MIT. See `LICENSE`.
