//! Import of Graphify (<https://github.com/Graphify-Labs/graphify>, Apache-2.0) `graph.json`.
//!
//! Only the documented node-link shape is read (`nodes[].id/label/source_file/source_location/
//! community`, `links[].source/target/relation/confidence`). Lyra neither runs Graphify's code
//! nor copies it; the file is treated as untrusted data and bounded. Edges keep the original
//! relation name and the INFERRED flag so the UI can tell read-from-source from derived links.
use crate::codegraph::{Edge, EdgeKind, Graph, Node, NodeKind};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    io,
};

const MAX_NODES: usize = 200_000;
const MAX_LINKS: usize = 600_000;

#[derive(Deserialize)]
struct RawGraph {
    #[serde(default)]
    nodes: Vec<RawNode>,
    #[serde(default, alias = "edges")]
    links: Vec<RawLink>,
}

#[derive(Deserialize)]
struct RawNode {
    id: serde_json::Value,
    #[serde(default)]
    label: String,
    #[serde(default)]
    source_file: String,
    #[serde(default)]
    source_location: String,
    #[serde(default)]
    community: Option<i64>,
}

#[derive(Deserialize)]
struct RawLink {
    source: serde_json::Value,
    target: serde_json::Value,
    #[serde(default)]
    relation: String,
    #[serde(default)]
    confidence: String,
}

fn key(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn clean_path(raw: &str) -> Option<String> {
    let p = raw.replace('\\', "/");
    let p = p.trim_start_matches("./");
    (!p.is_empty()
        && p.len() <= 1024
        && !p.starts_with('/')
        && !p.split('/').any(|s| s == "..")
        && !p.chars().any(char::is_control))
    .then(|| p.to_owned())
}

fn line_of(location: &str) -> u32 {
    location
        .trim_start_matches(['L', 'l'])
        .split(['-', ':'])
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn kind_of(relation: &str) -> EdgeKind {
    match relation {
        "contains" | "method" | "defines" => EdgeKind::Defines,
        "imports" | "imports_from" | "import" => EdgeKind::Imports,
        _ => EdgeKind::References,
    }
}

pub fn import(bytes: &[u8]) -> io::Result<Graph> {
    let raw: RawGraph = serde_json::from_slice(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid Graphify graph.json"))?;
    if raw.nodes.len() > MAX_NODES || raw.links.len() > MAX_LINKS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Graphify graph is too large",
        ));
    }
    let mut graph = Graph {
        schema_version: 1,
        ..Graph::default()
    };
    // File nodes: one per distinct source_file.
    let mut file_ids: BTreeMap<String, usize> = BTreeMap::new();
    for n in &raw.nodes {
        if let Some(path) = clean_path(&n.source_file)
            && !file_ids.contains_key(&path)
        {
            let id = graph.nodes.len();
            file_ids.insert(path.clone(), id);
            graph.nodes.push(Node {
                id,
                kind: NodeKind::File,
                label: path.rsplit('/').next().unwrap_or(&path).to_owned(),
                file: path,
                line: 0,
                community: 0,
                degree: 0,
            });
        }
    }
    // Graphify's own file-level nodes map onto the file nodes; everything else is a symbol.
    let mut mapped: HashMap<String, usize> = HashMap::new();
    let mut community_votes: HashMap<usize, BTreeMap<i64, u32>> = HashMap::new();
    for n in &raw.nodes {
        let Some(path) = clean_path(&n.source_file) else {
            continue;
        };
        let file = file_ids[&path];
        let is_file_node = n.label == graph.nodes[file].label && line_of(&n.source_location) <= 1;
        let id = if is_file_node {
            file
        } else {
            let id = graph.nodes.len();
            graph.nodes.push(Node {
                id,
                kind: NodeKind::Symbol,
                label: n.label.chars().take(200).collect(),
                file: path,
                line: line_of(&n.source_location),
                community: 0,
                degree: 0,
            });
            graph.edges.push(Edge {
                from: file,
                to: id,
                kind: EdgeKind::Defines,
                relation: Some("contains".into()),
                inferred: false,
            });
            id
        };
        if let Some(c) = n.community {
            *community_votes
                .entry(file)
                .or_default()
                .entry(c)
                .or_default() += 1;
        }
        mapped.insert(key(&n.id), id);
    }
    for link in &raw.links {
        let (Some(&a), Some(&b)) = (
            mapped.get(&key(&link.source)),
            mapped.get(&key(&link.target)),
        ) else {
            continue;
        };
        if a == b {
            continue;
        }
        let kind = kind_of(&link.relation);
        // `contains` between a file node and its symbols is already synthesized above.
        if kind == EdgeKind::Defines && graph.nodes[a].file == graph.nodes[b].file {
            continue;
        }
        graph.edges.push(Edge {
            from: a,
            to: b,
            kind,
            relation: Some(link.relation.chars().take(40).collect()),
            inferred: link.confidence.eq_ignore_ascii_case("INFERRED"),
        });
    }
    for e in &graph.edges {
        graph.nodes[e.from].degree += 1;
        graph.nodes[e.to].degree += 1;
    }
    // Communities come from the producer: majority vote per file, symbols inherit.
    let mut renumber: BTreeMap<i64, u32> = BTreeMap::new();
    for (i, node) in graph.nodes.iter_mut().enumerate() {
        if node.kind == NodeKind::File {
            let c = community_votes
                .get(&i)
                .and_then(|v| v.iter().max_by_key(|(c, n)| (**n, std::cmp::Reverse(**c))))
                .map(|(c, _)| *c)
                .unwrap_or(-1);
            let next = renumber.len() as u32;
            node.community = *renumber.entry(c).or_insert(next);
        }
    }
    let owner: HashMap<usize, u32> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::File)
        .map(|n| (file_ids[&n.file], n.community))
        .collect();
    for node in graph
        .nodes
        .iter_mut()
        .filter(|n| n.kind == NodeKind::Symbol)
    {
        node.community = owner[&file_ids[&node.file]];
    }
    graph.communities = renumber.len() as u32;
    Ok(graph)
}

