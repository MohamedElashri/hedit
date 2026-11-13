//! Read-only queries resolve revisions to immutable objects before diffing.
use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryScope {
    /// None includes all branches, tags, and HEAD.
    pub reference: Option<String>,
    pub path: Option<PathBuf>,
}
#[derive(Clone, Debug)]
pub struct Comparison {
    pub base: String,
    pub head: String,
    pub files: Vec<FileStatus>,
    pub ahead: usize,
    pub behind: usize,
}
#[derive(Clone, Debug)]
pub struct Tag {
    pub name: String,
    pub object: String,
    pub commit: Option<String>,
}
#[derive(Clone, Debug)]
pub struct ReflogEntry {
    pub oid: String,
    pub selector: String,
    pub subject: String,
    pub date: String,
}
#[derive(Clone, Debug)]
pub struct BlameLine {
    pub oid: String,
    pub number: usize,
    pub author: String,
    pub summary: String,
    pub text: String,
}

impl Git {
    pub fn resolve_commit(&self, root: &Path, reference: &str) -> Result<String> {
        valid_ref(reference)?;
        let oid = self.command(
            root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{reference}^{{commit}}"),
            ],
        )?;
        let oid = oid.trim().to_owned();
        check_oid(&oid)?;
        Ok(oid)
    }
    pub fn scoped_history(
        &self,
        root: &Path,
        limit: usize,
        scope: &HistoryScope,
    ) -> Result<Vec<Commit>> {
        let mut arguments = args(&[
            "log",
            &format!("-{}", limit.clamp(1, 2000)),
            "--date-order",
            "--format=%H%x00%P%x00%an%x00%ar%x00%D%x00%s%x00",
        ]);
        if let Some(reference) = &scope.reference {
            arguments.push(self.resolve_commit(root, reference)?.into());
        } else {
            arguments.extend(args(&["--branches", "--remotes", "--tags", "HEAD"]));
        }
        arguments.push("--".into());
        if let Some(path) = &scope.path {
            validate_relative(path)?;
            arguments.push(path.as_os_str().into());
        }
        let bytes = self.checked(root, &arguments, None, false)?;
        Ok(Self::parse_history(&String::from_utf8_lossy(&bytes)))
    }
    /// Merge-base mode reviews only the head branch's contribution since its fork.
    /// Counts always describe the tips, even when the diff starts at a merge base.
    pub fn compare(
        &self,
        root: &Path,
        base: &str,
        head: &str,
        merge_base: bool,
    ) -> Result<Comparison> {
        self.require_root(root)?;
        let tip = self.resolve_commit(root, base)?;
        let head = self.resolve_commit(root, head)?;
        let counts = self.command(
            root,
            &[
                "rev-list",
                "--left-right",
                "--count",
                &format!("{tip}...{head}"),
            ],
        )?;
        let mut counts = counts.split_whitespace();
        let behind = counts
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or("Invalid comparison counts.")?;
        let ahead = counts
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or("Invalid comparison counts.")?;
        let base = if merge_base {
            self.command(root, &["merge-base", &tip, &head])
                .map_err(|error| format!("Cannot find a common ancestor. Turn off ‘Changes since common ancestor’ to compare the tips directly.\n{error}"))?
                .trim()
                .to_owned()
        } else {
            tip
        };
        check_oid(&base)?;
        let raw = self.checked(
            root,
            &args(&[
                "diff",
                "--name-status",
                "--no-renames",
                "-z",
                &base,
                &head,
                "--",
            ]),
            None,
            false,
        )?;
        let mut parts = raw.split(|b| *b == 0);
        let mut files = Vec::new();
        while let Some(code) = parts.next().filter(|c| !c.is_empty()) {
            let path = parts.next().ok_or("Incomplete comparison file list.")?;
            files.push(FileStatus {
                path: PathBuf::from(os_path(path)?),
                original: None,
                index: code[0] as char,
                worktree: ' ',
                conflict: false,
            });
        }
        Ok(Comparison {
            base,
            head,
            files,
            ahead,
            behind,
        })
    }
    pub fn tags(&self, root: &Path) -> Result<Vec<Tag>> {
        let text = self.command(root, &["for-each-ref", "--sort=-creatordate", "--format=%(refname:strip=2)%00%(objectname)%00%(objecttype)%00%(*objectname)%00%(*objecttype)", "refs/tags"])?;
        Ok(text
            .lines()
            .filter_map(|line| {
                let fields: Vec<_> = line.split('\0').collect();
                if fields.len() != 5 {
                    return None;
                }
                let commit = match (fields[2], fields[4]) {
                    ("commit", _) => Some(fields[1].into()),
                    (_, "commit") => Some(fields[3].into()),
                    _ => None,
                };
                Some(Tag {
                    name: fields[0].into(),
                    object: fields[1].into(),
                    commit,
                })
            })
            .collect())
    }
    pub fn reflog(&self, root: &Path, limit: usize) -> Result<Vec<ReflogEntry>> {
        self.require_root(root)?;
        // An unborn HEAD has no log. Propagate cancellation and process errors.
        if !self
            .run(root, &args(&["rev-parse", "--verify", "HEAD"]), None, false)?
            .success
        {
            return Ok(vec![]);
        }
        let text = self.command(
            root,
            &[
                "reflog",
                "show",
                &format!("-{}", limit.clamp(1, 2000)),
                "--format=%H%x00%gd%x00%gs%x00%ar%x00",
                "HEAD",
                "--",
            ],
        )?;
        let fields: Vec<_> = text.split('\0').collect();
        Ok(fields
            .as_chunks::<4>()
            .0
            .iter()
            .map(|v| ReflogEntry {
                oid: v[0].trim_start_matches('\n').into(),
                selector: v[1].into(),
                subject: v[2].into(),
                date: v[3].into(),
            })
            .collect())
    }
    /// Blame committed text only: unsaved and unstaged edits never masquerade as history.
    pub fn blame(&self, root: &Path, reference: &str, path: &Path) -> Result<Vec<BlameLine>> {
        validate_relative(path)?;
        let oid = self.resolve_commit(root, reference)?;
        let mut arguments = args(&["blame", "--line-porcelain", &oid, "--"]);
        arguments.push(path.as_os_str().into());
        let bytes = self.checked(root, &arguments, None, false)?;
        if bytes.contains(&0) {
            return Err("Line history is available for text files only.".into());
        }
        let mut result = Vec::new();
        let (mut commit, mut number, mut author, mut summary) =
            (String::new(), 0, String::new(), String::new());
        for raw in bytes.split(|byte| *byte == b'\n') {
            let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
            if let Some(text) = raw.strip_prefix(b"\t") {
                // Metadata can include raw filename bytes when core.quotePath is
                // false. Only the source itself must be UTF-8.
                let text =
                    std::str::from_utf8(text).map_err(|_| "Line history requires UTF-8 text.")?;
                result.push(BlameLine {
                    oid: commit.clone(),
                    number,
                    author: author.clone(),
                    summary: summary.clone(),
                    text: text.into(),
                });
            } else {
                let line = String::from_utf8_lossy(raw);
                if let Some(value) = line.strip_prefix("author ") {
                    author = value.into();
                } else if let Some(value) = line.strip_prefix("summary ") {
                    summary = value.into();
                } else {
                    let parts: Vec<_> = line.split_whitespace().collect();
                    if parts.len() >= 3 && check_oid(parts[0]).is_ok() {
                        commit = parts[0].into();
                        number = parts[2].parse().map_err(|_| "Invalid blame line number.")?;
                        author.clear();
                        summary.clear();
                    }
                }
            }
        }
        Ok(result)
    }
}
