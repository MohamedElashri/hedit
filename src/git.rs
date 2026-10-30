//! Native source control, using the user's Git executable and configuration.
//! All commands have bounded output/time, run without a shell, and accept literal
//! paths. The UI calls this module only on background workers.
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::{Read, Seek, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: usize = 24 * 1024 * 1024;
pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Default)]
pub struct Git {
    pub cancel: Arc<AtomicBool>,
}
struct Output {
    success: bool,
    stdout: Vec<u8>,
    stderr: String,
}
impl Output {
    fn error(self) -> String {
        let message = format!(
            "{}\n{}",
            String::from_utf8_lossy(&self.stdout).trim(),
            self.stderr
        )
        .trim()
        .to_owned();
        if message.is_empty() {
            "Git could not complete the operation.".into()
        } else {
            message
        }
    }
}

fn read_bounded(mut stream: impl Read) -> std::io::Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        let keep = n.min((OUTPUT_LIMIT + 1).saturating_sub(result.len()));
        result.extend_from_slice(&chunk[..keep]);
    }
    Ok(result)
}

impl Git {
    fn run(
        &self,
        root: &Path,
        args: &[OsString],
        input: Option<&[u8]>,
        remote: bool,
    ) -> Result<Output> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err("Git operation cancelled.".into());
        }
        // A file avoids pipe deadlocks with hooks/filters that write before reading stdin.
        let mut stdin = tempfile::tempfile().map_err(|e| e.to_string())?;
        if let Some(input) = input {
            stdin.write_all(input).map_err(|e| e.to_string())?;
        }
        stdin.rewind().map_err(|e| e.to_string())?;
        let mut command = Command::new("git");
        command.args(["--no-pager", "-c", "color.ui=false"]);
        // --literal-pathspecs is inherited by Git's own subprocesses. Applying
        // it to `stash` prevents Git 2.43 from cleaning saved untracked files.
        // Only path-taking commands need it; ref/remote commands have no paths.
        if args.first().is_some_and(|a| {
            [
                "add",
                "reset",
                "rm",
                "diff",
                "show",
                "restore",
                "diff-tree",
                "checkout",
            ]
            .iter()
            .any(|name| a == name)
        }) {
            command.arg("--literal-pathspecs");
        }
        command
            .args(args)
            .current_dir(root)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_EDITOR", "true")
            .env("GIT_SEQUENCE_EDITOR", "true")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_LITERAL_PATHSPECS")
            .env_remove("GIT_GLOB_PATHSPECS")
            .env_remove("GIT_NOGLOB_PATHSPECS")
            .env_remove("GIT_ICASE_PATHSPECS")
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|e| {
            format!("Cannot start Git: {e}. Install Git and make sure it is on PATH.")
        })?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let out = std::thread::spawn(move || read_bounded(stdout));
        let err = std::thread::spawn(move || read_bounded(stderr));
        let start = Instant::now();
        let timeout = Duration::from_secs(if remote { 120 } else { 30 });
        let mut stopped = None;
        let status = loop {
            if self.cancel.load(Ordering::Relaxed) || start.elapsed() > timeout {
                stopped = Some(if self.cancel.load(Ordering::Relaxed) {
                    "Git operation cancelled. Refresh to check its outcome."
                } else {
                    "Git operation timed out. Refresh to check its outcome."
                });
                #[cfg(unix)]
                if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
                    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                }
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    let _ = Command::new("taskkill.exe")
                        .args(["/PID", &child.id().to_string(), "/T", "/F"])
                        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
                let _ = child.kill();
                break child.wait().map_err(|e| e.to_string())?;
            }
            match child.try_wait().map_err(|e| e.to_string())? {
                Some(status) if out.is_finished() && err.is_finished() => break status,
                Some(_) => std::thread::sleep(Duration::from_millis(10)),
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        if let Some(reason) = stopped {
            return Err(reason.into());
        }
        let stdout = out
            .join()
            .map_err(|_| "Git output worker failed.")?
            .map_err(|e| e.to_string())?;
        let stderr = err
            .join()
            .map_err(|_| "Git error worker failed.")?
            .map_err(|e| e.to_string())?;
        if stdout.len() > OUTPUT_LIMIT || stderr.len() > OUTPUT_LIMIT {
            return Err(
                "Git output exceeds 24 MiB; narrow the selection or use the command line.".into(),
            );
        }
        Ok(Output {
            success: status.success(),
            stdout,
            stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
        })
    }
    pub fn command(&self, root: &Path, args: &[&str]) -> Result<String> {
        self.checked(
            root,
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
            None,
            false,
        )
        .map(|v| String::from_utf8_lossy(&v).into_owned())
    }
    fn checked(
        &self,
        root: &Path,
        args: &[OsString],
        input: Option<&[u8]>,
        remote: bool,
    ) -> Result<Vec<u8>> {
        let out = self.run(root, args, input, remote)?;
        if out.success {
            Ok(out.stdout)
        } else {
            Err(out.error())
        }
    }
    pub fn discover(&self, directory: &Path) -> Result<PathBuf> {
        let out = self.checked(
            directory,
            &args(&["rev-parse", "--show-toplevel"]),
            None,
            false,
        )?;
        let bytes = out.strip_suffix(b"\n").unwrap_or(&out);
        let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
        PathBuf::from(os_path(bytes)?)
            .canonicalize()
            .map_err(|e| e.to_string())
    }
    pub fn snapshot(&self, root: &Path, limit: usize) -> Result<Snapshot> {
        self.require_root(root)?;
        let raw = self.checked(
            root,
            &args(&[
                "status",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--untracked-files=all",
            ]),
            None,
            false,
        )?;
        let mut snapshot = parse_status(&raw)?;
        snapshot.root = root.to_owned();
        snapshot.branches = self.command(root, &["for-each-ref", "--sort=refname", "--format=%(refname:short)%00%(refname)%00%(objectname)%00%(upstream:short)%00%(HEAD)%00%(symref)", "refs/heads", "refs/remotes"])?
            .lines().filter_map(|s| {
                let v: Vec<_> = s.split('\0').collect();
                (v.len() == 6 && v[5].is_empty()).then(|| Branch { name: v[0].into(), reference: v[1].into(), oid: v[2].into(), upstream: v[3].into(), current: v[4] == "*", remote: v[1].starts_with("refs/remotes/") })
            }).collect();
        snapshot.remotes = self
            .command(root, &["remote"])?
            .lines()
            .map(str::to_owned)
            .collect();
        snapshot.stashes = self
            .command(root, &["stash", "list", "--format=%H%x00%gd%x00%gs"])?
            .lines()
            .filter_map(|s| {
                let v: Vec<_> = s.splitn(3, '\0').collect();
                (v.len() == 3).then(|| Stash {
                    oid: v[0].into(),
                    name: v[1].into(),
                    subject: v[2].into(),
                })
            })
            .collect();
        if !snapshot.unborn {
            snapshot.commits = self.history(root, limit)?;
            snapshot.graph = history_graph(&snapshot.commits);
        }
        for (file, label) in [
            ("MERGE_HEAD", "Merge in progress"),
            ("CHERRY_PICK_HEAD", "Cherry-pick in progress"),
            ("REVERT_HEAD", "Revert in progress"),
            ("rebase-merge", "Rebase in progress"),
            ("rebase-apply", "Rebase in progress"),
        ] {
            let path = self.command(root, &["rev-parse", "--git-path", file])?;
            let path = PathBuf::from(path.trim_end());
            if root.join(path).exists() {
                snapshot.operation = Some(label.into());
                break;
            }
        }
        Ok(snapshot)
    }
    pub fn history(&self, root: &Path, limit: usize) -> Result<Vec<Commit>> {
        let out = self.command(
            root,
            &[
                "log",
                &format!("-{}", limit.min(2000)),
                "--date-order",
                "--branches",
                "--remotes",
                "--tags",
                "HEAD",
                "--format=%H%x00%P%x00%an%x00%ar%x00%D%x00%s%x00",
            ],
        )?;
        let mut fields = out.split('\0');
        let mut commits = Vec::new();
        while let Some(oid) = fields.next() {
            let oid = oid.trim_start_matches('\n');
            if oid.is_empty() {
                break;
            }
            let Some(parents) = fields.next() else {
                break;
            };
            let Some(author) = fields.next() else {
                break;
            };
            let Some(date) = fields.next() else {
                break;
            };
            let Some(refs) = fields.next() else {
                break;
            };
            let Some(subject) = fields.next() else {
                break;
            };
            commits.push(Commit {
                oid: oid.into(),
                parents: parents.split_whitespace().map(str::to_owned).collect(),
                author: author.into(),
                date: date.into(),
                refs: refs.into(),
                subject: subject.into(),
            });
        }
        Ok(commits)
    }
    pub fn diff(&self, root: &Path, selection: &Selection) -> Result<Diff> {
        if selection.kind == DiffKind::Conflict {
            let path = safe_path(root, &selection.path)?;
            let metadata = std::fs::symlink_metadata(&path).ok();
            if metadata
                .as_ref()
                .is_some_and(|m| !m.is_file() || m.len() > 2 * 1024 * 1024)
            {
                return Err(
                    "Resolve this link, submodule, or large conflict in the editor or a terminal."
                        .into(),
                );
            }
            let working = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
                Err(e) => return Err(e.to_string()),
            };
            let mut sides = Vec::new();
            for stage in [1, 2, 3] {
                let mut object = OsString::from(format!(":{stage}:"));
                object.push(selection.path.as_os_str());
                let output = self.run(root, &["show".into(), object], None, false)?;
                sides.push(if output.success {
                    if output.stdout.len() > 2 * 1024 * 1024 || output.stdout.contains(&0) { return Err("Binary or large conflict. Resolve it in the editor or a terminal, then stage the file.".into()); }
                    Some(String::from_utf8(output.stdout).map_err(|_| "This conflict is not UTF-8 text. Resolve it in the editor or a terminal.")?)
                } else { None });
            }
            let mut fingerprint = Sha256::new();
            fingerprint.update((working.len() as u64).to_le_bytes());
            fingerprint.update(&working);
            for side in &sides {
                if let Some(text) = side {
                    fingerprint.update([1]);
                    fingerprint.update((text.len() as u64).to_le_bytes());
                    fingerprint.update(text.as_bytes());
                } else {
                    fingerprint.update([0]);
                }
            }
            let result = Diff {
                conflict: Some(Conflict {
                    base: sides.remove(0),
                    ours: sides.remove(0),
                    theirs: sides.remove(0),
                    expected: fingerprint
                        .finalize()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect(),
                }),
                ..Default::default()
            };
            return Ok(result);
        }
        let mut arguments = args(&[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--unified=3",
        ]);
        match &selection.kind {
            DiffKind::Staged => arguments.push("--cached".into()),
            DiffKind::Commit(oid) => {
                check_oid(oid)?;
                if let Some(parent) = self.first_parent(root, oid)? {
                    arguments.extend(args(&[&parent, oid]));
                } else {
                    arguments = args(&[
                        "show",
                        "--format=",
                        "--no-ext-diff",
                        "--no-textconv",
                        "--no-color",
                        "--no-renames",
                        "--unified=3",
                        oid,
                    ]);
                }
            }
            DiffKind::Untracked => {
                let path = safe_path(root, &selection.path)?;
                let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 {
                    return Ok(Diff::message("New file. Preview is unavailable for folders, links, or files larger than 2 MiB."));
                }
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
                    return Ok(Diff::message(
                        "New binary file. Use Stage file to include it in the next commit.",
                    ));
                }
                let text = String::from_utf8(bytes).unwrap();
                let mut patch = String::from("New file\n");
                patch.push_str(&format!("@@ -0,0 +1,{} @@\n", text.lines().count()));
                for line in text.lines() {
                    patch.push('+');
                    patch.push_str(line);
                    patch.push('\n');
                }
                return Ok(Diff::parse(patch, false));
            }
            DiffKind::Working => (),
            DiffKind::Conflict => unreachable!(),
        }
        validate_relative(&selection.path)?;
        arguments.push("--".into());
        arguments.push(selection.path.as_os_str().into());
        let bytes = self.checked(root, &arguments, None, false)?;
        let patch = String::from_utf8_lossy(&bytes).into_owned();
        let applicable =
            !matches!(selection.kind, DiffKind::Commit(_)) && std::str::from_utf8(&bytes).is_ok();
        Ok(Diff::parse(patch, applicable))
    }
    pub fn commit_files(&self, root: &Path, oid: &str) -> Result<Vec<FileStatus>> {
        check_oid(oid)?;
        let arguments = if let Some(parent) = self.first_parent(root, oid)? {
            args(&[
                "diff",
                "--name-status",
                "--no-renames",
                "-z",
                &parent,
                oid,
                "--",
            ])
        } else {
            args(&[
                "diff-tree",
                "--root",
                "--no-commit-id",
                "--name-status",
                "--no-renames",
                "-r",
                "-z",
                oid,
                "--",
            ])
        };
        let raw = self.checked(root, &arguments, None, false)?;
        let mut parts = raw.split(|b| *b == 0);
        let mut files = Vec::new();
        while let Some(code) = parts.next() {
            if code.is_empty() {
                break;
            }
            let path = parts.next().ok_or("Incomplete Git commit file list.")?;
            files.push(FileStatus {
                path: PathBuf::from(os_path(path)?),
                original: None,
                index: code[0] as char,
                worktree: ' ',
                conflict: false,
            });
        }
        Ok(files)
    }
    fn first_parent(&self, root: &Path, oid: &str) -> Result<Option<String>> {
        let parents = self.command(root, &["rev-list", "--parents", "-n", "1", oid])?;
        Ok(parents.split_whitespace().nth(1).map(str::to_owned))
    }
    pub fn commit_message(&self, root: &Path, oid: &str) -> Result<String> {
        check_oid(oid)?;
        self.command(root, &["show", "-s", "--format=%B", oid])
    }
    pub fn execute(&self, root: &Path, operation: &Operation) -> Result<String> {
        if matches!(operation, Operation::Init) {
            if self.discover(root).is_ok() {
                return Err("This folder already belongs to a Git repository. Refresh or open another folder.".into());
            }
        } else {
            self.require_root(root)?;
        }
        let mut input = None;
        let mut remote = false;
        let arguments = match operation {
            Operation::Stage(paths) => return self.path_operation(root, &["add", "--all"], paths),
            Operation::Unstage(paths) => {
                let has_head = self
                    .run(root, &args(&["rev-parse", "--verify", "HEAD"]), None, false)?
                    .success;
                return self.path_operation(
                    root,
                    if has_head {
                        &["reset", "-q", "HEAD"]
                    } else {
                        &["rm", "--cached", "-r", "--force", "--ignore-unmatch"]
                    },
                    paths,
                );
            }
            Operation::Hunk {
                selection,
                expected,
                hunk,
            } => {
                let current = self.diff(root, selection)?;
                if current.patch != *expected {
                    return Err(
                        "The file changed since this diff was opened. Refresh and review it again."
                            .into(),
                    );
                }
                let patch = current
                    .hunk_patch(*hunk)
                    .ok_or("This change cannot be staged separately; use the file action.")?;
                input = Some(patch.into_bytes());
                let mut a = args(&["apply", "--cached", "--whitespace=nowarn"]);
                if selection.kind == DiffKind::Staged {
                    a.push("--reverse".into());
                }
                a
            }
            Operation::Commit { message, amend } => {
                if message.trim().is_empty() {
                    return Err("Enter a commit message first.".into());
                }
                input = Some(message.as_bytes().to_vec());
                let mut a = args(&["commit", "--file=-"]);
                if *amend {
                    a.push("--amend".into());
                }
                a
            }
            Operation::Switch(branch) => {
                valid_ref(branch)?;
                args(&["switch", "--", branch])
            }
            Operation::CreateBranch(branch) => {
                valid_ref(branch)?;
                args(&["switch", "-c", branch])
            }
            Operation::TrackBranch {
                local,
                remote: reference,
            } => {
                valid_ref(local)?;
                valid_ref(reference)?;
                args(&["switch", "--track", "-c", local, reference])
            }
            Operation::DeleteBranch(branch) => {
                valid_ref(branch)?;
                args(&["branch", "-d", "--", branch])
            }
            Operation::Merge(branch) => {
                valid_ref(branch)?;
                args(&["merge", "--no-edit", "--", branch])
            }
            Operation::CherryPick(oid) => {
                check_oid(oid)?;
                args(&["cherry-pick", "--no-edit", oid])
            }
            Operation::Revert(oid) => {
                check_oid(oid)?;
                args(&["revert", "--no-edit", oid])
            }
            Operation::Continue(kind) => match kind.as_str() {
                "Merge in progress" => args(&["commit", "--no-edit"]),
                "Cherry-pick in progress" => args(&["cherry-pick", "--continue"]),
                "Revert in progress" => args(&["revert", "--continue"]),
                "Rebase in progress" => args(&["rebase", "--continue"]),
                _ => return Err("Unknown Git operation.".into()),
            },
            Operation::Abort(kind) => match kind.as_str() {
                "Merge in progress" => args(&["merge", "--abort"]),
                "Cherry-pick in progress" => args(&["cherry-pick", "--abort"]),
                "Revert in progress" => args(&["revert", "--abort"]),
                "Rebase in progress" => args(&["rebase", "--abort"]),
                _ => return Err("Unknown Git operation.".into()),
            },
            Operation::Stash(message) => {
                args(&["stash", "push", "--include-untracked", "--message", message])
            }
            Operation::ApplyStash(oid) => {
                check_oid(oid)?;
                args(&["stash", "apply", "--index", oid])
            }
            Operation::DropStash { name, oid } => {
                if !name.starts_with("stash@{") || !name.ends_with('}') {
                    return Err("Invalid stash reference.".into());
                }
                let current = self.command(root, &["rev-parse", "--verify", name])?;
                if current.trim() != oid {
                    return Err("The stash list changed. Refresh and try again.".into());
                }
                args(&["stash", "drop", name])
            }
            Operation::Fetch => {
                remote = true;
                args(&["fetch", "--all", "--prune"])
            }
            Operation::AddRemote { name, url } => {
                valid_ref(name)?;
                if url.trim().is_empty() {
                    return Err("Enter a remote URL.".into());
                }
                args(&["remote", "add", "--", name, url])
            }
            Operation::RemoveRemote(name) => {
                valid_ref(name)?;
                args(&["remote", "remove", name])
            }
            Operation::Pull => {
                remote = true;
                args(&["pull", "--ff-only", "--no-rebase"])
            }
            Operation::Push => {
                remote = true;
                args(&["push"])
            }
            Operation::Publish {
                remote: name,
                branch,
            } => {
                valid_ref(name)?;
                valid_ref(branch)?;
                remote = true;
                args(&[
                    "push",
                    "--set-upstream",
                    name,
                    &format!("refs/heads/{branch}:refs/heads/{branch}"),
                ])
            }
            Operation::Discard {
                selection,
                expected,
                backup,
            } => {
                if selection.kind != DiffKind::Working {
                    return Err("Only tracked working changes can be discarded.".into());
                }
                if self.diff(root, selection)?.patch != *expected {
                    return Err("The file changed. Refresh and review it before discarding.".into());
                }
                let path = safe_path(root, &selection.path)?;
                let saved = backup_file(&path, backup)?;
                self.checked(
                    root,
                    &path_args(
                        &["restore", "--worktree"],
                        std::slice::from_ref(&selection.path),
                    )?,
                    None,
                    false,
                )?;
                return Ok(saved.map_or_else(
                    || "Working file restored from the index.".into(),
                    |path| format!("Working file restored. Backup: {}", path.display()),
                ));
            }
            Operation::Resolve {
                selection,
                ours,
                expected,
                backup,
            } => {
                if selection.kind != DiffKind::Conflict {
                    return Err("This file is not an unresolved conflict.".into());
                }
                let current = self.diff(root, selection)?;
                if current
                    .conflict
                    .as_ref()
                    .is_none_or(|c| &c.expected != expected)
                {
                    return Err(
                        "The conflict file or index changed. Refresh and review both sides again."
                            .into(),
                    );
                }
                let path = safe_path(root, &selection.path)?;
                let saved = backup_file(&path, backup)?;
                self.checked(
                    root,
                    &path_args(
                        &["checkout", if *ours { "--ours" } else { "--theirs" }],
                        std::slice::from_ref(&selection.path),
                    )?,
                    None,
                    false,
                )?;
                return Ok(format!("Conflict side restored. Review, save and stage the file to mark it resolved.{}", saved.map_or(String::new(), |p| format!(" Backup: {}",p.display()))));
            }
            Operation::Init => args(&["init"]),
        };
        self.operation_output(root, &arguments, input.as_deref(), remote)
    }
    fn operation_output(
        &self,
        root: &Path,
        arguments: &[OsString],
        input: Option<&[u8]>,
        remote: bool,
    ) -> Result<String> {
        let output = self.run(root, arguments, input, remote)?;
        if !output.success {
            return Err(output.error());
        }
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            output.stderr
        )
        .trim()
        .to_owned();
        Ok(if text.is_empty() {
            "Git operation completed.".into()
        } else {
            text
        })
    }
    fn path_operation(&self, root: &Path, prefix: &[&str], paths: &[PathBuf]) -> Result<String> {
        if paths.is_empty() {
            return Err("No files selected.".into());
        }
        let mut input = Vec::new();
        for path in paths {
            validate_relative(path)?;
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                input.extend_from_slice(path.as_os_str().as_bytes());
            }
            #[cfg(not(unix))]
            input.extend_from_slice(
                path.to_str()
                    .ok_or("This path cannot be passed to Git as UTF-8.")?
                    .as_bytes(),
            );
            input.push(0);
        }
        let mut arguments = args(prefix);
        arguments.extend(args(&["--pathspec-from-file=-", "--pathspec-file-nul"]));
        self.operation_output(root, &arguments, Some(&input), false)
    }
    pub fn clone_repository(&self, url: &str, destination: &Path) -> Result<PathBuf> {
        let parent = destination
            .parent()
            .ok_or("Enter a full destination path.")?;
        let name = destination.file_name().ok_or("Enter a new folder name.")?;
        if !destination.is_absolute() || destination.exists() {
            return Err("Choose an absolute path to a new, nonexistent folder.".into());
        }
        if url.trim().is_empty() {
            return Err("Enter a repository URL or local path.".into());
        }
        self.checked(
            parent,
            &["clone".into(), "--".into(), url.into(), name.into()],
            None,
            true,
        )?;
        self.discover(destination)
    }
    /// A selected repository is an exact working-tree root, never a directory
    /// from which Git may silently discover a different enclosing repository.
    fn require_root(&self, root: &Path) -> Result<()> {
        let expected = root
            .canonicalize()
            .map_err(|e| format!("Cannot access the selected repository: {e}"))?;
        let actual = self
            .discover(root)
            .map_err(|e| format!("Cannot access the selected repository: {e}"))?;
        if actual != expected {
            return Err(format!("The selected repository at {} is no longer available. Open the repository again before running Git actions.",root.display()));
        }
        Ok(())
    }
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
fn path_args(prefix: &[&str], paths: &[PathBuf]) -> Result<Vec<OsString>> {
    if paths.is_empty() {
        return Err("No files selected.".into());
    }
    let mut out = args(prefix);
    out.push("--".into());
    for path in paths {
        validate_relative(path)?;
        out.push(path.as_os_str().into());
    }
    Ok(out)
}
fn validate_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Git paths must stay inside the repository.".into());
    }
    Ok(())
}
fn safe_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    validate_relative(relative)?;
    let path = root.join(relative);
    let mut parent = path.parent().ok_or("Invalid file path.")?;
    loop {
        match parent.canonicalize() {
            Ok(parent) => {
                if !parent.starts_with(root.canonicalize().map_err(|e| e.to_string())?) {
                    return Err("This file resolves outside the repository.".into());
                }
                break;
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    && parent.symlink_metadata().is_err() =>
            {
                parent = parent.parent().ok_or("Invalid file parent.")?
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(path)
}
fn backup_file(path: &Path, directory: &Path) -> Result<Option<PathBuf>> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(meta) if !meta.is_file() => return Err("Only regular files can be replaced here. Use Git in a terminal for links and submodules.".into()),
        Ok(_) => (),
    }
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let mut copy = tempfile::Builder::new()
        .prefix("git-file-")
        .tempfile_in(directory)
        .map_err(|e| e.to_string())?;
    let mut source = std::fs::File::open(path).map_err(|e| e.to_string())?;
    std::io::copy(&mut source, &mut copy).map_err(|e| e.to_string())?;
    copy.as_file().sync_all().map_err(|e| e.to_string())?;
    let (_, saved) = copy.keep().map_err(|e| e.to_string())?;
    Ok(Some(saved))
}
fn check_oid(oid: &str) -> Result<()> {
    if !(oid.len() == 40 || oid.len() == 64) || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid Git object ID.".into());
    }
    Ok(())
}
fn valid_ref(name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('-') || name.chars().any(char::is_control) {
        return Err("Invalid branch or remote name.".into());
    }
    Ok(())
}
#[cfg(unix)]
fn os_path(bytes: &[u8]) -> Result<OsString> {
    use std::os::unix::ffi::OsStringExt;
    Ok(OsString::from_vec(bytes.to_vec()))
}
#[cfg(not(unix))]
fn os_path(bytes: &[u8]) -> Result<OsString> {
    String::from_utf8(bytes.to_vec())
        .map(OsString::from)
        .map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub root: PathBuf,
    pub branch: String,
    pub oid: String,
    pub upstream: String,
    pub ahead: usize,
    pub behind: usize,
    pub unborn: bool,
    pub files: Vec<FileStatus>,
    pub branches: Vec<Branch>,
    pub commits: Vec<Commit>,
    pub graph: Vec<GraphRow>,
    pub stashes: Vec<Stash>,
    pub remotes: Vec<String>,
    pub operation: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStatus {
    pub path: PathBuf,
    pub original: Option<PathBuf>,
    pub index: char,
    pub worktree: char,
    pub conflict: bool,
}
impl FileStatus {
    pub fn untracked(&self) -> bool {
        self.index == '?'
    }
    pub fn staged(&self) -> bool {
        !self.conflict && !matches!(self.index, ' ' | '.' | '?')
    }
    pub fn working(&self) -> bool {
        !self.conflict && (self.untracked() || !matches!(self.worktree, ' ' | '.'))
    }
    pub fn paths(&self) -> Vec<PathBuf> {
        let mut p = vec![self.path.clone()];
        if let Some(old) = &self.original {
            p.push(old.clone());
        }
        p
    }
}
#[derive(Clone, Debug)]
pub struct Branch {
    pub name: String,
    pub reference: String,
    pub oid: String,
    pub upstream: String,
    pub current: bool,
    pub remote: bool,
}
#[derive(Clone, Debug)]
pub struct Commit {
    pub oid: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    pub refs: String,
    pub subject: String,
}
#[derive(Clone, Debug)]
pub struct Stash {
    pub oid: String,
    pub name: String,
    pub subject: String,
}
/// Lanes follow actual parent IDs, including forks and merges. Edges meet at
/// the commit node; ongoing lanes continue through the complete row.
#[derive(Clone, Debug)]
pub struct GraphRow {
    pub node: usize,
    pub width: usize,
    pub edges: Vec<(usize, usize, bool)>,
    pub incoming: bool,
}
pub fn history_graph(commits: &[Commit]) -> Vec<GraphRow> {
    let mut lanes: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for commit in commits {
        let incoming = lanes.contains(&commit.oid);
        let node = lanes
            .iter()
            .position(|id| id == &commit.oid)
            .unwrap_or_else(|| {
                lanes.push(commit.oid.clone());
                lanes.len() - 1
            });
        let before = lanes.clone();
        lanes.remove(node);
        for (offset, parent) in commit.parents.iter().enumerate() {
            if !lanes.contains(parent) {
                lanes.insert((node + offset).min(lanes.len()), parent.clone());
            }
        }
        let mut edges = Vec::new();
        for (from, oid) in before.iter().enumerate() {
            if from != node {
                if let Some(to) = lanes.iter().position(|id| id == oid) {
                    edges.push((from, to, false));
                }
            }
        }
        for parent in &commit.parents {
            if let Some(to) = lanes.iter().position(|id| id == parent) {
                edges.push((node, to, true));
            }
        }
        rows.push(GraphRow {
            node,
            width: before.len().max(lanes.len()),
            edges,
            incoming,
        });
    }
    rows
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffKind {
    Working,
    Staged,
    Untracked,
    Commit(String),
    Conflict,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub path: PathBuf,
    pub kind: DiffKind,
}
#[derive(Clone, Debug)]
pub enum Operation {
    Stage(Vec<PathBuf>),
    Unstage(Vec<PathBuf>),
    Hunk {
        selection: Selection,
        expected: String,
        hunk: usize,
    },
    Commit {
        message: String,
        amend: bool,
    },
    Switch(String),
    CreateBranch(String),
    TrackBranch {
        local: String,
        remote: String,
    },
    DeleteBranch(String),
    Merge(String),
    CherryPick(String),
    Revert(String),
    Continue(String),
    Abort(String),
    Stash(String),
    ApplyStash(String),
    DropStash {
        name: String,
        oid: String,
    },
    Fetch,
    AddRemote {
        name: String,
        url: String,
    },
    RemoveRemote(String),
    Pull,
    Push,
    Publish {
        remote: String,
        branch: String,
    },
    Discard {
        selection: Selection,
        expected: String,
        backup: PathBuf,
    },
    Resolve {
        selection: Selection,
        ours: bool,
        expected: String,
        backup: PathBuf,
    },
    Init,
}
impl Operation {
    pub fn changes_worktree(&self) -> bool {
        matches!(
            self,
            Self::Switch(_)
                | Self::CreateBranch(_)
                | Self::TrackBranch { .. }
                | Self::Merge(_)
                | Self::CherryPick(_)
                | Self::Revert(_)
                | Self::Continue(_)
                | Self::Abort(_)
                | Self::Stash(_)
                | Self::ApplyStash(_)
                | Self::Pull
                | Self::Discard { .. }
                | Self::Resolve { .. }
        )
    }
}

fn parse_status(raw: &[u8]) -> Result<Snapshot> {
    let mut snapshot = Snapshot::default();
    let mut records = raw.split(|b| *b == 0);
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if record[0] == b'#' {
            let text = String::from_utf8_lossy(record);
            if let Some(v) = text.strip_prefix("# branch.head ") {
                snapshot.branch = v.into();
            }
            if let Some(v) = text.strip_prefix("# branch.oid ") {
                snapshot.oid = v.into();
                snapshot.unborn = v == "(initial)";
            }
            if let Some(v) = text.strip_prefix("# branch.upstream ") {
                snapshot.upstream = v.into();
            }
            if let Some(v) = text.strip_prefix("# branch.ab ") {
                let mut parts = v.split_whitespace();
                snapshot.ahead = parts
                    .next()
                    .and_then(|v| v.trim_start_matches('+').parse().ok())
                    .unwrap_or(0);
                snapshot.behind = parts
                    .next()
                    .and_then(|v| v.trim_start_matches('-').parse().ok())
                    .unwrap_or(0);
            }
            continue;
        }
        if record.starts_with(b"? ") {
            snapshot.files.push(FileStatus {
                path: PathBuf::from(os_path(&record[2..])?),
                original: None,
                index: '?',
                worktree: '?',
                conflict: false,
            });
            continue;
        }
        let count = match record[0] {
            b'1' => 9,
            b'2' => 10,
            b'u' => 11,
            _ => continue,
        };
        let fields: Vec<_> = record.splitn(count, |b| *b == b' ').collect();
        if fields.len() != count || fields[1].len() != 2 {
            return Err("Invalid Git status response.".into());
        }
        let original = if record[0] == b'2' {
            Some(PathBuf::from(os_path(
                records.next().ok_or("Incomplete rename record.")?,
            )?))
        } else {
            None
        };
        snapshot.files.push(FileStatus {
            path: PathBuf::from(os_path(fields[count - 1])?),
            original,
            index: fields[1][0] as char,
            worktree: fields[1][1] as char,
            conflict: record[0] == b'u',
        });
    }
    Ok(snapshot)
}

#[derive(Clone, Debug)]
pub struct DiffLine {
    pub text: String,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub kind: char,
    pub hunk: Option<usize>,
}
#[derive(Clone, Debug, Default)]
pub struct Diff {
    pub patch: String,
    pub lines: Vec<DiffLine>,
    pub hunks: Vec<(usize, usize)>,
    pub applicable: bool,
    pub additions: usize,
    pub deletions: usize,
    pub conflict: Option<Conflict>,
    pub split_rows: Vec<(Option<usize>, Option<usize>)>,
}
#[derive(Clone, Debug)]
pub struct Conflict {
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
    pub expected: String,
}
impl Diff {
    fn message(message: &str) -> Self {
        Self::parse(message.into(), false)
    }
    pub fn parse(patch: String, applicable: bool) -> Self {
        let mut result = Self {
            patch,
            applicable,
            ..Default::default()
        };
        let (mut old, mut new, mut offset, mut hunk) = (0, 0, 0, None::<usize>);
        for line in result.patch.split_inclusive('\n') {
            let text = line.trim_end_matches('\n').trim_end_matches('\r');
            let mut kind = 'i';
            let mut old_line = None;
            let mut new_line = None;
            if text.starts_with("@@ ") {
                if let Some(index) = hunk {
                    result.hunks[index].1 = offset;
                }
                hunk = Some(result.hunks.len());
                result.hunks.push((offset, result.patch.len()));
                let parts: Vec<_> = text.split_whitespace().collect();
                old = parts
                    .get(1)
                    .and_then(|s| s.trim_start_matches('-').split(',').next()?.parse().ok())
                    .unwrap_or(1);
                new = parts
                    .get(2)
                    .and_then(|s| s.trim_start_matches('+').split(',').next()?.parse().ok())
                    .unwrap_or(1);
                kind = '@';
            } else if hunk.is_some() {
                kind = text.chars().next().unwrap_or(' ');
                match kind {
                    '+' => {
                        new_line = Some(new);
                        new += 1;
                        result.additions += 1;
                    }
                    '-' => {
                        old_line = Some(old);
                        old += 1;
                        result.deletions += 1;
                    }
                    ' ' => {
                        old_line = Some(old);
                        new_line = Some(new);
                        old += 1;
                        new += 1;
                    }
                    _ => (),
                }
            }
            if hunk.is_some()
                || !(text.starts_with("diff --git ")
                    || text.starts_with("index ")
                    || text.starts_with("--- ")
                    || text.starts_with("+++ "))
            {
                result.lines.push(DiffLine {
                    text: text.into(),
                    old: old_line,
                    new: new_line,
                    kind,
                    hunk,
                });
            }
            offset += line.len();
        }
        let mut i = 0;
        while i < result.lines.len() {
            let start = i;
            if result.lines[i].kind == '-' {
                while i < result.lines.len() && result.lines[i].kind == '-' {
                    i += 1;
                }
                let middle = i;
                while i < result.lines.len() && result.lines[i].kind == '+' {
                    i += 1;
                }
                for offset in 0..(middle - start).max(i - middle) {
                    result.split_rows.push((
                        (start + offset < middle).then_some(start + offset),
                        (middle + offset < i).then_some(middle + offset),
                    ));
                }
            } else if result.lines[i].kind == '+' {
                result.split_rows.push((None, Some(i)));
                i += 1;
            } else {
                result.split_rows.push((Some(i), Some(i)));
                i += 1;
            }
        }
        result
    }
    pub fn hunk_patch(&self, index: usize) -> Option<String> {
        if !self.applicable {
            return None;
        }
        let (start, end) = *self.hunks.get(index)?;
        let header_end = self.hunks.first()?.0;
        let header = &self.patch[..header_end];
        // Creation/deletion and mode/rename changes must be staged as a whole.
        if !header.contains("--- a/")
            || !header.contains("+++ b/")
            || header.contains("mode ")
            || header.contains("rename ")
        {
            return None;
        }
        Some(format!("{}{}", header, &self.patch[start..end]))
    }
}
