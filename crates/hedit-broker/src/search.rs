//! Literal search over scoped, bounded UTF-8 snapshots. No regex execution.
use crate::workspace::{Budget, Root, MAX_RESULTS};
use std::io;
#[derive(Debug, serde::Serialize)]
pub struct Match {
    pub relative: String,
    pub start: usize,
    pub finish: usize,
    pub excerpt: String,
}
#[derive(Debug, serde::Serialize)]
pub struct Results {
    pub matches: Vec<Match>,
    pub truncated: bool,
}
pub fn search(root: &Root, query: &str, budget: &mut Budget<'_>) -> io::Result<Results> {
    root.require(crate::workspace::Right::Read)?;
    if query.is_empty() || query.len() > 128 || query.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Search needs 1–128 UTF-8 bytes.",
        ));
    }
    let listing = root.discover("", budget)?;
    let mut results = Results {
        matches: Vec::new(),
        truncated: listing.truncated,
    };
    for entry in listing.entries.into_iter().filter(|entry| !entry.directory) {
        budget.check()?;
        let snapshot = match root.read(&entry.relative, budget) {
            Ok(snapshot) => snapshot,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::InvalidData
                        | io::ErrorKind::PermissionDenied
                        | io::ErrorKind::NotFound
                ) =>
            {
                if budget.remaining_bytes() == 0 {
                    results.truncated = true;
                    break;
                }
                continue;
            }
            Err(error) => return Err(error),
        };
        for (number, (start, matched)) in snapshot.text.match_indices(query).enumerate() {
            if number >= 16 || results.matches.len() >= MAX_RESULTS {
                results.truncated = true;
                break;
            }
            let finish = start + matched.len();
            let mut left = start.saturating_sub(80);
            while !snapshot.text.is_char_boundary(left) {
                left += 1;
            }
            let mut right = (finish + 80).min(snapshot.text.len());
            while !snapshot.text.is_char_boundary(right) {
                right -= 1;
            }
            let excerpt = snapshot.text[left..right]
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect::<String>();
            budget.charge(excerpt.len() + entry.relative.len())?;
            results.matches.push(Match {
                relative: entry.relative.clone(),
                start,
                finish,
                excerpt,
            });
        }
        if results.matches.len() >= MAX_RESULTS {
            results.truncated = true;
            break;
        }
    }
    Ok(results)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{Policy, Protected, Right};
    use std::{sync::atomic::AtomicBool, time::Duration};
    #[test]
    fn search_reports_byte_coordinates_caps_results_and_obeys_rights_and_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("unicode.txt"), "λ東京\r\n東京").unwrap();
        let root = Root::open_selected(
            dir.path(),
            Policy::new([Right::Discover, Right::Read]).unwrap(),
            Protected::new([]).unwrap(),
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        let mut budget = Budget::new(&cancel, Duration::from_secs(2), 4096);
        let results = search(&root, "東京", &mut budget).unwrap();
        assert_eq!(
            results
                .matches
                .iter()
                .map(|m| (m.start, m.finish))
                .collect::<Vec<_>>(),
            [(2, 8), (10, 16)]
        );
        assert!(!results.truncated);
        assert!(search(&root, "", &mut budget).is_err());
        std::fs::write(dir.path().join("many.txt"), "x".repeat(100)).unwrap();
        assert!(search(&root, "x", &mut budget).unwrap().truncated);
        cancel.store(true, std::sync::atomic::Ordering::Release);
        assert!(search(&root, "東京", &mut budget).is_err());
    }
}
