//! Dotted version numbers such as driver versions (`8.0.197.0`) and release tags (`v0.2.1`).

use std::cmp::Ordering;

/// Compares dotted version strings number by number, so `8.0.197.0` is older than `8.0.213.0`.
pub fn compare_versions(first_version: &str, second_version: &str) -> Ordering {
    let parse_parts = |version: &str| -> Vec<u64> {
        version
            .trim()
            .trim_start_matches(['v', 'V'])
            .split('.')
            .map(|part| part.trim().parse().unwrap_or(0))
            .collect()
    };
    let first_parts = parse_parts(first_version);
    let second_parts = parse_parts(second_version);
    let part_count = first_parts.len().max(second_parts.len());
    (0..part_count)
        .map(|index| {
            let first_part = first_parts.get(index).copied().unwrap_or(0);
            let second_part = second_parts.get(index).copied().unwrap_or(0);
            first_part.cmp(&second_part)
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::compare_versions;
    use std::cmp::Ordering;

    #[test]
    fn older_build_number_compares_as_less() {
        assert_eq!(compare_versions("8.0.197.0", "8.0.213.0"), Ordering::Less);
    }

    #[test]
    fn missing_trailing_parts_count_as_zero() {
        assert_eq!(compare_versions("8.0", "8.0.0.0"), Ordering::Equal);
    }

    #[test]
    fn release_tag_prefix_is_ignored() {
        assert_eq!(compare_versions("v0.2.1", "0.2.0"), Ordering::Greater);
    }
}
