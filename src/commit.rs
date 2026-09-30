/// A single parsed commit record.
///
/// `author_time` is seconds since the Unix epoch, taken straight from `%at`,
/// so it is always UTC regardless of the author's local timezone at the time
/// of the commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    /// Hashes of this commit's parents, in the order git reports them.
    /// Empty for a root commit, length two or more for a merge.
    pub parents: Vec<String>,
    pub author_name: String,
    pub author_email: String,
    pub author_time: i64,
    pub subject: String,
}

/// Line counts for one file in a commit, from `git log --numstat`.
///
/// `added` and `deleted` are `None` for binary files, where git prints `-`
/// instead of a number. `path` is exactly what git printed, so renames show up
/// in git's `old => new` form and unusual names may be C-quoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub added: Option<u64>,
    pub deleted: Option<u64>,
    pub path: String,
}

/// A commit plus the files it touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatCommit {
    pub commit: Commit,
    pub files: Vec<FileStat>,
}
