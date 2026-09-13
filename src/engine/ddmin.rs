use anyhow::Result;

#[derive(Debug, Clone)]
pub struct DdminOutcome<T> {
    pub minimal: Vec<T>,
    pub tests: usize,
}

/// Return a 1-minimal failure-inducing subset using the classic ddmin strategy.
///
/// `reproduces_failure` must be deterministic for a given subset. The caller is
/// responsible for repetition/stability checks before returning `true`.
pub fn minimize<T, F>(items: &[T], mut reproduces_failure: F) -> Result<DdminOutcome<T>>
where
    T: Clone,
    F: FnMut(&[T]) -> Result<bool>,
{
    if items.is_empty() {
        return Ok(DdminOutcome {
            minimal: Vec::new(),
            tests: 0,
        });
    }

    let mut current = items.to_vec();
    let mut granularity = 2_usize.min(current.len());
    let mut tests = 0_usize;

    while current.len() >= 2 {
        let partitions = partition(&current, granularity);
        let mut reduced = false;

        for subset in &partitions {
            tests += 1;
            if reproduces_failure(subset)? {
                current = subset.clone();
                granularity = 2_usize.min(current.len());
                reduced = true;
                break;
            }
        }
        if reduced {
            continue;
        }

        for omitted in 0..partitions.len() {
            let complement = complement(&partitions, omitted);
            if complement.is_empty() {
                continue;
            }
            tests += 1;
            if reproduces_failure(&complement)? {
                current = complement;
                granularity = granularity
                    .saturating_sub(1)
                    .max(2)
                    .min(current.len());
                reduced = true;
                break;
            }
        }
        if reduced {
            continue;
        }

        if granularity >= current.len() {
            break;
        }
        granularity = (granularity * 2).min(current.len());
    }

    Ok(DdminOutcome {
        minimal: current,
        tests,
    })
}

fn partition<T: Clone>(items: &[T], parts: usize) -> Vec<Vec<T>> {
    if items.is_empty() {
        return Vec::new();
    }

    let parts = parts.max(1).min(items.len());
    let base = items.len() / parts;
    let remainder = items.len() % parts;
    let mut result = Vec::with_capacity(parts);
    let mut start = 0_usize;

    for index in 0..parts {
        let extra = if index < remainder { 1 } else { 0 };
        let width = base + extra;
        let end = start + width;
        result.push(items[start..end].to_vec());
        start = end;
    }

    result
}

fn complement<T: Clone>(partitions: &[Vec<T>], omitted: usize) -> Vec<T> {
    partitions
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != omitted)
        .flat_map(|(_, part)| part.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_single_failure_inducing_item() {
        let items = vec!["a", "b", "c", "d"];
        let result = minimize(&items, |subset| Ok(subset.contains(&"c"))).unwrap();
        assert_eq!(result.minimal, vec!["c"]);
    }

    #[test]
    fn preserves_interacting_pair_when_neither_item_fails_alone() {
        let items = vec!["path", "locale", "timezone", "hostname"];
        let result = minimize(&items, |subset| {
            Ok(subset.contains(&"locale") && subset.contains(&"timezone"))
        })
        .unwrap();

        assert_eq!(result.minimal.len(), 2);
        assert!(result.minimal.contains(&"locale"));
        assert!(result.minimal.contains(&"timezone"));
    }

    #[test]
    fn partition_covers_every_item_once() {
        let partitions = partition(&[1, 2, 3, 4, 5], 3);
        assert_eq!(partitions, vec![vec![1, 2], vec![3, 4], vec![5]]);
    }
}
