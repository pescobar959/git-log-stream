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