/// Run `graphify extract <repo> --code-only` with every LLM backend disabled, in `repo`.
/// No shell; bounded by a timeout. The executable is the user's explicit choice.
pub fn run_extract(
    exe: &str,
    repo: &std::path::Path,
    timeout: std::time::Duration,
) -> io::Result<()> {
    use std::process::{Command, Stdio};
    if exe.is_empty() || exe.contains(['\0', '\n']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid graphify executable",
        ));
    }
    let mut child = Command::new(exe)
        .args(["extract", ".", "--code-only"])
        .current_dir(repo)
        .env("GRAPHIFY_BACKEND", "none")
        .env("GRAPHIFY_QUERY_LOG_DISABLE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| io::Error::new(e.kind(), format!("Cannot start '{exe}': {e}")))?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            let mut text = String::new();
            if let Some(mut stderr) = child.stderr.take() {
                use std::io::Read;
                let _ = stderr.by_ref().take(2000).read_to_string(&mut text);
            }
            return Err(io::Error::other(format!(
                "graphify failed ({status}): {text}"
            )));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "graphify timed out",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"directed":false,"multigraph":false,"graph":{},
      "nodes":[
        {"label":"app.py","file_type":"code","source_file":"pkg/app.py","source_location":"L1","id":"app","community":1},
        {"label":"run()","file_type":"code","source_file":"pkg/app.py","source_location":"L5","id":"app_run","community":1},
        {"label":"core.py","file_type":"code","source_file":"pkg/core.py","source_location":"L1","id":"core","community":2},
        {"label":"Engine","file_type":"code","source_file":"pkg/core.py","source_location":"L3","id":"core_engine","community":2},
        {"label":"evil","source_file":"../../etc/passwd","source_location":"L1","id":"evil"}
      ],
      "links":[
        {"relation":"contains","confidence":"EXTRACTED","source":"app","target":"app_run"},
        {"relation":"imports_from","confidence":"EXTRACTED","source":"app","target":"core"},
        {"relation":"calls","confidence":"INFERRED","source":"app_run","target":"core_engine"},
        {"relation":"calls","source":"app_run","target":"missing"}
      ]}"#;

    #[test]
    fn imports_files_symbols_relations_and_communities_and_drops_unsafe_paths() {
        let g = import(SAMPLE.as_bytes()).unwrap();
        let files: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::File)
            .map(|n| n.file.as_str())
            .collect();
        assert_eq!(files, ["pkg/app.py", "pkg/core.py"]);
        assert!(g.nodes.iter().any(|n| n.label == "run()" && n.line == 5));
        assert!(g.edges.iter().any(|e| e.kind == EdgeKind::Imports));
        let call = g
            .edges
            .iter()
            .find(|e| e.relation.as_deref() == Some("calls"))
            .unwrap();
        assert!(call.inferred && call.kind == EdgeKind::References);
        assert_eq!(g.communities, 2);
        // The call edge makes core.py's dependents include app.py.
        let radius = g.impact(&["pkg/core.py".to_owned()], 1);
        assert_eq!(radius["pkg/app.py"], 1);
        assert!(g.nodes.iter().all(|n| !n.file.contains("passwd")));
    }

    #[cfg(unix)]
    #[test]
    fn extract_runner_uses_offline_flags_and_bounds_runtime() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("fake-graphify");
        std::fs::write(
            &exe,
            "#!/bin/sh\n[ \"$1 $2 $3\" = \"extract . --code-only\" ] && [ \"$GRAPHIFY_BACKEND\" = none ] || exit 7\nmkdir -p graphify-out && echo '{}' > graphify-out/graph.json\n",
        )
        .unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let repo = tempfile::tempdir().unwrap();
        run_extract(
            exe.to_str().unwrap(),
            repo.path(),
            std::time::Duration::from_secs(10),
        )
        .unwrap();
        assert!(repo.path().join("graphify-out/graph.json").exists());
        let slow = dir.path().join("slow");
        std::fs::write(&slow, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = run_extract(
            slow.to_str().unwrap(),
            repo.path(),
            std::time::Duration::from_millis(300),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            run_extract(
                "/nonexistent/graphify",
                repo.path(),
                std::time::Duration::from_secs(1)
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_non_graph_json_and_oversized_graphs() {
        assert!(import(b"[1,2]").is_err());
        assert!(import(b"not json").is_err());
    }
}
