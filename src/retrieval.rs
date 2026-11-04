//! Path-only repository discovery and bounded retrieval. No full-tree source upload.
use crate::workspace::{Edit, Workspace};
use serde_json::{Value, json};
use std::{collections::BTreeSet, io};

pub const FILE_PAGE: usize = 64;
pub const SEARCH_PAGE: usize = 32;
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn visible(workspace: &Workspace, proposals: &[Edit]) -> BTreeSet<String> {
    workspace
        .files
        .iter()
        .cloned()
        .chain(proposals.iter().map(|p| p.path.clone()))
        .collect()
}
pub fn ranked(workspace: &Workspace, task: &str) -> Vec<String> {
    let words: Vec<_> = task
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.chars().count() >= 3)
        .take(128)
        .map(str::to_lowercase)
        .collect();
    let mut paths: Vec<_> = workspace
        .files
        .iter()
        .map(|path| {
            let lower = path.to_lowercase();
            let name = lower.rsplit('/').next().unwrap_or(&lower);
            let score = words
                .iter()
                .map(|word| {
                    if name.contains(word) {
                        8
                    } else if lower.contains(word) {
                        2
                    } else {
                        0
                    }
                })
                .sum::<usize>()
                + usize::from(matches!(
                    path.as_str(),
                    "Cargo.toml"
                        | "package.json"
                        | "pyproject.toml"
                        | "AGENTS.md"
                        | "pom.xml"
                        | "build.gradle.kts"
                ));
            (std::cmp::Reverse(score), path.clone())
        })
        .collect();
    paths.sort();
    paths.into_iter().map(|(_, p)| p).collect()
}
pub fn list(
    workspace: &Workspace,
    proposals: &[Edit],
    prefix: &str,
    offset: usize,
) -> io::Result<Value> {
    if prefix.len() > 256 || prefix.chars().any(char::is_control) {
        return Err(io::Error::other("Invalid list prefix"));
    }
    let paths: Vec<_> = visible(workspace, proposals)
        .into_iter()
        .filter(|p| p.starts_with(prefix))
        .collect();
    if offset > paths.len() {
        return Err(io::Error::other("List offset exceeds index"));
    }
    let end = (offset + FILE_PAGE).min(paths.len());
    Ok(
        json!({"files":paths[offset..end],"total":paths.len(),"next_offset":(end<paths.len()).then_some(end),"incomplete":workspace.truncated || end<paths.len(),"index_incomplete":workspace.truncated}),
    )
}
pub fn read(workspace: &mut Workspace, proposals: &[Edit], path: &str) -> io::Result<String> {
    match proposals.iter().find(|p| p.path == path) {
        Some(pending) => workspace.read_pending(pending),
        None => workspace.read_tool(path),
    }
}
pub fn range(
    workspace: &mut Workspace,
    proposals: &[Edit],
    path: &str,
    start_line: usize,
    count: usize,
) -> io::Result<Value> {
    if start_line == 0 || start_line > 2 * 1024 * 1024 + 1 || !(1..=200).contains(&count) {
        return Err(io::Error::other(
            "read_range needs line >=1 and 1..200 lines",
        ));
    }
    let text = if proposals.iter().any(|p| p.path == path) {
        read(workspace, proposals, path)?
    } else {
        workspace.read_snapshot(path)?
    };
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    if start_line > lines.len().saturating_add(1) {
        return Err(io::Error::other("Line exceeds file"));
    }
    let mut content = String::new();
    let mut end = start_line - 1;
    for line in lines.iter().skip(start_line - 1).take(count) {
        if content.len() + line.len() > 8192 {
            break;
        }
        content.push_str(line);
        end += 1;
    }
    if end < start_line && start_line <= lines.len() {
        return Err(io::Error::other(
            "Line exceeds 8 KiB range result; read_file is required",
        ));
    }
    Ok(
        json!({"path":path,"start_line":start_line,"end_line":end,"total_lines":lines.len(),
        "next_line":(end<lines.len()).then_some(end+1),"content":content,"source_sha256":sha256(text.as_bytes()),
        "scoped_instructions":workspace.scoped_instructions(path)?}),
    )
}
pub fn search(
    workspace: &mut Workspace,
    proposals: &[Edit],
    query: &str,
    offset: usize,
) -> io::Result<Value> {
    if query.is_empty() || query.len() > 256 {
        return Err(io::Error::other("Запрос поиска: 1–256 байт."));
    }
    let paths: Vec<_> = visible(workspace, proposals).into_iter().collect();
    if offset > paths.len() {
        return Err(io::Error::other("Search offset exceeds index"));
    }
    let end = (offset + SEARCH_PAGE).min(paths.len());
    let mut matches = Vec::new();
    let mut skipped = 0;
    let mut omitted = 0;
    for path in &paths[offset..end] {
        match read(workspace, proposals, path) {
            Ok(text) => {
                let digest = sha256(text.as_bytes());
                for (line, text) in text
                    .lines()
                    .enumerate()
                    .filter(|(_, line)| line.contains(query))
                {
                    if matches.len() == 32 {
                        omitted += 1;
                        continue;
                    }
                    matches.push(json!({"path":path,"line":line+1,"text":text.chars().take(200).collect::<String>(),"source_sha256":digest}));
                }
            }
            Err(_) => skipped += 1,
        }
    }
    Ok(
        json!({"matches":matches,"incomplete":workspace.truncated || end<paths.len() || skipped>0 || omitted>0,
        "skipped":skipped,"omitted_matches":omitted,"next_offset":(end<paths.len()).then_some(end),"files_scanned":end-offset}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_beyond_old_limit_and_pages_without_read_budget_starvation() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..300 {
            std::fs::write(
                dir.path().join(format!("file{i:03}.rs")),
                format!("fn marker{i}() {{}}\r\n"),
            )
            .unwrap();
        }
        std::fs::write(dir.path().join(".gitignore"), "private/\n").unwrap();
        std::fs::create_dir(dir.path().join("private")).unwrap();
        std::fs::write(dir.path().join("private/password.txt"), "secret").unwrap();
        let mut workspace = Workspace::open(dir.path(), &[]).unwrap();
        assert_eq!(workspace.files.len(), 300);
        assert!(!workspace.truncated);
        let mut found = BTreeSet::new();
        let mut offset = 0;
        loop {
            let page = list(&workspace, &[], "", offset).unwrap();
            for path in page["files"].as_array().unwrap() {
                found.insert(path.as_str().unwrap().to_owned());
            }
            match page["next_offset"].as_u64() {
                Some(next) => offset = next as usize,
                None => break,
            }
        }
        assert_eq!(found.len(), 300);
        let baseline = workspace.bytes_read;
        for _ in 0..40 {
            assert_eq!(
                search(&mut workspace, &[], "marker299", 288).unwrap()["matches"][0]["path"],
                "file299.rs"
            );
        }
        assert_eq!(workspace.bytes_read, baseline);
        assert_eq!(
            ranked(&workspace, "fix marker in file299.rs")[0],
            "file299.rs"
        );
        assert!(workspace.read_tool("private/password.txt").is_err());
    }
    #[test]
    fn range_reads_large_text_without_exposing_full_content_or_expanding_edit_limits() {
        let dir = tempfile::tempdir().unwrap();
        let line = format!("{}\r\n", "x".repeat(118));
        std::fs::write(dir.path().join("large.rs"), line.repeat(3000)).unwrap();
        let mut workspace = Workspace::open(dir.path(), &[]).unwrap();
        assert!(workspace.read_tool("large.rs").is_err());
        let view = range(&mut workspace, &[], "large.rs", 2900, 10).unwrap();
        assert_eq!(view["content"].as_str().unwrap(), line.repeat(10));
        assert_eq!(view["next_line"], 2910);
        assert!(workspace.propose("large.rs", "x", "y").is_err());
    }
    #[test]
    fn ranges_preserve_crlf_and_return_scoped_provenance_and_pending_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "Root guidance").unwrap();
        std::fs::write(dir.path().join("src/AGENTS.md"), "Nested guidance").unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "one\r\ntwo\r\nthree\r\n").unwrap();
        let mut workspace = Workspace::open(dir.path(), &["src/a.rs".into()]).unwrap();
        let edit = workspace.propose("src/a.rs", "two", "changed").unwrap();
        let result = range(&mut workspace, &[edit], "src/a.rs", 2, 1).unwrap();
        assert_eq!(result["content"], "changed\r\n");
        assert_eq!(result["next_line"], 3);
        assert_eq!(result["scoped_instructions"].as_array().unwrap().len(), 2);
        assert!(range(&mut workspace, &[], "../a.rs", 1, 2).is_err());
    }
}
