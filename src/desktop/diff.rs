//! Line diff for the proposal viewer. Pure function, bounded work.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Same,
    Added,
    Removed,
}

const MAX_CELLS: usize = 4_000_000;

/// Line-based diff using common prefix/suffix trimming and an LCS table for the middle.
/// Very large inputs fall back to "everything removed, everything added".
pub fn lines<'a>(old: &'a str, new: &'a str) -> Vec<(Tag, &'a str)> {
    let a: Vec<&str> = old.split_inclusive('\n').collect();
    let b: Vec<&str> = new.split_inclusive('\n').collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let mut out: Vec<(Tag, &str)> = a[..prefix].iter().map(|l| (Tag::Same, *l)).collect();
    if am.len().saturating_mul(bm.len()) > MAX_CELLS {
        out.extend(am.iter().map(|l| (Tag::Removed, *l)));
        out.extend(bm.iter().map(|l| (Tag::Added, *l)));
    } else {
        let width = bm.len() + 1;
        let mut table = vec![0u32; (am.len() + 1) * width];
        for i in (0..am.len()).rev() {
            for j in (0..bm.len()).rev() {
                table[i * width + j] = if am[i] == bm[j] {
                    table[(i + 1) * width + j + 1] + 1
                } else {
                    table[(i + 1) * width + j].max(table[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < am.len() && j < bm.len() {
            if am[i] == bm[j] {
                out.push((Tag::Same, am[i]));
                i += 1;
                j += 1;
            } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
                out.push((Tag::Removed, am[i]));
                i += 1;
            } else {
                out.push((Tag::Added, bm[j]));
                j += 1;
            }
        }
        out.extend(am[i..].iter().map(|l| (Tag::Removed, *l)));
        out.extend(bm[j..].iter().map(|l| (Tag::Added, *l)));
    }
    out.extend(a[a.len() - suffix..].iter().map(|l| (Tag::Same, *l)));
    out
}

pub fn stats(diff: &[(Tag, &str)]) -> (usize, usize) {
    (
        diff.iter().filter(|(t, _)| *t == Tag::Added).count(),
        diff.iter().filter(|(t, _)| *t == Tag::Removed).count(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_marks_only_changed_lines_and_preserves_crlf_text() {
        let d = lines("a\r\nb\r\nc\r\n", "a\r\nB\r\nc\r\n");
        assert_eq!(
            d,
            vec![
                (Tag::Same, "a\r\n"),
                (Tag::Removed, "b\r\n"),
                (Tag::Added, "B\r\n"),
                (Tag::Same, "c\r\n")
            ]
        );
        assert_eq!(stats(&d), (1, 1));
    }

    #[test]
    fn diff_handles_new_files_deletions_and_huge_inputs() {
        assert_eq!(stats(&lines("", "x\ny\n")), (2, 0));
        assert_eq!(stats(&lines("x\ny\n", "")), (0, 2));
        assert!(lines("same", "same").iter().all(|(t, _)| *t == Tag::Same));
        let big_a = "a\n".repeat(3000) + "left\n";
        let big_b = "b\n".repeat(3000) + "right\n";
        let d = lines(&big_a, &big_b);
        assert_eq!(stats(&d), (3001, 3001));
    }

    #[test]
    fn diff_reconstructs_both_sides() {
        let (old, new) = ("1\n2\n3\n4\n", "1\n3\n4\n5\n");
        let d = lines(old, new);
        let rebuilt_old: String = d
            .iter()
            .filter(|(t, _)| *t != Tag::Added)
            .map(|(_, l)| *l)
            .collect();
        let rebuilt_new: String = d
            .iter()
            .filter(|(t, _)| *t != Tag::Removed)
            .map(|(_, l)| *l)
            .collect();
        assert_eq!((rebuilt_old.as_str(), rebuilt_new.as_str()), (old, new));
    }
}
